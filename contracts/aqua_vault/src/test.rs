#![cfg(test)]

//! Unit + integration tests for the Aqua vault. A mock yield pool contract
//! implements the same `YieldPool` interface the real Blend integration uses,
//! accruing a deterministic annual interest rate so that "no-loss" and
//! proportional-draw invariants can be asserted exactly.

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Events as _, Ledger as _},
    token, vec, Address, Bytes, Env, IntoVal, Symbol, Vec,
};

use crate::storage;
use crate::{select_weighted_winner, AquaError, AquaVault, AquaVaultClient, DrawOutcome};

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
        e.storage().instance().set(&MockKey::LastTs, &e.ledger().timestamp());
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

fn advance(s: &Setup, secs: u64) {
    let base = s.env.ledger().timestamp();
    s.env.ledger().set_timestamp(base + secs);
}

fn token_balance(s: &Setup, who: &Address) -> i128 {
    token::Client::new(&s.env, &s.token).balance(who)
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
    let outcome = s.vault.execute_prize_draw();

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
    let outcome = s.vault.execute_prize_draw();

    // The prize draw emits a single `aqua_prize_awarded` event that matches the
    // returned outcome exactly (winner, prize amount, and PRNG roll).
    let prize_events = s.env.events().all().filter_by_contract(&s.vault_id);
    assert_eq!(
        prize_events,
        vec![
            &s.env,
            (
                s.vault_id.clone(),
                (Symbol::new(&s.env, "aqua_prize_awarded"), outcome.winner.clone())
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
fn test_draw_errors_when_no_positive_yield() {
    let s = setup(1_000, 86_400);
    // Tiny principal so a single day of 10%/yr interest truncates to zero.
    s.vault.deposit(&s.u1, &100);
    advance(&s, 86_400);
    // interest accrual rounds to zero at this scale + short elapsed => no yield
    assert_eq!(
        s.vault.try_execute_prize_draw().unwrap_err(),
        Ok(AquaError::NoYieldToAward)
    );
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
        // 10k selections in one host frame; lift the metering budget.
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
// Deposit/withdraw lifecycle with draw between
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle_multiple_rounds() {
    let s = setup(1_000, 86_400);
    s.vault.deposit(&s.u1, &100_000);
    s.vault.deposit(&s.u2, &300_000);
    s.vault.deposit(&s.u3, &100_000);

    advance(&s, SECS_PER_YEAR);
    let first = s.vault.execute_prize_draw();
    assert!(first.total_weight >= 500_000);
    assert_eq!(s.vault.get_vault_stats().total_deposits, 500_000);

    // Another round after more yield accrues.
    advance(&s, SECS_PER_YEAR);
    let second = s.vault.execute_prize_draw();
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
