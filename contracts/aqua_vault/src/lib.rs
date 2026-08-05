#![no_std]
#![allow(clippy::redundant_else)]

mod blend_adapter;
mod events;
mod errors;
mod storage;

use blend_adapter::clients as pool;
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

pub use errors::AquaError;
pub use storage::{DataKey, VaultStats};

pub(crate) const DEFAULT_DRAW_INTERVAL_SECS: u64 = 86_400; // 24h
pub(crate) const MAX_DEPOSITORS_DETAIL: usize = 100;

/// Weighted random outcome produced by drawing a depositor. Exposed so that the
/// selection routine is independently unit-testable without touching the chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawOutcome {
    pub winner: Address,
    pub roll: u64,
    pub total_weight: i128,
    pub participants: Vec<Address>,
}

#[contract]
pub struct AquaVault;

#[contractimpl]
impl AquaVault {
    /// Deploy-time configuration. May only be called once.
    /// `draw_interval` is in seconds; pass `None` to keep the 24h default.
    ///
    /// # Arguments
    /// * `admin`  - address authorized to force a draw early (and manage pool).
    /// * `asset`  - deposit token contract (testnet USDC / Stellar Asset Contract).
    /// * `yield_pool` - Blend pool (or mock) contract Aqua accrues yield in.
    /// * `draw_interval` - seconds between prize draws.
    pub fn initialize(
        e: Env,
        admin: Address,
        asset: Address,
        yield_pool: Address,
        draw_interval: Option<u64>,
    ) -> core::result::Result<(), AquaError> {
        if storage::has_admin(&e) {
            return Err(AquaError::AlreadyInitialized);
        }
        admin.require_auth();
        let interval = draw_interval.unwrap_or(DEFAULT_DRAW_INTERVAL_SECS);
        if interval == 0 {
            return Err(AquaError::InvalidConfig);
        }
        storage::set_admin(&e, &admin);
        storage::set_asset(&e, &asset);
        storage::set_yield_pool(&e, &yield_pool);
        storage::set_draw_interval(&e, interval);
        storage::set_last_draw_time(&e, e.ledger().timestamp());
        storage::set_total_deposits(&e, 0);
        let empty: Vec<Address> = Vec::new(&e);
        storage::set_depositors(&e, &empty);
        events::initialized(&e, &admin, &asset, &yield_pool, interval);
        Ok(())
    }

    /// Deposit `amount` of `asset` into the vault on behalf of `from`. The
    /// freshly received principal is immediately forwarded to the yield pool so
    /// every unit starts earning. A user's win probability is their share of
    /// `total_deposits`, so holding more means a greater chance at the prize
    /// while always preserving 100% of the principal.
    ///
    /// Follows Checks-Effects-Interactions: all internal accounting is updated
    /// **before** any external token/pool call, so a malicious yield pool that
    /// re-enters the vault mid-transfer observes already-consistent state and
    /// can never double-credit an account.
    pub fn deposit(e: Env, from: Address, amount: i128) -> core::result::Result<i128, AquaError> {
        from.require_auth();
        storage::guard_initialized(&e)?;
        if storage::is_locked(&e) {
            return Err(AquaError::Reentrancy);
        }
        if amount <= 0 {
            return Err(AquaError::AmountMustBePositive);
        }

        // 1. internal accounting (CEI: effects before interactions)
        let prev = storage::user_balance(&e, &from);
        storage::set_user_balance(&e, &from, prev + amount);
        storage::set_total_deposits(&e, storage::total_deposits(&e) + amount);
        storage::register_depositor(&e, &from);

        let asset = storage::asset(&e);
        let vault = e.current_contract_address();
        let yp = storage::yield_pool(&e);

        // 2. user -> vault
        token::Client::new(&e, &asset).transfer(&from, &vault, &amount);

        // 3. vault -> yield pool (accrues interest from now on)
        pool::pool_deposit(&e, &yp, &asset, &vault, amount);

        events::deposited(&e, &from, amount, prev + amount);
        Ok(prev + amount)
    }

    /// Withdraw up to `amount` of principal. Guarantees the user can always get
    /// back their full deposit (zero-loss) regardless of prize history.
    ///
    /// Also follows Checks-Effects-Interactions: the principal is deducted
    /// from internal accounting before the pool/token transfer, so re-entrant
    /// withdrawals observe up-to-date balances and cannot be double-credited.
    pub fn withdraw(e: Env, from: Address, amount: i128) -> core::result::Result<i128, AquaError> {
        from.require_auth();
        storage::guard_initialized(&e)?;
        if storage::is_locked(&e) {
            return Err(AquaError::Reentrancy);
        }
        if amount <= 0 {
            return Err(AquaError::AmountMustBePositive);
        }

        let balance = storage::user_balance(&e, &from);
        if amount > balance {
            return Err(AquaError::InsufficientBalance);
        }

        // 1. internal accounting (CEI: effects before interactions)
        storage::set_user_balance(&e, &from, balance - amount);
        storage::set_total_deposits(&e, storage::total_deposits(&e) - amount);
        storage::unregister_depositor(&e, &from);

        let asset = storage::asset(&e);
        let vault = e.current_contract_address();
        let yp = storage::yield_pool(&e);

        // 2. pull principal back out of the pool into the vault
        pool::pool_withdraw(&e, &yp, &asset, &vault, amount);

        // 3. vault -> user
        token::Client::new(&e, &asset).transfer(&vault, &from, &amount);

        events::withdrawn(&e, &from, amount, balance - amount);
        Ok(balance - amount)
    }

