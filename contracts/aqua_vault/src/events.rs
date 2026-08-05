//! # Events
//!
//! Aqua publishes four contract-level events. Each is a two-field Soroban
//! `publish(topic, data)` where the **topic** is the event name plus indexed
//! filters and the **data** carries the payload:
//!
//! | Topic | Topic payload | Data payload |
//! | --- | --- | --- |
//! | `aqua_initialized` | `(Symbol, admin, asset, pool)` | `interval: u64` |
//! | `aqua_deposit` | `(Symbol, from)` | `(amount, new_balance): (i128, i128)` |
//! | `aqua_withdraw` | `(Symbol, from)` | `(amount, new_balance): (i128, i128)` |
//! | `aqua_prize_awarded` | `(Symbol, winner)` | `(prize_amount, roll): (i128, u64)` |
//!
//! Indexers should subscribe on the `Symbol` first field (the topic name) and
//! read the remaining topic fields as filters (`from` / `winner` / config
//! addresses). The `roll` in `aqua_prize_awarded` lets any observer reproduce
//! the weighted selection on-chain.

use soroban_sdk::{Address, Env, Symbol};

/// Contract-level event topics. Symbols are built at call time because
/// `Symbol::new` requires an `Env` in SDK 27.
#[allow(deprecated)]
pub(crate) fn initialized(e: &Env, admin: &Address, asset: &Address, pool: &Address, interval: u64) {
    e.events().publish(
        (Symbol::new(e, "aqua_initialized"), admin.clone(), asset.clone(), pool.clone()),
        interval,
    );
}

#[allow(deprecated)]
pub(crate) fn deposited(e: &Env, from: &Address, amount: i128, new_balance: i128) {
    e.events().publish(
        (Symbol::new(e, "aqua_deposit"), from.clone()),
        (amount, new_balance),
    );
}

#[allow(deprecated)]
pub(crate) fn withdrawn(e: &Env, from: &Address, amount: i128, new_balance: i128) {
    e.events().publish(
        (Symbol::new(e, "aqua_withdraw"), from.clone()),
        (amount, new_balance),
    );
}

#[allow(deprecated)]
pub(crate) fn prize_awarded(e: &Env, winner: &Address, prize_amount: i128, roll: u64) {
    e.events().publish(
        (Symbol::new(e, "aqua_prize_awarded"), winner.clone()),
        (prize_amount, roll),
    );
}