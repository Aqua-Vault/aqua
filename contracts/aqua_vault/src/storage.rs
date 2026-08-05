//! # Storage layout
//!
//! All vault state is addressed through the [`DataKey`] enum. Soroban splits
//! storage into two tiers, and Aqua uses them as follows:
//!
//! * **Instance storage** (single copy, cheap for hot reads) — the singleton
//!   configuration and aggregate accounting: [`DataKey::Admin`],
//!   [`DataKey::Asset`], [`DataKey::YieldPool`], [`DataKey::DrawInterval`],
//!   [`DataKey::LastDrawTime`], [`DataKey::TotalDeposits`].
//! * **Persistent storage** (keyed, survives until deleted) — anything
//!   address- or list-shaped and potentially large: one entry per user
//!   ([`DataKey::UserBalance`]) and the ordered depositor registry
//!   ([`DataKey::Depositors`]).
//!
//! Rationale: instance reads are cheaper and there is exactly one admin/asset/
//! pool, while user balances and the depositor list scale with adoption and
//! belong in persistent storage where they can be expired/released.
//!
//! ## Depositor registry
//!
//! [`register_depositor`] / [`unregister_depositor`] keep `Depositors` exactly
//! in sync with positive principal so the winner-selection routine can sum
//! weights over a list of known-participating addresses instead of scanning
//! arbitrary keyspace.

use soroban_sdk::{contracttype, unwrap::UnwrapOptimized, Address, Env, Vec};

use crate::AquaError;

/// TTL policy for persistent (per-user / registry) entries.
///
/// Instance storage is automatically TTL-extended by the ledger on every
/// invocation that touches the contract, so singleton config fields are exempt
/// from this policy. Persistent entries are NOT auto-extended: they die unless
/// the contract explicitly calls `extend_ttl`. Because Aqua is a savings
/// product (users stay idle for months between deposits), every persistent
/// read/write below refreshes the entry out to ~1 year when it is at or below
/// the network minimum TTL, so a recorded principal can never silently expire.
pub(crate) const PERSISTENT_TTL_EXTEND_THRESHOLD: u32 = 4096;
pub(crate) const PERSISTENT_TTL_EXTEND_TO: u32 = 6_312_000;

/// Storage keys for instance (singleton) fields.
#[contracttype]
pub enum DataKey {
    /// The authorized manager (can force early draws).
    Admin,
    /// The deposit token contract (testnet USDC / Stellar Asset Contract).
    Asset,
    /// The yield pool contract Aqua accrues interest in.
    YieldPool,
    /// Seconds between prize draws.
    DrawInterval,
    /// Ledger timestamp of the most recent draw (or initialization).
    LastDrawTime,
    /// Sum of all user principals; doubles as the draw weight total.
    TotalDeposits,
    /// A single user's principal balance.
    UserBalance(Address),
    /// Ordered list of users with positive principal (draw weight sources).
    Depositors,
    /// Re-entrancy guard: `true` while a multi-step external interaction
    /// (prize draw) is in progress.
    Locked,
    /// Emergency circuit breaker flag (`true` = deposits/draws blocked).
    Paused,
    /// Per-user standing-balance cap in `asset` units (`0` = unlimited).
    MaxDepositPerInterval,
}

pub(crate) fn guard_initialized(e: &Env) -> core::result::Result<(), AquaError> {
    if !has_admin(e) {
        return Err(AquaError::NotInitialized);
    }
    Ok(())
}

// ---- Scalar instance fields -------------------------------------------------

pub(crate) fn has_admin(e: &Env) -> bool {
    e.storage().instance().has(&DataKey::Admin)
}

pub(crate) fn admin(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap_optimized()
}

pub(crate) fn set_admin(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::Admin, a);
}

pub(crate) fn asset(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::Asset)
        .unwrap_optimized()
}

pub(crate) fn set_asset(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::Asset, a);
}

pub(crate) fn yield_pool(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::YieldPool)
        .unwrap_optimized()
}

pub(crate) fn set_yield_pool(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::YieldPool, a);
}

pub(crate) fn draw_interval(e: &Env) -> u64 {
    e.storage()
        .instance()
        .get(&DataKey::DrawInterval)
        .unwrap_optimized()
}

pub(crate) fn set_draw_interval(e: &Env, v: u64) {
    e.storage().instance().set(&DataKey::DrawInterval, &v);
}

pub(crate) fn last_draw_time(e: &Env) -> u64 {
    e.storage()
        .instance()
        .get(&DataKey::LastDrawTime)
        .unwrap_optimized()
}

pub(crate) fn set_last_draw_time(e: &Env, v: u64) {
    e.storage().instance().set(&DataKey::LastDrawTime, &v);
}

pub(crate) fn total_deposits(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::TotalDeposits)
        .unwrap_or(0)
}

