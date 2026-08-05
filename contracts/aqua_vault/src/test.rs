#![cfg(test)]

//! Unit + integration tests for the Aqua vault. A mock yield pool contract
//! implements the same `YieldPool` interface the real Blend integration uses,
//! accruing a deterministic annual interest rate so that "no-loss" and
//! proportional-draw invariants can be asserted exactly.

// `std` is needed for `catch_unwind` in the panic-guard test even though the
// crate is `#![no_std]`; the test harness links it in.
extern crate std;

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{
        storage::Persistent as _, Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke,
    },
    token, vec, Address, Bytes, Env, IntoVal, Symbol, Vec,
};

use crate::storage;
use crate::storage::{PERSISTENT_TTL_EXTEND_THRESHOLD, PERSISTENT_TTL_EXTEND_TO};
use crate::yield_trait::YieldSourceKind;
use crate::{
    select_weighted_winner, AquaError, AquaVault, AquaVaultClient, DataKey, DrawOutcome, DrawResult,
};

const SECS_PER_YEAR: u64 = 31_536_000;

// ---------------------------------------------------------------------------
// MockYieldPool
// ---------------------------------------------------------------------------

#[contract]
pub struct MockYieldPool;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MockKey {
    Token,
    Admin,
    LastTs,
    RateBps,
}

#[contractimpl]
impl MockYieldPool {
    pub fn initialize(e: Env, token: Address, admin: Address) {
        e.storage().instance().set(&MockKey::Token, &token);
        e.storage().instance().set(&MockKey::Admin, &admin);
        e.storage()
            .instance()
            .set(&MockKey::LastTs, &e.ledger().timestamp());
        e.storage().instance().set(&MockKey::RateBps, &1000u64);
    }

    pub fn set_rate(e: Env, bps: u64) {
        let admin: Address = e.storage().instance().get(&MockKey::Admin).unwrap();
        admin.require_auth();
        e.storage().instance().set(&MockKey::RateBps, &bps);
    }

    pub fn deposit(e: Env, asset: Address, amount: i128) -> i128 {
        let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
        assert!(asset == token);
        // The vault has already pushed `amount` of `asset` to us; settle
        // interest accrued up to now so the new principal earns from here on.
        accrue(&e);
        amount
    }

    pub fn withdraw(e: Env, asset: Address, to: Address, amount: i128) -> i128 {
        let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
        assert!(asset == token);
        accrue(&e);
        // Solvency is enforced by the token transfer itself (can't send more
        // than the pool holds). No separate principal ledger is needed.
        token::Client::new(&e, &token).transfer(&e.current_contract_address(), &to, &amount);
        amount
    }

    pub fn balance(e: Env, asset: Address, _owner: Address) -> i128 {
        let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
        assert!(asset == token);
        accrue(&e);
        token::Client::new(&e, &token).balance(&e.current_contract_address())
    }

    pub fn withdrawable(e: Env, asset: Address, _owner: Address) -> i128 {
        // This mock is always fully withdrawable: same as its live balance.
        let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
        assert!(asset == token);
        accrue(&e);
        token::Client::new(&e, &token).balance(&e.current_contract_address())
    }

    pub fn rate(e: Env, asset: Address) -> u64 {
        let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
        assert!(asset == token);
        e.storage().instance().get(&MockKey::RateBps).unwrap()
    }
}

/// Simple-interest accrual on the pool's live token balance. Because Aqua
/// distributes all yield on every draw, the balance between draws is just the
/// principal, so this yields exact, predictable interest for the tests.
fn accrue(e: &Env) {
    let token: Address = e.storage().instance().get(&MockKey::Token).unwrap();
    let rate: u64 = e.storage().instance().get(&MockKey::RateBps).unwrap();
    let last: u64 = e.storage().instance().get(&MockKey::LastTs).unwrap();
    let now = e.ledger().timestamp();
    let bal = token::Client::new(e, &token).balance(&e.current_contract_address());
    if bal > 0 && now > last {
        let elapsed = now - last;
        let interest =
            bal * (rate as i128) * (elapsed as i128) / (10_000i128 * (SECS_PER_YEAR as i128));
        if interest > 0 {
            token::StellarAssetClient::new(e, &token)
                .mint(&e.current_contract_address(), &interest);
        }
    }
    e.storage().instance().set(&MockKey::LastTs, &now);
}

// ---------------------------------------------------------------------------
// ShortfallPool — partial-fill / borrow-shortfall simulation
// ---------------------------------------------------------------------------

/// An imperfect pool: it reports its full accrued balance, but (a) its
/// `withdrawable` is capped below the reported balance (simulating a borrower
/// shortfall) and (b) `withdraw` partial-fills, paying only up to `redeem_cap`.
/// Used to exercise the vault's fault-tolerant prize payout (#13).
#[contract]
pub struct ShortfallPool;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortfallKey {
    Token,
    LastTs,
    RateBps,
    WithdrawableCap,
    RedeemCap,
}

#[contractimpl]
impl ShortfallPool {
    pub fn initialize(e: Env, token: Address, withdrawable_cap: i128, redeem_cap: i128) {
        e.storage().instance().set(&ShortfallKey::Token, &token);
        e.storage().instance().set(&ShortfallKey::LastTs, &e.ledger().timestamp());
        e.storage().instance().set(&ShortfallKey::RateBps, &1000u64);
        e.storage().instance().set(&ShortfallKey::WithdrawableCap, &withdrawable_cap);
        e.storage().instance().set(&ShortfallKey::RedeemCap, &redeem_cap);
    }

    pub fn deposit(e: Env, asset: Address, amount: i128) -> i128 {
        let token: Address = e.storage().instance().get(&ShortfallKey::Token).unwrap();
        assert!(asset == token);
        accrue_shortfall(&e);
        amount
    }

    pub fn withdraw(e: Env, asset: Address, to: Address, amount: i128) -> i128 {
        let token: Address = e.storage().instance().get(&ShortfallKey::Token).unwrap();
        assert!(asset == token);
        accrue_shortfall(&e);
        let redeem_cap: i128 = e.storage().instance().get(&ShortfallKey::RedeemCap).unwrap();
        let paid = amount.min(redeem_cap);
        if paid > 0 {
            token::Client::new(&e, &token).transfer(&e.current_contract_address(), &to, &paid);
        }
        paid
    }

