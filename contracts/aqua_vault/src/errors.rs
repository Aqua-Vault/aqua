//! # Error codes
//!
//! [`AquaError`] is the single failure channel for the vault. It is exported to
//! callers through the WASM ABI via `#[contracterror]`, so clients see the exact
//! variant (rather than a generic failure) through the `try_` client methods.
//!
//! | Code | Variant | Fires when |
//! | --- | --- | --- |
//! | 1 | `AlreadyInitialized` | `initialize` called more than once |
//! | 2 | `NotInitialized` | any mutating/ledger call before `initialize` |
//! | 3 | `AmountMustBePositive` | deposit/withdraw amount is zero or negative |
//! | 4 | `InsufficientBalance` | withdraw exceeds the caller's principal |
//! | 5 | `TooEarly` | a draw is attempted before the interval elapses |
//! | 6 | `NoDepositors` | no user holds positive principal |
//! | 7 | `NoYieldToAward` | current yield is not positive |
//! | 8 | `InvalidConfig` | `draw_interval` is configured as zero |
//! | 9 | `Unauthorized` | a non-admin calls an admin-only action |

use soroban_sdk::contracterror;

/// Error codes surfaced to callers as `Err(AquaError::…)`.
///
/// `#[contracterror]` + `#[repr(u32)]` generates the conversions Soroban needs
/// to (de)serialize the error across the WASM ABI, so callers see this exact
/// enum (e.g. via the `try_` client variants) instead of a generic failure.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AquaError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// Mutating/Ledger entry called before `initialize`.
    NotInitialized = 2,
    /// A deposit/withdraw amount was zero or negative.
    AmountMustBePositive = 3,
    /// Requested withdraw exceeds the caller's principal.
    InsufficientBalance = 4,
    /// A draw was attempted before `draw_interval` had elapsed.
    TooEarly = 5,
    /// No depositors with positive principal exist.
    NoDepositors = 6,
    /// No positive yield to award right now.
    NoYieldToAward = 7,
    /// `draw_interval` configuration was invalid (zero).
    InvalidConfig = 8,
    /// Only the admin may perform this action.
    Unauthorized = 9,
    /// The contract is already inside a multi-step external interaction and a
    /// re-entrant call into the vault was attempted. The caller must not
    /// re-enter the vault while a draw (or other locked operation) is running.
    Reentrancy = 10,
}