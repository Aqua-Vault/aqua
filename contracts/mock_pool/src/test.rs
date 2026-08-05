#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Ledger as _}, token, Address, Env};

use crate::{MockPool, MockPoolClient, PoolError};

const YEAR: u64 = 31_536_000;

struct T {
    env: Env,
    pool: MockPoolClient<'static>,
    pool_id: Address,
    token: Address,
    depositor: Address,
}

fn setup() -> T {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);

    let pool_id = env.register(MockPool, ());
    // The pool must be the token admin so it can mint interest.
    let sac = env.register_stellar_asset_contract_v2(pool_id.clone());
    let token = sac.address();
    token::StellarAssetClient::new(&env, &token).mint(&depositor, &1_000_000i128);

    let pool = MockPoolClient::new(&env, &pool_id);
    pool.initialize(&token, &admin);

    T { env, pool, pool_id, token, depositor }
}

fn advance(t: &T, secs: u64) {
    let base = t.env.ledger().timestamp();
    t.env.ledger().set_timestamp(base + secs);
}

#[test]
fn deposit_then_balance_accrues_interest() {
    let t = setup();
    // Simulate the vault pushing principal in, then recording it.
    token::Client::new(&t.env, &t.token).transfer(&t.depositor, &t.pool_id, &100_000);
    t.pool.deposit(&t.token, &100_000);

    advance(&t, YEAR); // 10% default rate
    let bal = t.pool.balance(&t.token, &t.pool_id);
    assert_eq!(bal, 110_000, "100k @ 10%/yr for one year => 110k");
}

#[test]
fn withdraw_sends_tokens_out() {
    let t = setup();
    token::Client::new(&t.env, &t.token).transfer(&t.depositor, &t.pool_id, &100_000);
    t.pool.deposit(&t.token, &100_000);

    advance(&t, YEAR);
    // Yield = 10_000; pull it back to the depositor.
    t.pool.withdraw(&t.token, &t.depositor, &10_000);
    // Remaining pool value is the principal.
    assert_eq!(t.pool.balance(&t.token, &t.pool_id), 100_000);
}

#[test]
fn set_rate_changes_yield() {
    let t = setup();
    t.pool.set_rate(&2_000); // 20%/yr
    token::Client::new(&t.env, &t.token).transfer(&t.depositor, &t.pool_id, &100_000);
    t.pool.deposit(&t.token, &100_000);
    advance(&t, YEAR);
    assert_eq!(t.pool.balance(&t.token, &t.pool_id), 120_000);
}

#[test]
fn get_rate_returns_configured_bps() {
    let t = setup();
    // Default is 10% (1_000 bps) set at initialize.
    assert_eq!(t.pool.get_rate(), 1_000);
    t.pool.set_rate(&2_500);
    assert_eq!(t.pool.get_rate(), 2_500);
}

#[test]
fn withdrawable_equals_live_balance() {
    let t = setup();
    token::Client::new(&t.env, &t.token).transfer(&t.depositor, &t.pool_id, &100_000);
    t.pool.deposit(&t.token, &100_000);
    advance(&t, YEAR); // 10% default rate
    assert_eq!(t.pool.withdrawable(&t.token), 110_000);
    assert_eq!(t.pool.balance(&t.token, &t.pool_id), t.pool.withdrawable(&t.token));
}

#[test]
fn rejects_wrong_asset() {
    let t = setup();
    let other = Address::generate(&t.env);
    assert_eq!(t.pool.try_balance(&other, &t.pool_id).unwrap_err(), Ok(PoolError::WrongAsset));
}

#[test]
fn rejects_double_init() {
    let t = setup();
    let admin = Address::generate(&t.env);
    assert_eq!(
        t.pool.try_initialize(&t.token, &admin).unwrap_err(),
        Ok(PoolError::AlreadyInitialized)
    );
}
