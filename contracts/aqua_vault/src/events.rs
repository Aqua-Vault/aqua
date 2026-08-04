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