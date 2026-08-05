use soroban_sdk::{contracttype, unwrap::UnwrapOptimized, Address, Env, Vec};

use crate::AquaError;

/// Storage keys for instance (singleton) fields.
#[contracttype]
pub enum DataKey {
    Admin,
    Asset,
    YieldPool,
    DrawInterval,
    LastDrawTime,
    TotalDeposits,
    UserBalance(Address),
    Depositors,
    /// Re-entrancy guard: `true` while a multi-step external interaction
    /// (prize draw) is in progress.
    Locked,
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
    e.storage().instance().get(&DataKey::Admin).unwrap_optimized()
}

pub(crate) fn set_admin(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::Admin, a);
}

pub(crate) fn asset(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::Asset).unwrap_optimized()
}

pub(crate) fn set_asset(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::Asset, a);
}

pub(crate) fn yield_pool(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::YieldPool).unwrap_optimized()
}

pub(crate) fn set_yield_pool(e: &Env, a: &Address) {
    e.storage().instance().set(&DataKey::YieldPool, a);
}

pub(crate) fn draw_interval(e: &Env) -> u64 {
    e.storage().instance().get(&DataKey::DrawInterval).unwrap_optimized()
}

pub(crate) fn set_draw_interval(e: &Env, v: u64) {
    e.storage().instance().set(&DataKey::DrawInterval, &v);
}

pub(crate) fn last_draw_time(e: &Env) -> u64 {
    e.storage().instance().get(&DataKey::LastDrawTime).unwrap_optimized()
}

pub(crate) fn set_last_draw_time(e: &Env, v: u64) {
    e.storage().instance().set(&DataKey::LastDrawTime, &v);
}

pub(crate) fn total_deposits(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0)
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

// ---- Per-user principal -----------------------------------------------------

pub(crate) fn user_balance(e: &Env, who: &Address) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::UserBalance(who.clone()))
        .unwrap_or(0)
}

pub(crate) fn set_user_balance(e: &Env, who: &Address, v: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::UserBalance(who.clone()), &v);
}

// ---- Depositors membership (weight source for draws) ------------------------

pub(crate) fn depositors(e: &Env) -> Vec<Address> {
    e.storage()
        .persistent()
        .get(&DataKey::Depositors)
        .unwrap_or_else(|| Vec::new(e))
}

pub(crate) fn set_depositors(e: &Env, list: &Vec<Address>) {
    e.storage().persistent().set(&DataKey::Depositors, list);
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
    pub total_deposits: i128,
    pub current_yield: i128,
    pub seconds_until_next_draw: u64,
    pub participants: Vec<Address>,
}