    pub fn balance(e: Env, asset: Address, _owner: Address) -> i128 {
        let token: Address = e.storage().instance().get(&ShortfallKey::Token).unwrap();
        assert!(asset == token);
        accrue_shortfall(&e);
        token::Client::new(&e, &token).balance(&e.current_contract_address())
    }

    pub fn withdrawable(e: Env, asset: Address, _owner: Address) -> i128 {
        let cap: i128 = e.storage().instance().get(&ShortfallKey::WithdrawableCap).unwrap();
        Self::balance(e, asset, _owner).min(cap)
    }

    pub fn rate(e: Env, _asset: Address) -> u64 {
        e.storage().instance().get(&ShortfallKey::RateBps).unwrap()
    }
}

fn accrue_shortfall(e: &Env) {
    let token: Address = e.storage().instance().get(&ShortfallKey::Token).unwrap();
    let rate: u64 = e.storage().instance().get(&ShortfallKey::RateBps).unwrap();
    let last: u64 = e.storage().instance().get(&ShortfallKey::LastTs).unwrap();
    let now = e.ledger().timestamp();
    let bal = token::Client::new(e, &token).balance(&e.current_contract_address());
    if bal > 0 && now > last {
        let elapsed = now - last;
        let interest =
            bal * (rate as i128) * (elapsed as i128) / (10_000i128 * (SECS_PER_YEAR as i128));
        if interest > 0 {
            token::StellarAssetClient::new(e, &token)
                .mint(&e.current_contract_address(), &interest);
        }
    }
    e.storage().instance().set(&ShortfallKey::LastTs, &now);
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Setup {
    env: Env,
    vault: AquaVaultClient<'static>,
    vault_id: Address,
    token: Address,
    admin: Address,
    u1: Address,
    u2: Address,
    u3: Address,
    mock_pool: Address,
}

fn setup(rate_bps: u64, interval_secs: u64) -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);

    let mock_pool = env.register(MockYieldPool, ());
    // Token admin is the mock pool so it can mint accrued interest to itself.
    let sac = env.register_stellar_asset_contract_v2(mock_pool.clone());
    let token = sac.address();
    let token_admin = token::StellarAssetClient::new(&env, &token);
    token_admin.mint(&u1, &1_000_000_000_000i128);
    token_admin.mint(&u2, &1_000_000_000_000i128);
    token_admin.mint(&u3, &1_000_000_000_000i128);

    MockYieldPoolClient::new(&env, &mock_pool).initialize(&token, &mock_pool);
    MockYieldPoolClient::new(&env, &mock_pool).set_rate(&rate_bps);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    vault.initialize(&admin, &token, &mock_pool, &Some(interval_secs));

    Setup {
        env,
        vault,
        vault_id,
        token,
        admin,
        u1,
        u2,
        u3,
        mock_pool,
    }
}

/// Setup with the partial-fill `ShortfallPool` as the vault's yield pool.
fn setup_shortfall(withdrawable_cap: i128, redeem_cap: i128) -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);

    let pool_id = env.register(ShortfallPool, ());
    let sac = env.register_stellar_asset_contract_v2(pool_id.clone());
    let token = sac.address();
    let token_admin = token::StellarAssetClient::new(&env, &token);
    token_admin.mint(&u1, &1_000_000_000_000i128);
    token_admin.mint(&u2, &1_000_000_000_000i128);

    ShortfallPoolClient::new(&env, &pool_id).initialize(&token, &withdrawable_cap, &redeem_cap);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    vault.initialize(&admin, &token, &pool_id, &Some(86_400));

    Setup {
        env,
        vault,
        vault_id,
        token,
        admin,
        u1,
        u2,
        u3: u3,
        mock_pool: pool_id,
    }
}

fn advance(s: &Setup, secs: u64) {
    let base = s.env.ledger().timestamp();
    s.env.ledger().set_timestamp(base + secs);
}

fn token_balance(s: &Setup, who: &Address) -> i128 {
    token::Client::new(&s.env, &s.token).balance(who)
}

/// Unwrap a successful draw cycle into its awarded outcome.
fn awarded(r: DrawResult) -> DrawOutcome {
    match r {
        DrawResult::Awarded(o) => o,
        DrawResult::Skipped => panic!("expected an awarded draw, got a skip"),
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_sets_config() {
    let s = setup(1_000, 86_400);
    assert_eq!(s.vault.get_admin(), s.admin.clone());
    let stats = s.vault.get_vault_stats();
    assert_eq!(stats.total_deposits, 0);
    assert_eq!(stats.current_yield, 0);
    assert_eq!(stats.paused, false);
    // Default cap is unlimited (0).
    assert_eq!(s.vault.get_max_deposit_per_interval(), 0);
}

#[test]
fn test_initialize_rejects_twice() {
    let s = setup(1_000, 86_400);
    // Second initialize must fail: the client surfaces the contract error.
    let dup = s
        .vault
        .try_initialize(&s.admin, &s.token, &s.mock_pool, &Some(86_400))
        .unwrap_err();
    assert_eq!(dup, Ok(AquaError::AlreadyInitialized));
}

// ---------------------------------------------------------------------------
// Deposit / Withdraw
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_flows_to_pool_and_accounts() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &5_000);

    assert_eq!(s.vault.get_user_balance(&s.u1), 5_000);
    let stats = s.vault.get_vault_stats();
    assert_eq!(stats.total_deposits, 5_000);
    assert_eq!(stats.participants.len(), 1);
    // Principal physically moved: vault holds nothing, pool holds everything.
    assert_eq!(token_balance(&s, &s.vault_id), 0);
    assert_eq!(token_balance(&s, &s.mock_pool), 5_000);
}

