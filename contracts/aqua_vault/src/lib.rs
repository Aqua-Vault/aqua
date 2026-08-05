#![no_std]
#![allow(clippy::redundant_else)]

//! # Aqua Vault
//!
//! A no-loss prize-linked savings vault for Stellar (Soroban, soroban-sdk
//! 27.0.5). Users deposit a stablecoin into a shared vault; the vault forwards
//! the principal into a yield pool (Blend, or the bundled `mock_pool`); and
//! each draw period 100% of the pooled yield is awarded to a single depositor
//! selected by weighted randomness. Principal is always withdrawable in full.
//!
//! ## Flow
//!
//! ```text
//! User --USDC--> Vault --deposit--> YieldPool
//! YieldPool --yield--> Vault --prize--> Winner
//! ```
//!
//! ## Modules
//!
//! * `storage` — persistent vs instance storage layout, `DataKey` variants,
//!   and the depositor registry that supplies draw weights.
//! * `events` — event topics and payload shapes (`aqua_*`).
//! * `errors` — `AquaError` codes and when each fires.
//! * `blend_adapter` — the swappable yield-pool integration layer.
//!
//! ## Design notes
//!
//! * **Zero-loss**: a withdraw is capped at the caller's recorded principal, so
//!   prize payouts can never reduce anyone's balance.
//! * **Weighted draw**: each depositor's win probability equals their share of
//!   `total_deposits`, using the CAP-0074 on-chain PRNG (`env.prng()`).
//! * **Admin escape hatch**: the admin may force a draw before the interval
//!   elapses; anyone may trigger one once `draw_interval` seconds have passed.

mod blend_adapter;
mod draw;
mod errors;
mod events;
mod storage;

use blend_adapter::clients as pool;
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

use draw::select_weighted_winner;
pub use errors::AquaError;
pub use storage::{DataKey, VaultStats};

pub(crate) const DEFAULT_DRAW_INTERVAL_SECS: u64 = 86_400; // 24h
pub(crate) const MAX_DEPOSITORS_DETAIL: usize = 100;

/// Weighted random outcome produced by drawing a depositor. Exposed so that the
/// selection routine is independently unit-testable without touching the chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawOutcome {
    /// The depositor whose cumulative weight range contained the PRNG roll.
    pub winner: Address,
    /// Raw `env.prng()` value used for the selection (auditable on-chain).
    pub roll: u64,
    /// Sum of all depositor balances at selection time.
    pub total_weight: i128,
    /// The depositors considered, in weight-accumulation order.
    pub participants: Vec<Address>,
}