    /// Award 100% of the accrued yield (current pool value minus total
    /// deposits) to a single depositor drawn at random. Selection is weighted
    /// by each depositor's share of `total_deposits` using the on-chain
    /// CAP-0074 PRNG supplied by `env.prng()`.
    ///
    /// Any caller may trigger a draw once `draw_interval` seconds have elapsed
    /// since `last_draw_time`; the admin may force one at any time.
    ///
    /// The multi-step prize path (pool redeem → winner transfer) is guarded by
    /// a re-entrancy `Locked` flag: a malicious pool that calls back into the
    /// vault mid-draw hits the [`AquaError::Reentrancy`] guard instead of
    /// reading half-updated state or running a second draw.
    pub fn execute_prize_draw(e: Env) -> core::result::Result<DrawOutcome, AquaError> {
        storage::guard_initialized(&e)?;
        if storage::is_locked(&e) {
            return Err(AquaError::Reentrancy);
        }

        if !storage::can_draw(&e) {
            return Err(AquaError::TooEarly);
        }

        let asset = storage::asset(&e);
        let vault = e.current_contract_address();
        let yp = storage::yield_pool(&e);
        let total = storage::total_deposits(&e);

        if total <= 0 {
            return Err(AquaError::NoDepositors);
        }

        // Yield = whatever value the pool has beyond principal.
        let pool_value = pool::pool_balance(&e, &yp, &asset, &vault);
        let yield_amount = pool_value.saturating_sub(total);
        if yield_amount <= 0 {
            return Err(AquaError::NoYieldToAward);
        }

        // Lock the vault while the draw's external interactions are in flight.
        // A re-entrant call during `pool_withdraw`/`transfer` must not be able
        // to mutate state that this draw already depends on.
        storage::set_locked(&e, true);

        // Draw the weighted winner via the CAP-0074 on-chain PRNG.
        let outcome = select_weighted_winner(&e);
        pool::pool_withdraw(&e, &yp, &asset, &vault, yield_amount);
        token::Client::new(&e, &asset).transfer(&vault, &outcome.winner, &yield_amount);

        // Release the guard before returning.
        storage::set_locked(&e, false);

        storage::set_last_draw_time(&e, e.ledger().timestamp());
        events::prize_awarded(&e, &outcome.winner, yield_amount, outcome.roll);

        Ok(outcome)
    }

    /// Public stats view: total locked, live yield, seconds until next draw,
    /// and the (capped) participant list.
    pub fn get_vault_stats(e: Env) -> core::result::Result<VaultStats, AquaError> {
        storage::guard_initialized(&e)?;
        let asset = storage::asset(&e);
        let vault = e.current_contract_address();
        let yp = storage::yield_pool(&e);

        let total = storage::total_deposits(&e);
        let pool_value = pool::pool_balance(&e, &yp, &asset, &vault);
        let yield_amount = pool_value.saturating_sub(total).max(0);

        let interval = storage::draw_interval(&e);
        let elapsed = e.ledger().timestamp().saturating_sub(storage::last_draw_time(&e));
        let seconds_until_next_draw = interval.saturating_sub(elapsed);

        let mut participants: Vec<Address> = Vec::new(&e);
        for addr in storage::depositors(&e).iter().take(MAX_DEPOSITORS_DETAIL) {
            participants.push_back(addr);
        }

        Ok(VaultStats {
            total_deposits: total,
            current_yield: yield_amount,
            seconds_until_next_draw,
            participants,
        })
    }

    /// Read helpers for integration testers / audits.
    pub fn get_admin(e: Env) -> core::result::Result<Address, AquaError> {
        storage::guard_initialized(&e)?;
        Ok(storage::admin(&e))
    }

    pub fn get_user_balance(e: Env, user: Address) -> core::result::Result<i128, AquaError> {
        storage::guard_initialized(&e)?;
        Ok(storage::user_balance(&e, &user))
    }
}

/// Weighted random selection over all current depositors using the on-chain
/// PRNG. A depositor's probability of winning is exactly
/// `balance / total_deposits`. Kept separate from the environment-mutating API
/// so it can be unit tested directly with a seeded PRNG.
fn select_weighted_winner(e: &Env) -> DrawOutcome {
    let mut total: i128 = 0;
    let mut weighted: Vec<(Address, i128)> = Vec::new(e);

    for addr in storage::depositors(e).iter() {
        let bal = storage::user_balance(e, &addr);
        if bal > 0 {
            total = total.saturating_add(bal);
            weighted.push_back((addr, total));
        }
    }

    debug_assert!(total > 0, "caller must guard total > 0 before selecting");

    // u64 roll over [0, total), then pick the depositor whose cumulative upper
    // bound first exceeds the roll.
    let roll: u64 = e.prng().gen_range(0..(total as u64));
    let mut winner = weighted.first().unwrap().0.clone();
    for (addr, cumulative) in weighted.iter() {
        if (roll as i128) < cumulative {
            winner = addr.clone();
            break;
        }
    }

    let mut participants: Vec<Address> = Vec::new(e);
    for (addr, _) in weighted.iter() {
        participants.push_back(addr.clone());
    }

    DrawOutcome {
        winner,
        roll,
        total_weight: total,
        participants,
    }
}

#[cfg(feature = "testutils")]
mod test;