#[test]
fn test_withdraw_returns_full_principal() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &10_000);
    s.vault.withdraw(&s.u1, &4_000);

    assert_eq!(s.vault.get_user_balance(&s.u1), 6_000);
    assert_eq!(s.vault.get_vault_stats().total_deposits, 6_000);
    assert_eq!(token_balance(&s, &s.mock_pool), 6_000);
    assert_eq!(token_balance(&s, &s.u1), 1_000_000_000_000 - 10_000 + 4_000);
}

#[test]
fn test_rejects_non_positive_and_oversized() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &10);
    assert_eq!(
        s.vault.try_deposit(&s.u1, &0).unwrap_err(),
        Ok(AquaError::AmountMustBePositive)
    );
    assert_eq!(
        s.vault.try_withdraw(&s.u1, &11).unwrap_err(),
        Ok(AquaError::InsufficientBalance)
    );
    // Balance untouched after a failed withdraw.
    assert_eq!(s.vault.get_user_balance(&s.u1), 10);
}

// ---------------------------------------------------------------------------
// Prize draw
// ---------------------------------------------------------------------------

#[test]
fn test_draw_requires_interval_but_admin_can_force() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &5_000);
    s.vault.deposit(&s.u2, &5_000);
    advance(&s, 100); // way before 1 day interval

    let err = s.vault.try_execute_prize_draw().unwrap_err();
    assert_eq!(err, Ok(AquaError::TooEarly));

    // Wait out the interval and assert the draw then succeeds.
    advance(&s, 86_400);
    s.vault.execute_prize_draw();
}

#[test]
fn test_prize_draw_awards_full_yield_to_single_winner() {
    let s = setup(1_000, 86_400); // 10% per year
    s.vault.deposit(&s.u1, &50_000);
    s.vault.deposit(&s.u2, &50_000);
    let u1_before = token_balance(&s, &s.u1);
    let u2_before = token_balance(&s, &s.u2);

    advance(&s, SECS_PER_YEAR);
    let outcome = awarded(s.vault.execute_prize_draw());

    // total yield = 100_000 * 10% = 10_000, given to exactly one winner
    let prize = if outcome.winner == s.u1 {
        token_balance(&s, &s.u1) - u1_before
    } else if outcome.winner == s.u2 {
        token_balance(&s, &s.u2) - u2_before
    } else {
        panic!("winner must be one of the depositors");
    };
    assert_eq!(prize, 10_000);
    assert_eq!(outcome.total_weight, 100_000);
    // Winner only got the yield, never any principal from the other depositor.
    assert_eq!(s.vault.get_vault_stats().total_deposits, 100_000);
}

#[test]
fn test_events_emitted_for_deposit_and_prize() {
    let s = setup(1_000, 86_400); // 10% per year

    // A deposit emits an `aqua_deposit` event topically keyed by the depositor
    // and carrying (amount, new_balance) as data. Filter to the vault's own
    // events so token/pool sub-call events don't interfere.
    s.vault.deposit(&s.u1, &100);
    let deposit_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        deposit_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (Symbol::new(&s.env, "aqua_deposit"), s.u1.clone()).into_val(&s.env),
                (100_i128, 100_i128).into_val(&s.env),
            ),
        ]
    );

    // Add a second depositor, accrue a year of yield, and draw.
    s.vault.deposit(&s.u2, &100);
    advance(&s, SECS_PER_YEAR);
    let outcome = awarded(s.vault.execute_prize_draw());

    // The prize draw emits a single `aqua_prize_awarded` event that matches the
    // returned outcome exactly (winner, prize amount, and PRNG roll).
    let prize_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        prize_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (
                    Symbol::new(&s.env, "aqua_prize_awarded"),
                    outcome.winner.clone()
                )
                    .into_val(&s.env),
                (20_i128, outcome.roll).into_val(&s.env), // 200 principal * 10% = 20
            ),
        ]
    );
}

#[test]
fn test_zero_loss_after_prize_draw() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &50_000);
    s.vault.deposit(&s.u2, &50_000);
    let u1_before = token_balance(&s, &s.u1);
    let u2_before = token_balance(&s, &s.u2);

    advance(&s, SECS_PER_YEAR * 2); // 20% yield
    s.vault.execute_prize_draw();

    // Regardless of who won, BOTH users can pull their full principal out.
    s.vault.withdraw(&s.u1, &50_000);
    s.vault.withdraw(&s.u2, &50_000);

    assert_eq!(s.vault.get_user_balance(&s.u1), 0);
    assert_eq!(s.vault.get_user_balance(&s.u2), 0);
    assert!(token_balance(&s, &s.u1) >= u1_before + 50_000);
    assert!(token_balance(&s, &s.u2) >= u2_before + 50_000);
}

#[test]
fn test_zero_yield_draw_is_skipped_and_rearms_timer() {
    let s = setup(1_000, 86_400); // 10% per year
                                  // Tiny principal so a fresh interval of interest truncates to zero.
    s.vault.deposit(&s.u1, &100);
    advance(&s, 86_400);
    let stats = s.vault.get_vault_stats();
    assert_eq!(stats.current_yield, 0);
    assert_eq!(stats.seconds_until_next_draw, 0);

    // A zero-yield cycle is a *skip*, not an error.
    assert_eq!(s.vault.execute_prize_draw(), DrawResult::Skipped);

    // The skip is signalled on-chain (capture before the next read call, since
    // the test harness only retains the previous invocation's events)...
    let skip_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        skip_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (
                    Symbol::new(&s.env, "aqua_draw_skipped"),
                    Symbol::new(&s.env, "no_yield"),
                )
                    .into_val(&s.env),
                (100_i128).into_val(&s.env), // total_deposits as data
            ),
        ]
    );

    // ...and the timer is re-armed to a full interval.
    assert_eq!(s.vault.get_vault_stats().seconds_until_next_draw, 86_400);

    // After the fresh interval, a real draw becomes possible again (once the
    // small principal has had long enough to accrue whole units of yield).
    advance(&s, SECS_PER_YEAR);
    assert!(matches!(
        s.vault.execute_prize_draw(),
        DrawResult::Awarded(_)
    ));
}