/// Result of an `execute_prize_draw` cycle. A draw either awards the full
/// accrued yield to a single depositor ([`DrawResult::Awarded`]) or completes
/// without a prize because there was nothing to award ([`DrawResult::Skipped`]).
/// A skipped cycle still advances the draw timer and emits `aqua_draw_skipped`,
/// so callers never see a dead-end "draw ready" state that always reverts.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawResult {
    Awarded(DrawOutcome),
    Skipped,
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
    ///
    /// When the admin has set a per-user deposit cap
    /// ([`set_max_deposit_per_interval`]), a deposit that would push the
    /// caller's *standing balance* above the cap reverts with
    /// [`AquaError::RateLimitExceeded`]. New deposits are also rejected while
    /// the pause circuit breaker is engaged ([`AquaError::Paused`]).
    pub fn deposit(e: Env, from: Address, amount: i128) -> core::result::Result<i128, AquaError> {
        from.require_auth();
        storage::guard_initialized(&e)?;
        if storage::is_locked(&e) {
            return Err(AquaError::Reentrancy);
        }
        if storage::is_paused(&e) {
            return Err(AquaError::Paused);
        }
        if amount <= 0 {
            return Err(AquaError::AmountMustBePositive);
        }

        // Anti-whale throttle: cap bounds the user's standing balance, not
        // cumulative deposits over time, so semantics stay simple and
        // withdrawals are never affected.
        let cap = storage::max_deposit_per_interval(&e);
        if cap > 0 {
            let current = storage::user_balance(&e, &from);
            if amount.saturating_add(current) > cap {
                return Err(AquaError::RateLimitExceeded);
            }
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
    ///
    /// Deliberately **not** gated by the pause circuit breaker: users must
    /// always be able to exit, even (especially) during an incident.
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
    ///
    /// If no positive yield is available this cycle, the draw is **skipped**
    /// rather than reverting: the timer is re-armed to a full interval and an
    /// `aqua_draw_skipped` event is emitted, returning [`DrawResult::Skipped`].
    /// The draw is also halted while the pause circuit breaker is engaged.
    pub fn execute_prize_draw(e: Env) -> core::result::Result<DrawResult, AquaError> {
        storage::guard_initialized(&e)?;
        if storage::is_locked(&e) {
            return Err(AquaError::Reentrancy);
        }
        if storage::is_paused(&e) {
            return Err(AquaError::Paused);
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
            // Skipped cycle: re-arm the timer so the next draw is eligible a
            // fresh interval from now, and signal the skip on-chain instead of
            // wedging the cycle in a perpetual "draw ready" state.
            storage::set_last_draw_time(&e, e.ledger().timestamp());
            events::draw_skipped(&e, total, events::SKIP_REASON_NO_YIELD);
            return Ok(DrawResult::Skipped);
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

        Ok(DrawResult::Awarded(outcome))
    }

    /// Public stats view: total locked, live yield, seconds until next draw,
    /// the (capped) participant list, and the pause state.
    pub fn get_vault_stats(e: Env) -> core::result::Result<VaultStats, AquaError> {
        storage::guard_initialized(&e)?;
        let asset = storage::asset(&e);
        let vault = e.current_contract_address();
        let yp = storage::yield_pool(&e);

        let total = storage::total_deposits(&e);
        let pool_value = pool::pool_balance(&e, &yp, &asset, &vault);
        let yield_amount = pool_value.saturating_sub(total).max(0);

        let interval = storage::draw_interval(&e);
        let elapsed = e
            .ledger()
            .timestamp()
            .saturating_sub(storage::last_draw_time(&e));
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
            paused: storage::is_paused(&e),
        })
    }

    /// The contract's admin address.
    pub fn get_admin(e: Env) -> core::result::Result<Address, AquaError> {
        storage::guard_initialized(&e)?;
        Ok(storage::admin(&e))
    }

    /// A single depositor's current principal balance.
    pub fn get_user_balance(e: Env, user: Address) -> core::result::Result<i128, AquaError> {
        storage::guard_initialized(&e)?;
        Ok(storage::user_balance(&e, &user))
    }

    /// Current per-user deposit cap (`0` = unlimited).
    pub fn get_max_deposit_per_interval(e: Env) -> core::result::Result<i128, AquaError> {
        storage::guard_initialized(&e)?;
        Ok(storage::max_deposit_per_interval(&e))
    }

    /// Emergency circuit breaker: halt new deposits and draws. Withdrawals
    /// remain open so every depositor can exit with full principal.
    pub fn pause(e: Env) -> core::result::Result<(), AquaError> {
        storage::guard_initialized(&e)?;
        let admin = storage::admin(&e);
        admin.require_auth();
        if storage::is_paused(&e) {
            return Ok(());
        }
        storage::set_paused(&e, true);
        events::paused(&e, &admin);
        Ok(())
    }

    /// Disengage the emergency circuit breaker.
    pub fn unpause(e: Env) -> core::result::Result<(), AquaError> {
        storage::guard_initialized(&e)?;
        let admin = storage::admin(&e);
        admin.require_auth();
        if !storage::is_paused(&e) {
            return Ok(());
        }
        storage::set_paused(&e, false);
        events::unpaused(&e, &admin);
        Ok(())
    }

    /// Configure the per-user standing-balance cap (anti-whale). `None` clears
    /// the cap back to unlimited (`0`). Admin-only.
    pub fn set_max_deposit_per_interval(
        e: Env,
        amount: Option<i128>,
    ) -> core::result::Result<(), AquaError> {
        storage::guard_initialized(&e)?;
        let admin = storage::admin(&e);
        admin.require_auth();
        let cap = amount.unwrap_or(0);
        if cap < 0 {
            return Err(AquaError::InvalidConfig);
        }
        storage::set_max_deposit_per_interval(&e, cap);
        Ok(())
    }
}

#[cfg(feature = "testutils")]
mod test;
#[cfg(feature = "testutils")]
mod test_draw;

#[cfg(feature = "testutils")]
mod fuzz_test;
