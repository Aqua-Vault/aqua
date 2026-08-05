//! Yield-source adapters for the Aqua vault.
//!
//! Each adapter implements [`YieldSource`] to drive one concrete pool protocol
//! through the shared [`YieldPoolClient`] (see `yield_trait.rs`). The vault
//! dispatches on its stored [`YieldSourceKind`] and never names a protocol.
//!
//! The shared interface models the subset of the Blend pool/backend surface
//! Aqua depends on:
//!   * `deposit`      -> Blend `Pool::submit`       (tokens already pushed)
//!   * `withdraw`     -> Blend `Pool::redeem`
//!   * `balance`      -> Blend `Pool::get_withdrawable` − borrow value
//!   * `withdrawable` -> Blend `Pool::get_withdrawable`
//!   * `rate`         -> n/a: a Blend pool has per-reserve rates, not a single
//!                       annual rate (returns 0 so the UI degrades gracefully)
//!
//! On testnet the [`MockYieldSource`] drives the deterministic mock pool, which
//! accrues simple interest so draws and zero-loss invariants are demonstrable.

use soroban_sdk::Address;
use soroban_sdk::Env;

use crate::yield_trait::{YieldPoolClient, YieldSource};

/// Blend adapter: drives a deployed Blend pool through `YieldPoolClient`.
pub struct BlendYieldSource;

impl YieldSource for BlendYieldSource {
    fn deposit(&self, env: &Env, pool: &Address, asset: &Address, amount: i128) -> i128 {
        YieldPoolClient::new(env, pool).deposit(asset, &amount)
    }

    fn withdraw(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        to: &Address,
        amount: i128,
    ) -> i128 {
        YieldPoolClient::new(env, pool).withdraw(asset, to, &amount)
    }

    fn balance(&self, env: &Env, pool: &Address, asset: &Address, who: &Address) -> i128 {
        YieldPoolClient::new(env, pool).balance(asset, who)
    }

    fn withdrawable(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        who: &Address,
    ) -> i128 {
        YieldPoolClient::new(env, pool).withdrawable(asset, who)
    }

    fn rate(&self, _env: &Env, _pool: &Address, _asset: &Address) -> u64 {
        // A multi-reserve Blend pool has no single annual rate; surface 0 so
        // the frontend shows "—" instead of a fabricated projection.
        0
    }
}

/// Mock adapter: drives the testnet mock pool through `YieldPoolClient`.
pub struct MockYieldSource;

impl YieldSource for MockYieldSource {
    fn deposit(&self, env: &Env, pool: &Address, asset: &Address, amount: i128) -> i128 {
        YieldPoolClient::new(env, pool).deposit(asset, &amount)
    }

    fn withdraw(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        to: &Address,
        amount: i128,
    ) -> i128 {
        YieldPoolClient::new(env, pool).withdraw(asset, to, &amount)
    }

    fn balance(&self, env: &Env, pool: &Address, asset: &Address, who: &Address) -> i128 {
        YieldPoolClient::new(env, pool).balance(asset, who)
    }

    fn withdrawable(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        who: &Address,
    ) -> i128 {
        YieldPoolClient::new(env, pool).withdrawable(asset, who)
    }

    fn rate(&self, env: &Env, pool: &Address, asset: &Address) -> u64 {
        YieldPoolClient::new(env, pool).rate(asset)
    }
}