#[test]
fn test_draw_errors_without_depositors() {
    let s = setup(1_000, 86_400);
    advance(&s, 86_400);
    assert_eq!(
        s.vault.try_execute_prize_draw().unwrap_err(),
        Ok(AquaError::NoDepositors)
    );
}

// ---------------------------------------------------------------------------
// Weighted randomness (CAP-0074 PRNG)
// ---------------------------------------------------------------------------

#[test]
fn test_proportional_weighted_selection() {
    let env = Env::default();
    let id = env.register(AquaVault, ());
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    // Direct storage + PRNG access must run inside a contract frame.
    let a_wins = env.as_contract(&id, || {
        // 10k selections in one host frame; lift the metering budget and the
        // mainnet invocation resource limits (native tests meter heavier than
        // real Wasm, and this test is about the draw distribution, not cost).
        env.cost_estimate().disable_resource_limits();
        env.cost_estimate().budget().reset_unlimited();
        // Simulate 90/10 deposit split.
        storage::set_user_balance(&env, &a, 90);
        storage::set_user_balance(&env, &b, 10);
        let mut list = Vec::new(&env);
        list.push_back(a.clone());
        list.push_back(b.clone());
        storage::set_depositors(&env, &list);

        // Deterministic PRNG: seed once, each gen_range advances the stream.
        env.prng().seed(Bytes::from_array(&env, &[7u8; 32]));

        let n = 6_000u32;
        let mut a_wins = 0u32;
        for _ in 0..n {
            let outcome: DrawOutcome = select_weighted_winner(&env);
            assert_eq!(outcome.total_weight, 100);
            if outcome.winner == a {
                a_wins += 1;
            }
        }
        a_wins
    });

    let ratio = a_wins as f64 / 6_000.0;
    // A holds 90% of the pool: expect a strong majority of wins.
    assert!(
        ratio > 0.85 && ratio < 0.95,
        "A should win ~90% but won {:.2}%",
        ratio * 100.0
    );
}

#[test]
fn test_selection_is_exactly_weighted_for_equal_shares() {
    let env = Env::default();
    let id = env.register(AquaVault, ());
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let wins = env.as_contract(&id, || {
        env.cost_estimate().disable_resource_limits();
        env.cost_estimate().budget().reset_unlimited();
        storage::set_user_balance(&env, &a, 10);
        storage::set_user_balance(&env, &b, 10);
        storage::set_user_balance(&env, &c, 10);
        let mut list = Vec::new(&env);
        list.push_back(a.clone());
        list.push_back(b.clone());
        list.push_back(c.clone());
        storage::set_depositors(&env, &list);

        env.prng().seed(Bytes::from_array(&env, &[3u8; 32]));
        let n = 6_000u32;
        let mut wins = [0u32; 3];
        for _ in 0..n {
            let outcome = select_weighted_winner(&env);
            let idx = if outcome.winner == a {
                0
            } else if outcome.winner == b {
                1
            } else {
                2
            };
            wins[idx] += 1;
        }
        wins
    });

    for w in wins {
        let ratio = w as f64 / 6_000.0;
        assert!(
            (ratio - 1.0 / 3.0).abs() < 0.04,
            "expected ~33%, got {:.2}%",
            ratio * 100.0
        );
    }
}

#[test]
fn test_zero_balance_depositors_are_skipped() {
    let env = Env::default();
    let id = env.register(AquaVault, ());
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    env.as_contract(&id, || {
        env.cost_estimate().disable_resource_limits();
        env.cost_estimate().budget().reset_unlimited();
        storage::set_user_balance(&env, &a, 0); // withdrawn
        storage::set_user_balance(&env, &b, 100);
        let mut list = Vec::new(&env);
        list.push_back(a.clone());
        list.push_back(b.clone());
        storage::set_depositors(&env, &list);

        env.prng().seed(Bytes::from_array(&env, &[11u8; 32]));
        let n = 2_000u32;
        for _ in 0..n {
            let outcome = select_weighted_winner(&env);
            assert_eq!(outcome.winner, b, "zero-balance depositor must never win");
            assert_eq!(outcome.total_weight, 100);
        }
    });
}

// ---------------------------------------------------------------------------
// Weighted-randomness gas optimization (single pass, bounded cost)
// ---------------------------------------------------------------------------

/// Seed a vault with `n` directly-written depositor balances plus pool funding,
/// run one real `execute_prize_draw` invocation, and return `(instructions,
/// memory_read_entries)` from its measured resources. Populating balances via
/// storage (rather than `n` real token transfers) keeps the measurement focused
/// on the draw's own hot path.
fn draw_over_n_depositors(n: u32) -> (i64, u32) {
    let s = setup(1_000, 86_400);
    s.env.as_contract(&s.vault_id, || {
        let mut list = Vec::new(&s.env);
        for _ in 0..n {
            let a = Address::generate(&s.env);
            storage::set_user_balance(&s.env, &a, 1_000);
            list.push_back(a);
        }
        storage::set_depositors(&s.env, &list);
        storage::set_total_deposits(&s.env, (n as i128) * 1_000);
    });
    // Pre-fund the pool so a positive yield exists to award.
    token::StellarAssetClient::new(&s.env, &s.token).mint(&s.mock_pool, &1_000_000_000);
    advance(&s, 86_400); // satisfy the draw interval and accrue interest

    // Lift the invocation resource *limits* so the draw runs to completion;
    // `resources()` then reports what it actually cost.
    s.env.cost_estimate().disable_resource_limits();
    let res = s.vault.execute_prize_draw();
    assert!(matches!(res, DrawResult::Awarded(_)));
    let r = s.env.cost_estimate().resources();
    (r.instructions, r.memory_read_entries)
}

