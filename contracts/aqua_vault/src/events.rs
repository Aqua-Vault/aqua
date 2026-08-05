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

/// The reason a prize draw cycle was skipped instead of awarding a prize.
#[allow(dead_code)]
pub(crate) const SKIP_REASON_NO_YIELD: &str = "no_yield";

/// Contract-level event topics. Symbols are built at call time because
/// `Symbol::new` requires an `Env` in SDK 27.
#[allow(deprecated)]
pub(crate) fn initialized(
    e: &Env,
    admin: &Address,
    asset: &Address,
    pool: &Address,
    interval: u64,
) {
    e.events().publish(
        (
            Symbol::new(e, "aqua_initialized"),
            admin.clone(),
            asset.clone(),
            pool.clone(),
        ),
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

/// Emergency circuit breaker engaged by the admin. New deposits and draws are
/// blocked until `aqua_unpaused` is emitted.
#[allow(deprecated)]
pub(crate) fn paused(e: &Env, admin: &Address) {
    e.events()
        .publish((Symbol::new(e, "aqua_paused"), admin.clone()), ());
}

/// Emergency circuit breaker disengaged by the admin.
#[allow(deprecated)]
pub(crate) fn unpaused(e: &Env, admin: &Address) {
    e.events()
        .publish((Symbol::new(e, "aqua_unpaused"), admin.clone()), ());
}

/// A prize-draw cycle completed without awarding a prize (e.g. no positive
/// yield accrued this interval). The timer has been re-armed, so the next draw
/// becomes eligible after a fresh `draw_interval`.
#[allow(deprecated)]
pub(crate) fn draw_skipped(e: &Env, total_deposits: i128, reason: &str) {
    e.events().publish(
        (Symbol::new(e, "aqua_draw_skipped"), Symbol::new(e, reason)),
        total_deposits,
    );
}

/// Emitted when a draw cycle completes without awarding a prize because the
/// yield pool had no reachable yield (borrow shortfall / paused pool). The
/// cycle still advances so the draw is never wedged.
#[allow(deprecated)]
pub(crate) fn prize_skipped(e: &Env, winner: &Address, roll: u64) {
    e.events().publish(
        (Symbol::new(e, "aqua_prize_skipped"), winner.clone()),
        roll,
    );
}
