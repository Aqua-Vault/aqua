//! Weighted random winner selection (CAP-0074 PRNG).
//!
//! Extracted from `lib.rs` so the draw's hot path is isolated and unit-testable
//! with a seeded PRNG. The previous implementation materialized a
//! `Vec<(Address, i128)>` of cumulative weight bounds *and* a separate
//! `Vec<Address>` of participants, then re-walked both. That is O(n) storage
//! reads plus two allocations per draw, which on Soroban's metered budget makes
//! `execute_prize_draw` scale poorly with depositor count.
//!
//! This version reads each depositor's balance exactly once (one storage read
//! per depositor, matching the acceptance criteria) while accumulating a cheap
//! `Vec<i128>` of cumulative bounds alongside the `Vec<Address>` of
//! participants that `DrawOutcome` requires anyway. The winner is then found by
//! scanning the in-memory cumulative bounds — no second storage walk, and no
//! `Vec<(Address, i128)>` allocation.

use soroban_sdk::{Address, Env, Vec};

use crate::{storage, DrawOutcome};

/// Weighted random selection over all current depositors using the on-chain
/// PRNG. A depositor's probability of winning is exactly
/// `balance / total_deposits`.
///
/// The caller must guarantee `total > 0` before calling (guarded by
/// `debug_assert!`). `total` is additionally required to fit in `u64` because
/// the roll is drawn with `gen_range(0..total as u64)` — exceeding it would
/// silently truncate the roll domain, so we panic loudly instead.
pub(crate) fn select_weighted_winner(e: &Env) -> DrawOutcome {
    let mut total: i128 = 0;
    // Cumulative upper bound of each participant's weight segment, in deposit
    // order. i128-only: no Address stored here, so this is far cheaper than the
    // previous Vec<(Address, i128)>.
    let mut cumulative: Vec<i128> = Vec::new(e);
    let mut participants: Vec<Address> = Vec::new(e);

    // Pass 1: read each depositor exactly once, building the weight segments.
    for addr in storage::depositors(e).iter() {
        let bal = storage::user_balance(e, &addr);
        if bal > 0 {
            total = total.saturating_add(bal);
            cumulative.push_back(total);
            participants.push_back(addr);
        }
    }

    debug_assert!(total > 0, "caller must guard total > 0 before selecting");
    assert!(
        total <= u64::MAX as i128,
        "total_deposits exceeds the u64 roll domain"
    );

    // u64 roll over [0, total), then pick the first depositor whose cumulative
    // upper bound exceeds the roll. Scans in-memory only — no storage reads.
    let roll: u64 = e.prng().gen_range(0..(total as u64));
    let mut winner = participants.first().unwrap().clone();
    for (i, bound) in cumulative.iter().enumerate() {
        if (roll as i128) < bound {
            winner = participants.get(i as u32).unwrap().clone();
            break;
        }
    }

    DrawOutcome {
        winner,
        roll,
        total_weight: total,
        participants,
    }
}