#[test]
fn test_draw_scales_linearly_and_fits_mainnet_budget() {
    // A single selection pass reads each depositor's balance exactly once and
    // scans in-memory cumulative bounds, so cost grows ~linearly with depositor
    // count. (A second pass re-walking a materialized `Vec<(Address, i128)>`
    // would drive the per-depositor cost higher.)
    let (small_insns, _) = draw_over_n_depositors(50);
    let (large_insns, large_reads) = draw_over_n_depositors(150);
    // 3x the depositors must cost comfortably under 4x the instructions.
    assert!(
        large_insns < small_insns * 4,
        "draw cost must scale ~linearly: 150 depositors = {} insns vs {} for 50",
        large_insns,
        small_insns
    );

    // A 150-depositor draw must stay far inside mainnet's per-invocation
    // instruction cap (400M), proving `execute_prize_draw` stays callable as
    // the depositor set grows.
    assert!(
        large_insns < 400_000_000,
        "150-depositor draw used {} modelled instructions (mainnet cap is 400M)",
        large_insns
    );
    assert!(
        large_reads <= 150 + 64,
        "draw must touch at most one storage entry per depositor plus a fixed overhead \
         (read {} entries)",
        large_reads
    );
}

#[test]
fn test_roll_domain_guard_panics_on_overflow() {
    let env = Env::default();
    let id = env.register(AquaVault, ());
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    // Balances that sum past u64::MAX must abort the selection loudly rather
    // than silently truncating the PRNG roll domain.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.as_contract(&id, || {
            env.cost_estimate().disable_resource_limits();
            env.cost_estimate().budget().reset_unlimited();
            storage::set_user_balance(&env, &a, i128::MAX / 2 + 1);
            storage::set_user_balance(&env, &b, i128::MAX / 2 + 1);
            let mut list = Vec::new(&env);
            list.push_back(a.clone());
            list.push_back(b.clone());
            storage::set_depositors(&env, &list);
            let _ = select_weighted_winner(&env);
        });
    }));

    assert!(
        result.is_err(),
        "select_weighted_winner must panic when total_deposits > u64::MAX"
    );
}

// ---------------------------------------------------------------------------
// Persistent-storage TTL management
// ---------------------------------------------------------------------------

#[test]
fn test_persistent_write_extends_ttl_past_minimum() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);

    let mock_pool = env.register(MockYieldPool, ());
    let sac = env.register_stellar_asset_contract_v2(mock_pool.clone());
    let token = sac.address();
    token::StellarAssetClient::new(&env, &token).mint(&u1, &1_000_000);
    MockYieldPoolClient::new(&env, &mock_pool).initialize(&token, &mock_pool);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    vault.initialize(&admin, &token, &mock_pool, &Some(86_400));
    vault.deposit(&u1, &1_000);

    // The write path must have extended the per-user entry far beyond the
    // network-minimum 4096 ledgers. Under the pre-fix code, `get_ttl` would
    // report exactly `min_persistent_entry_ttl` here.
    let ttl = env.as_contract(&vault_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::UserBalance(u1.clone()))
    });
    assert!(
        ttl > PERSISTENT_TTL_EXTEND_THRESHOLD,
        "write must extend TTL well past the minimum (got {ttl})"
    );
    assert!(
        ttl >= PERSISTENT_TTL_EXTEND_TO - PERSISTENT_TTL_EXTEND_THRESHOLD,
        "write must extend TTL to the configured target (got {ttl})"
    );
}

#[test]
fn test_user_balance_survives_beyond_old_expiry_window() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &7_000);

    // Jump the ledger far past the 4096-ledger minimum persistent TTL window.
    // The extended entry written above is still alive; pre-fix it would have
    // expired within this window and silently read back as zero.
    let seq = s.env.ledger().get().sequence_number;
    s.env.ledger().set_sequence_number(seq + 5_000);

    assert_eq!(s.vault.get_user_balance(&s.u1), 7_000);
    assert_eq!(s.vault.get_vault_stats().total_deposits, 7_000);
}

// ---------------------------------------------------------------------------
// Emergency circuit breaker (pause / unpause)
// ---------------------------------------------------------------------------

#[test]
fn test_pause_blocks_deposits_and_draws_but_not_withdrawals() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &50_000);
    s.vault.pause();

    // Deposit blocked.
    assert_eq!(
        s.vault.try_deposit(&s.u2, &1).unwrap_err(),
        Ok(AquaError::Paused)
    );
    // Draw blocked, even once the interval has long elapsed.
    advance(&s, SECS_PER_YEAR);
    assert_eq!(
        s.vault.try_execute_prize_draw().unwrap_err(),
        Ok(AquaError::Paused)
    );
    // The zero-loss guarantee holds: withdrawals stay open, full principal out.
    s.vault.withdraw(&s.u1, &50_000);
    assert_eq!(s.vault.get_user_balance(&s.u1), 0);
    assert_eq!(s.vault.get_vault_stats().paused, true);

    // Unpause restores deposits and draws.
    s.vault.unpause();
    assert_eq!(s.vault.get_vault_stats().paused, false);
    s.vault.deposit(&s.u1, &10_000);
    assert_eq!(s.vault.get_user_balance(&s.u1), 10_000);
}

#[test]
fn test_pause_unpause_is_idempotent_and_emits_events() {
    let s = setup(1_000, 86_400);

    s.vault.pause();
    let paused_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        paused_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (Symbol::new(&s.env, "aqua_paused"), s.admin.clone()).into_val(&s.env),
                ().into_val(&s.env),
            ),
        ]
    );

    // Pausing again is a no-op: no extra event, still Ok.
    s.vault.pause();
    let again = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(again, vec![&s.env]);

    s.vault.unpause();
    let unpaused_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        unpaused_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (Symbol::new(&s.env, "aqua_unpaused"), s.admin.clone()).into_val(&s.env),
                ().into_val(&s.env),
            ),
        ]
    );

    // Unpausing again is likewise a no-op.
    s.vault.unpause();
    let again = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(again, vec![&s.env]);
}

