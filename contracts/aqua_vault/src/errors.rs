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