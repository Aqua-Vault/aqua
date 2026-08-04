//! Blend adapter: a thin, swappable integration layer between the Aqua vault
//! and its yield-generating pool.
//!
//! On testnet this targets a [`MockYieldPool`] signalled through
//! [`MockYieldPoolClient`], which accrues a deterministic interest rate so that
//! draws and zero-loss invariants are demonstrable. To wire up the real Blend
//! lending protocol, keep the same methods and point the session at a deployed
//! Blend pool — the vault logic never changes.
//!
//! The shared interface models the subset of the Blend pool/backend surface
//! Aqua depends on:
//!   * `deposit`  -> Blend `Pool::submit`
//!   * `withdraw` -> Blend `Pool::redeem`
//!   * `balance`  -> Blend `Pool::get_withdrawable` + borrow value

use soroban_sdk::{contractclient, token, Address, Env};

/// The token-interface-compatible interface used to talk to a yield pool.
///
/// Three "roles" are collapsed into one shared interface on purpose so that a
/// real Blend pool and the testnet mock have byte-for-byte identical call sites.
#[contractclient(crate_path = "soroban_sdk", name = "YieldPoolClient")]
#[allow(dead_code)]
pub trait YieldPool {
    /// Record `amount` of `asset` as freshly supplied principal. The vault
    /// pushes the tokens to the pool *before* calling this (see
    /// [`clients::pool_deposit`]), so the pool never has to pull from the
    /// vault — that keeps sub-invocation authorization clean on-chain.
    /// Returns the number of shares credited.
    fn deposit(env: Env, asset: Address, amount: i128) -> i128;

    /// Redeem principal from the pool and deliver it *to* `to`.
    fn withdraw(env: Env, asset: Address, to: Address, amount: i128) -> i128;

    /// Total withdrawable value (plus accrued interest) for `owner` in the
    /// pool, denominated in `asset`.
    fn balance(env: Env, asset: Address, owner: Address) -> i128;
}

/// The testnet mock pool's own contract-facing interface. Used by the mock
/// contract deployed in unit tests and the deploy scripts.
#[contractclient(crate_path = "soroban_sdk", name = "MockYieldPoolClient")]
#[allow(dead_code)]
pub trait MockYieldPool {
    /// Configure the pool after deployment: the accepted `token` (whose admin
    /// must be this pool so it can mint interest) and `admin`.
    fn initialize(env: Env, token: Address, admin: Address);

    /// Set the gross annual interest rate in basis points (10_000 = 100%/yr).
    fn set_rate(env: Env, bps: u64);

    fn deposit(env: Env, asset: Address, amount: i128) -> i128;
    fn withdraw(env: Env, asset: Address, to: Address, amount: i128) -> i128;
    fn balance(env: Env, asset: Address, owner: Address) -> i128;
}

/// Convenience wrappers so `lib.rs` never has to import two client types.
pub mod clients {
    use super::*;

    pub fn pool_balance(env: &Env, pool: &Address, asset: &Address, who: &Address) -> i128 {
        YieldPoolClient::new(env, pool).balance(asset, who)
    }

    /// Supply `amount` of principal into the pool on behalf of `vault`. The
    /// vault pushes the tokens itself (direct token caller → invoker auth is
    /// automatic) and the pool merely records them, so no cross-contract auth
    /// escalation is required.
    pub fn pool_deposit(env: &Env, pool: &Address, asset: &Address, vault: &Address, amount: i128) -> i128 {
        token::Client::new(env, asset).transfer(vault, pool, &amount);
        YieldPoolClient::new(env, pool).deposit(asset, &amount)
    }

    pub fn pool_withdraw(env: &Env, pool: &Address, asset: &Address, to: &Address, amount: i128) -> i128 {
        YieldPoolClient::new(env, pool).withdraw(asset, to, &amount)
    }

    /// Sanity check helper: read a token balance.
    #[allow(dead_code)]
    pub fn token_balance(env: &Env, asset: &Address, who: &Address) -> i128 {
        token::Client::new(env, asset).balance(who)
    }
}