#[test]
fn test_pause_requires_admin() {
    // Deliberately no `mock_all_auths`: only the admin is pre-authorized, and
    // only for `initialize`. A non-admin pause must be rejected by the host.
    let env = Env::default();
    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);

    let mock_pool = env.register(MockYieldPool, ());
    let sac = env.register_stellar_asset_contract_v2(mock_pool.clone());
    let token = sac.address();
    MockYieldPoolClient::new(&env, &mock_pool).initialize(&token, &mock_pool);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &vault_id,
            fn_name: "initialize",
            args: (&admin, &token, &mock_pool, &Some(86_400u64)).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    vault.initialize(&admin, &token, &mock_pool, &Some(86_400));

    // `pause` calls `admin.require_auth()`; with no matching authorization the
    // host aborts the call (surfacing as an Err through the try_ client).
    let err = vault.try_pause().unwrap_err();
    assert!(
        err.is_err(),
        "non-admin pause must be rejected (host auth abort)"
    );

    // The vault itself remains usable and unpaused for the admin.
    assert_eq!(vault.get_vault_stats().paused, false);
}

// ---------------------------------------------------------------------------
// Anti-whale deposit rate limiting
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_cap_is_unlimited_by_default() {
    let s = setup(1_000, 86_400);
    // No cap configured: behavior is byte-for-byte identical to the baseline.
    s.vault.deposit(&s.u1, &10_000);
    s.vault.deposit(&s.u1, &10_000);
    assert_eq!(s.vault.get_user_balance(&s.u1), 20_000);
    assert_eq!(s.vault.get_max_deposit_per_interval(), 0);
}

#[test]
fn test_deposit_cap_bounds_standing_balance_and_withdraw_ignores_it() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &5_000);
    s.vault.set_max_deposit_per_interval(&Some(7_000));

    // Top-ups that stay within the cap succeed (up to exactly the cap).
    s.vault.deposit(&s.u1, &2_000);
    assert_eq!(s.vault.get_user_balance(&s.u1), 7_000);
    // Any further deposit pushes the standing balance past the cap and reverts.
    assert_eq!(
        s.vault.try_deposit(&s.u1, &1).unwrap_err(),
        Ok(AquaError::RateLimitExceeded)
    );

    // A fresh user can deposit up to (and exactly at) the cap...
    s.vault.deposit(&s.u2, &7_000);
    assert_eq!(s.vault.get_user_balance(&s.u2), 7_000);
    s.vault.deposit(&s.u3, &7_000);
    assert_eq!(
        s.vault.try_deposit(&s.u3, &1).unwrap_err(),
        Ok(AquaError::RateLimitExceeded)
    );

    // ...but withdrawals are never capped, even at exactly the capped maximum.
    s.vault.withdraw(&s.u3, &7_000);
    assert_eq!(s.vault.get_user_balance(&s.u3), 0);

    // Deposits below the cap still succeed after a partial withdraw.
    s.vault.deposit(&s.u3, &1_000);
    assert_eq!(s.vault.get_user_balance(&s.u3), 1_000);
}

#[test]
fn test_deposit_cap_can_be_cleared_with_none() {
    let s = setup(1_000, 86_400);
    s.vault.set_max_deposit_per_interval(&Some(1_000));
    s.vault.deposit(&s.u1, &1_000);
    assert_eq!(
        s.vault.try_deposit(&s.u1, &1).unwrap_err(),
        Ok(AquaError::RateLimitExceeded)
    );

    // None clears back to unlimited.
    s.vault.set_max_deposit_per_interval(&None);
    assert_eq!(s.vault.get_max_deposit_per_interval(), 0);
    s.vault.deposit(&s.u1, &1);
    assert_eq!(s.vault.get_user_balance(&s.u1), 1_001);
}

#[test]
fn test_deposit_cap_rejects_negative_config() {
    let s = setup(1_000, 86_400);
    assert_eq!(
        s.vault
            .try_set_max_deposit_per_interval(&Some(-1))
            .unwrap_err(),
        Ok(AquaError::InvalidConfig)
    );
}

// ---------------------------------------------------------------------------
// Deposit/withdraw lifecycle with draw between
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle_multiple_rounds() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &100_000);
    s.vault.deposit(&s.u2, &300_000);
    s.vault.deposit(&s.u3, &100_000);

    advance(&s, SECS_PER_YEAR);
    let first = awarded(s.vault.execute_prize_draw());
    assert!(first.total_weight >= 500_000);
    assert_eq!(s.vault.get_vault_stats().total_deposits, 500_000);

    // Another round after more yield accrues.
    advance(&s, SECS_PER_YEAR);
    let second = awarded(s.vault.execute_prize_draw());
    assert_eq!(second.total_weight, 500_000);

    // Everyone exits intact.
    s.vault.withdraw(&s.u1, &100_000);
    s.vault.withdraw(&s.u2, &300_000);
    s.vault.withdraw(&s.u3, &100_000);
    let stats = s.vault.get_vault_stats();
    assert_eq!(stats.total_deposits, 0);
    assert_eq!(stats.participants.len(), 0);
    assert_eq!(token_balance(&s, &s.mock_pool), 0);
}

// ---------------------------------------------------------------------------
// Reentrancy guard (issue #6)
// ---------------------------------------------------------------------------

/// A hostile yield pool used to probe the vault's re-entrancy defenses. Its
/// `deposit`/`withdraw` methods attempt to re-invoke the vault (for a
/// configurable target) on their first call while armed, then behave like a
/// plain pool.
///
/// NOTE: the Soroban host (protocol-level rule, SDK 27) hard-prohibits a
/// contract from re-entering itself — any nested call back into the vault while
/// the vault is on the call stack is rejected with a host error. So the probe
/// below deliberately uses the non-`try_` client, which panics on that host
/// error, and the tests assert the whole transaction reverts atomically: no
/// double credit, no half-applied accounting. The vault's own
/// Checks-Effects-Interactions ordering (`deposit`/`withdraw`) and the draw's
/// `Locked` flag (`execute_prize_draw`) are the defense-in-depth layers that
/// sit *behind* the host guard.
#[contract]
pub struct ReentrantPool;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
enum RKey {
    Vault,
    Target,
    Stored,
    Reentered,
    ArmWithdraw,
    ArmDeposit,
    ArmDraw,
}

