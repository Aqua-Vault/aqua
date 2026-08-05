//! Abstract yield-source abstraction for the Aqua vault.
//!
//! The vault must not care whether it earns yield from a Blend lending pool,
//! the testnet mock pool, or a future staking protocol. Everything the vault
//! does with its yield pool is expressed through the [`YieldSource`] adapter
//! trait, and `lib.rs` dispatches purely on the stored [`YieldSourceKind`].
//!
//! Adding a third yield source:
//!   1. implement [`YieldSource`] for a new adapter struct in
//!      `blend_adapter.rs` (or its own module),
//!   2. register its [`YieldSourceKind`] in [`yield_source::for_kind`],
//!   3. set that kind on the vault via `set_yield_source_kind`.
//! No `lib.rs` business logic changes are required.
//!
//! Mapping to Blend's `Pool` interface (also see `blend_adapter.rs` docs):
//!   * `deposit`      -> `Pool::submit`
//!   * `withdraw`     -> `Pool::redeem`
//!   * `balance`      -> `Pool::get_withdrawable` − borrow value
//!   * `withdrawable` -> `Pool::get_withdrawable`
//!   * `rate`         -> no single annual rate on a multi-reserve Blend pool
//!                      (returns 0; mock pools return their configured rate)

use soroban_sdk::{contractclient, contracttype, token, Address, Env};

use crate::blend_adapter::{BlendYieldSource, MockYieldSource};
use crate::storage;

/// Identifies which yield-source adapter the vault is bound to. Stored at
/// `initialize` (defaults to [`YieldSourceKind::Mock`] for the testnet
/// deployment); the admin may switch it post-deploy via
/// [`crate::AquaVault::set_yield_source_kind`].
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YieldSourceKind {
    Blend,
    Mock,
    Custom,
}

/// The on-chain pool interface the vault calls. Byte-identical call sites
/// across the deployable mock pool, the test mock pool, and (in production) a
/// Blend pool, so the vault code never changes when the target pool does.
#[contractclient(crate_path = "soroban_sdk", name = "YieldPoolClient")]
#[allow(dead_code)]
pub trait YieldPool {
    /// Record `amount` of `asset` as freshly supplied principal. The vault
    /// pushes the tokens before calling this (see [`yield_source::deposit`]),
    /// so the pool never has to pull — that keeps sub-invocation authorization
    /// clean on-chain. Returns the number of shares credited.
    fn deposit(env: Env, asset: Address, amount: i128) -> i128;

    /// Redeem principal+yield from the pool and deliver it *to* `to`. Returns
    /// the amount actually received (may be less than requested).
    fn withdraw(env: Env, asset: Address, to: Address, amount: i128) -> i128;

    /// Total withdrawable value (plus accrued interest) for `owner` in the
    /// pool, denominated in `asset`.
    fn balance(env: Env, asset: Address, owner: Address) -> i128;

    /// Maximum currently withdrawable for `owner` (Blend `get_withdrawable`).
    /// May be *less* than `balance` when borrowers have shortfalls.
    fn withdrawable(env: Env, asset: Address, owner: Address) -> i128;

    /// Gross annual rate in basis points (10_000 = 100%). Mock pools expose
    /// their configured rate; protocols without a single rate return 0.
    fn rate(env: Env, asset: Address) -> u64;
}

/// Adapter contract: concrete structs implement this to drive a specific pool
/// protocol, keeping `lib.rs` agnostic of any particular yield source.
pub trait YieldSource {
    /// Record `amount` of `asset` as principal supplied by the vault (the
    /// tokens were already transferred to the pool by [`yield_source::deposit`]).
    fn deposit(&self, env: &Env, pool: &Address, asset: &Address, amount: i128) -> i128;

    /// Redeem `amount` of value to `to`; returns the amount actually received,
    /// which may be less than `amount` if the pool partial-fills.
    fn withdraw(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        to: &Address,
        amount: i128,
    ) -> i128;

    /// Total value (principal + accrued yield) held for `who`.
    fn balance(&self, env: &Env, pool: &Address, asset: &Address, who: &Address) -> i128;

    /// Maximum currently withdrawable for `who`; may be less than `balance`.
    fn withdrawable(
        &self,
        env: &Env,
        pool: &Address,
        asset: &Address,
        who: &Address,
    ) -> i128;

    /// Gross annual rate in bps (10_000 = 100%). 0 means unknown / not
    /// applicable (e.g. a multi-reserve Blend pool).
    fn rate(&self, env: &Env, pool: &Address, asset: &Address) -> u64;
}

/// Resolve the adapter bound to the vault's stored [`YieldSourceKind`].
pub fn for_kind(e: &Env) -> &'static dyn YieldSource {
    match storage::yield_source_kind(e) {
        YieldSourceKind::Blend => &BlendYieldSource,
        YieldSourceKind::Mock => &MockYieldSource,
        YieldSourceKind::Custom => &MockYieldSource,
    }
}

/// Dispatch helpers: the only pool calls `lib.rs` makes. The deposit wrapper
/// pushes tokens into the pool before recording them, mirroring the original
/// `clients::pool_deposit` behaviour.
pub mod yield_source {
    use super::*;

    pub fn balance(e: &Env, pool: &Address, asset: &Address, who: &Address) -> i128 {
        for_kind(e).balance(e, pool, asset, who)
    }

    pub fn withdrawable(e: &Env, pool: &Address, asset: &Address, who: &Address) -> i128 {
        for_kind(e).withdrawable(e, pool, asset, who)
    }

    pub fn withdraw(
        e: &Env,
        pool: &Address,
        asset: &Address,
        to: &Address,
        amount: i128,
    ) -> i128 {
        for_kind(e).withdraw(e, pool, asset, to, amount)
    }

    /// Push `amount` of `asset` from `vault` into `pool`, then record it via
    /// the adapter so the new principal starts earning immediately.
    pub fn deposit(
        e: &Env,
        pool: &Address,
        asset: &Address,
        vault: &Address,
        amount: i128,
    ) -> i128 {
        token::Client::new(e, asset).transfer(vault, pool, &amount);
        for_kind(e).deposit(e, pool, asset, amount)
    }

    pub fn rate(e: &Env, pool: &Address, asset: &Address) -> u64 {
        for_kind(e).rate(e, pool, asset)
    }
}