pub(crate) fn set_total_deposits(e: &Env, v: i128) {
    e.storage().instance().set(&DataKey::TotalDeposits, &v);
}

// ---- Re-entrancy guard -------------------------------------------------------

/// Whether a multi-step external interaction is currently in progress. While
/// `true`, every mutating vault entry point reverts with
/// [`AquaError::Reentrancy`] instead of reading potentially half-updated state.
pub(crate) fn is_locked(e: &Env) -> bool {
    e.storage()
        .instance()
        .get(&DataKey::Locked)
        .unwrap_or(false)
}

pub(crate) fn set_locked(e: &Env, locked: bool) {
    e.storage().instance().set(&DataKey::Locked, &locked);
}

// ---- Emergency circuit breaker -------------------------------------------------

/// Whether new deposits and draws are currently blocked.
pub(crate) fn is_paused(e: &Env) -> bool {
    e.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub(crate) fn set_paused(e: &Env, paused: bool) {
    e.storage().instance().set(&DataKey::Paused, &paused);
}

// ---- Per-user principal -----------------------------------------------------

pub(crate) fn user_balance(e: &Env, who: &Address) -> i128 {
    let key = DataKey::UserBalance(who.clone());
    let val: Option<i128> = e.storage().persistent().get(&key);
    if val.is_some() {
        e.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_EXTEND_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }
    val.unwrap_or(0)
}

pub(crate) fn set_user_balance(e: &Env, who: &Address, v: i128) {
    let key = DataKey::UserBalance(who.clone());
    e.storage().persistent().set(&key, &v);
    e.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_TTL_EXTEND_THRESHOLD,
        PERSISTENT_TTL_EXTEND_TO,
    );
}

// ---- Depositors membership (weight source for draws) ------------------------

pub(crate) fn depositors(e: &Env) -> Vec<Address> {
    let key = DataKey::Depositors;
    let val: Option<Vec<Address>> = e.storage().persistent().get(&key);
    if val.is_some() {
        e.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_EXTEND_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }
    val.unwrap_or_else(|| Vec::new(e))
}

pub(crate) fn set_depositors(e: &Env, list: &Vec<Address>) {
    let key = DataKey::Depositors;
    e.storage().persistent().set(&key, list);
    e.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_TTL_EXTEND_THRESHOLD,
        PERSISTENT_TTL_EXTEND_TO,
    );
}

/// Append `who` if they are not already tracked and hold a positive balance.
pub(crate) fn register_depositor(e: &Env, who: &Address) {
    if user_balance(e, who) <= 0 {
        return;
    }
    let mut list = depositors(e);
    if !list.contains(who) {
        list.push_back(who.clone());
        set_depositors(e, &list);
    }
}

/// Drop `who` from the membership list once their balance reaches zero, keeping
/// the draw weight distribution exact.
pub(crate) fn unregister_depositor(e: &Env, who: &Address) {
    if user_balance(e, who) != 0 {
        return;
    }
    let list = depositors(e);
    let mut pruned: Vec<Address> = Vec::new(e);
    for addr in list.iter() {
        if addr != *who {
            pruned.push_back(addr);
        }
    }
    set_depositors(e, &pruned);
}

// ---- Anti-whale rate limiting -------------------------------------------------

/// Standing-balance cap for a single user (`0` = unlimited). Semantics are
/// deliberately simple: the cap bounds `user_balance`, not total deposits over
/// time, and only affects new deposits — withdrawals are never limited.
pub(crate) fn max_deposit_per_interval(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::MaxDepositPerInterval)
        .unwrap_or(0)
}

pub(crate) fn set_max_deposit_per_interval(e: &Env, v: i128) {
    e.storage()
        .instance()
        .set(&DataKey::MaxDepositPerInterval, &v);
}

/// Whether a draw may be run right now: interval elapsed, or caller is admin
/// (forcing an early draw). Admin check requires fresh auth at the call site;
/// here we only read who the admin is and let `execute_prize_draw` decide.
pub(crate) fn can_draw(e: &Env) -> bool {
    let now = e.ledger().timestamp();
    let last = last_draw_time(e);
    let interval = draw_interval(e);
    now.saturating_sub(last) >= interval
}

/// Public-facing stats struct returned by `get_vault_stats`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultStats {
    /// Sum of all user principals locked in the vault.
    pub total_deposits: i128,
    /// Live yield: current pool value minus total principal, floored at zero.
    pub current_yield: i128,
    /// Seconds remaining until the next draw is allowed.
    pub seconds_until_next_draw: u64,
    /// Participating depositors (capped at 100 entries).
    pub participants: Vec<Address>,
    /// `true` when the emergency circuit breaker is engaged.
    pub paused: bool,
}