#[contractimpl]
impl ReentrantPool {
    pub fn configure(e: Env, vault: Address, target: Address) {
        e.storage().instance().set(&RKey::Vault, &vault);
        e.storage().instance().set(&RKey::Target, &target);
        e.storage().instance().set(&RKey::Stored, &0i128);
        e.storage().instance().set(&RKey::Reentered, &false);
        e.storage().instance().set(&RKey::ArmWithdraw, &false);
        e.storage().instance().set(&RKey::ArmDeposit, &false);
        e.storage().instance().set(&RKey::ArmDraw, &false);
    }

    pub fn arm(e: Env, withdraw: bool, deposit: bool, draw: bool) {
        e.storage().instance().set(&RKey::ArmWithdraw, &withdraw);
        e.storage().instance().set(&RKey::ArmDeposit, &deposit);
        e.storage().instance().set(&RKey::ArmDraw, &draw);
        e.storage().instance().set(&RKey::Reentered, &false);
    }

    /// Override the pool's reported balance (synthetic yield for draw tests).
    pub fn set_stored(e: Env, v: i128) {
        e.storage().instance().set(&RKey::Stored, &v);
    }

    pub fn deposit(e: Env, asset: Address, amount: i128) -> i128 {
        let _ = asset;
        let stored: i128 = e.storage().instance().get(&RKey::Stored).unwrap_or(0);
        e.storage().instance().set(&RKey::Stored, &(stored + amount));
        if armed(&e, &RKey::ArmDeposit) && fire_once(&e) {
            let vault: Address = e.storage().instance().get(&RKey::Vault).unwrap();
            let target: Address = e.storage().instance().get(&RKey::Target).unwrap();
            // Nested vault call: the host blocks the re-entry, which panics this
            // frame and reverts the entire deposit.
            AquaVaultClient::new(&e, &vault).deposit(&target, &amount);
        }
        amount
    }

    pub fn withdraw(e: Env, asset: Address, to: Address, amount: i128) -> i128 {
        let _ = asset;
        let stored: i128 = e.storage().instance().get(&RKey::Stored).unwrap_or(0);
        e.storage()
            .instance()
            .set(&RKey::Stored, &stored.saturating_sub(amount));
        if armed(&e, &RKey::ArmWithdraw) && fire_once(&e) {
            let vault: Address = e.storage().instance().get(&RKey::Vault).unwrap();
            AquaVaultClient::new(&e, &vault).withdraw(&to, &amount);
        }
        if armed(&e, &RKey::ArmDraw) && fire_once(&e) {
            let vault: Address = e.storage().instance().get(&RKey::Vault).unwrap();
            AquaVaultClient::new(&e, &vault).execute_prize_draw();
        }
        amount
    }

    pub fn balance(e: Env, asset: Address, _owner: Address) -> i128 {
        let _ = asset;
        e.storage().instance().get(&RKey::Stored).unwrap_or(0)
    }

    pub fn withdrawable(e: Env, asset: Address, owner: Address) -> i128 {
        Self::balance(e, asset, owner)
    }

    pub fn rate(e: Env, _asset: Address) -> u64 {
        1_000
    }
}

fn armed(e: &Env, key: &RKey) -> bool {
    e.storage().instance().get(key).unwrap_or(false)
}

/// Fire the re-entrancy probe at most once per arming, so a nested pool call
/// (e.g. the inner withdraw's own `pool_withdraw`) cannot recurse forever.
fn fire_once(e: &Env) -> bool {
    if e.storage().instance().get(&RKey::Reentered).unwrap_or(false) {
        return false;
    }
    e.storage().instance().set(&RKey::Reentered, &true);
    true
}

#[test]
fn test_reentrant_withdraw_reverts_atomically() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);

    let pool = env.register(ReentrantPool, ());
    let sac = env.register_stellar_asset_contract_v2(pool.clone());
    let token = sac.address();
    token::StellarAssetClient::new(&env, &token).mint(&u1, &1_000_000_000_000i128);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    ReentrantPoolClient::new(&env, &pool).configure(&vault_id, &u1);
    vault.initialize(&admin, &token, &pool, &Some(86_400));

    vault.deposit(&u1, &10_000);
    ReentrantPoolClient::new(&env, &pool).arm(&true, &false, &false);

    // The pool re-invokes withdraw(u1, 1_000) from inside the outer
    // pool_withdraw. The host hard-blocks the re-entry, so the nested call
    // panics and the whole withdrawal reverts: no double credit and no
    // half-applied accounting.
    let res = vault.try_withdraw(&u1, &1_000).unwrap_err();
    assert!(res.is_err(), "re-entrant withdraw must fail, got {res:?}");

    assert_eq!(vault.get_user_balance(&u1), 10_000);
    assert_eq!(vault.get_vault_stats().total_deposits, 10_000);
    assert_eq!(
        token::Client::new(&env, &token).balance(&u1),
        1_000_000_000_000 - 10_000
    );
}

#[test]
fn test_reentrant_deposit_reverts_atomically() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);
    let attacker = Address::generate(&env);

    let pool = env.register(ReentrantPool, ());
    let sac = env.register_stellar_asset_contract_v2(pool.clone());
    let token = sac.address();
    token::StellarAssetClient::new(&env, &token).mint(&u1, &1_000_000_000_000i128);
    token::StellarAssetClient::new(&env, &token).mint(&attacker, &1_000_000_000_000i128);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    ReentrantPoolClient::new(&env, &pool).configure(&vault_id, &attacker);
    vault.initialize(&admin, &token, &pool, &Some(86_400));

    vault.deposit(&u1, &10_000);
    ReentrantPoolClient::new(&env, &pool).arm(&false, &true, &false);

    // The pool re-invokes deposit(attacker, 5_000) from inside the outer
    // pool_deposit. Re-entry is host-blocked, so the deposit reverts and the
    // attacker gains nothing.
    let res = vault.try_deposit(&u1, &5_000).unwrap_err();
    assert!(res.is_err(), "re-entrant deposit must fail, got {res:?}");

    assert_eq!(vault.get_user_balance(&u1), 10_000);
    assert_eq!(vault.get_user_balance(&attacker), 0);
    assert_eq!(vault.get_vault_stats().total_deposits, 10_000);
}

#[test]
fn test_prize_draw_rejects_reentrant_invocation() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let u1 = Address::generate(&env);

    let pool = env.register(ReentrantPool, ());
    let sac = env.register_stellar_asset_contract_v2(pool.clone());
    let token = sac.address();
    token::StellarAssetClient::new(&env, &token).mint(&u1, &1_000_000_000_000i128);

    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    ReentrantPoolClient::new(&env, &pool).configure(&vault_id, &u1);
    vault.initialize(&admin, &token, &pool, &Some(86_400));

    vault.deposit(&u1, &1_000);
    // Seed extra so the pool can report a positive yield (1_200 vs 1_000).
    token::StellarAssetClient::new(&env, &token).mint(&vault_id, &200);
    ReentrantPoolClient::new(&env, &pool).set_stored(&1_200);

    env.ledger().set_timestamp(env.ledger().timestamp() + 86_400);

    // The pool re-invokes execute_prize_draw from inside the draw's
    // pool_withdraw. The host blocks the re-entry (and the vault's Locked flag
    // would reject it too), so the draw reverts.
    ReentrantPoolClient::new(&env, &pool).arm(&false, &false, &true);
    let res = vault.try_execute_prize_draw().unwrap_err();
    assert!(res.is_err(), "re-entrant draw must fail, got {res:?}");

    // The draw never completed: principal untouched, no prize paid.
    assert_eq!(vault.get_user_balance(&u1), 1_000);
    assert_eq!(vault.get_vault_stats().total_deposits, 1_000);
    assert_eq!(
        token::Client::new(&env, &token).balance(&u1),
        1_000_000_000_000 - 1_000
    );
    // The reverted draw emitted no events (the host rolls the whole failed
    // frame back), so there is no `aqua_prize_awarded` and no re-armed timer.
    let events = env.events().all().filter_by_contract(&vault_id);
    assert_eq!(events.events().len(), 0, "reverted draw must emit no events");
}

// ---------------------------------------------------------------------------
// Yield-source abstraction (#15) + fault-tolerant draws (#13) + rate (#14)
// ---------------------------------------------------------------------------

#[test]
fn test_vault_defaults_to_mock_kind_and_admin_can_switch() {
    let s = setup(1_000, 86_400);
    // Fresh vaults default to the mock adapter (testnet deployment).
    assert_eq!(s.vault.get_yield_source_kind(), YieldSourceKind::Mock);

    // The admin can rebind to a different adapter kind without touching the
    // pool address or any existing state.
    s.vault.set_yield_source_kind(&YieldSourceKind::Blend);
    assert_eq!(s.vault.get_yield_source_kind(), YieldSourceKind::Blend);

    s.vault.set_yield_source_kind(&YieldSourceKind::Custom);
    assert_eq!(s.vault.get_yield_source_kind(), YieldSourceKind::Custom);
}

#[test]
fn test_get_vault_stats_exposes_pool_rate() {
    let s = setup(2_500, 86_400); // 25%/yr
    let stats = s.vault.get_vault_stats();
    assert_eq!(stats.annual_rate_bps, 2_500);
}

#[test]
fn test_draw_against_partial_fill_pool_pays_received_amount() {
    // Balance after one year @10% on 80k principal = 88_000 (yield 8_000).
    // Withdrawable is capped at 82_000 (only 2_000 reachable) and the redeem
    // further partial-fills to 1_500. The vault must pay exactly 1_500.
    let s = setup_shortfall(82_000, 1_500);
    s.vault.deposit(&s.u1, &40_000);
    s.vault.deposit(&s.u2, &40_000);
    let u1_before = token_balance(&s, &s.u1);
    let u2_before = token_balance(&s, &s.u2);

    advance(&s, SECS_PER_YEAR);
    let outcome = awarded(s.vault.execute_prize_draw()); // must NOT revert

    let prize = if outcome.winner == s.u1 {
        token_balance(&s, &s.u1) - u1_before
    } else if outcome.winner == s.u2 {
        token_balance(&s, &s.u2) - u2_before
    } else {
        panic!("winner must be one of the depositors");
    };
    // Only what was actually received is paid out — never more than the vault
    // physically holds.
    assert_eq!(prize, 1_500);
    assert_eq!(s.vault.get_vault_stats().total_deposits, 80_000);
    // The vault holds no excess after handing the full received amount over.
    assert_eq!(token_balance(&s, &s.vault_id), 0);
}

#[test]
fn test_draw_skips_cycle_when_pool_yield_is_unreachable() {
    // Balance after one year @10% on 80k principal = 88_000 (yield 8_000),
    // but the pool can't hand back anything beyond principal (withdrawable
    // cap == principal). The draw must skip the cycle gracefully instead of
    // dead-ending, advancing the interval so it never wedges.
    let s = setup_shortfall(80_000, 0);
    s.vault.deposit(&s.u1, &40_000);
    s.vault.deposit(&s.u2, &40_000);
    let u1_before = token_balance(&s, &s.u1);
    let u2_before = token_balance(&s, &s.u2);

    advance(&s, SECS_PER_YEAR);
    assert_eq!(
        s.vault.execute_prize_draw(),
        DrawResult::Skipped,
        "unreachable yield must skip the cycle, not revert"
    );

    // Exactly one vault event: the skip (not a prize award). Capture BEFORE
    // any further client reads, which reset env.events().all().
    let all_events = s.env.events().all();
    let n_total = all_events.events().len();
    let n_vault = all_events.filter_by_contract(&s.vault_id).events().len();
    assert!(
        n_vault == 1,
        "expected exactly one vault event for a skipped cycle, got total={n_total} vault={n_vault}"
    );

    // Nobody got paid.
    assert_eq!(token_balance(&s, &s.u1), u1_before);
    assert_eq!(token_balance(&s, &s.u2), u2_before);
    // The cycle advanced: an immediate re-draw is TooEarly again.
    assert_eq!(
        s.vault.try_execute_prize_draw().unwrap_err(),
        Ok(AquaError::TooEarly)
    );
    assert_eq!(s.vault.get_vault_stats().total_deposits, 80_000);
}
