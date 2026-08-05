#![no_std]

//! A deployable stand-in for a Blend lending pool, used on testnet so the Aqua
//! vault has something that produces real, observable yield without depending on
//! a live Blend deployment.
//!
//! It implements exactly the interface `aqua_vault` calls:
//!   * `deposit(asset, amount)`   — the vault has already pushed the tokens here,
//!                                  this just settles interest up to now.
//!   * `withdraw(asset, to, amt)` — sends principal/yield back out.
//!   * `balance(asset, owner)`    — principal + accrued interest (its token bal).
//!
//! Yield is simulated by minting simple interest on the pool's live token
//! balance at a configurable annual rate. Because the pool mints, it must be the
//! **admin of the token** (Stellar Asset Contract). The deploy script wires this
//! up by issuing a fresh test USDC SAC whose admin is this pool.
//!
//! To swap in the real Blend protocol, deploy Blend instead and point the vault's
//! `yield_pool` at it — the vault code is unchanged.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env,
};

const SECS_PER_YEAR: u64 = 31_536_000;

/// Failure channel for the mock pool.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PoolError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// A call was made before `initialize`.
    NotInitialized = 2,
    /// A call referenced an asset other than the configured token.
    WrongAsset = 3,
}

/// Storage keys. All are instance-level singletons (single copy, hot reads).
#[contracttype]
pub enum Key {
    /// The accepted asset (whose admin this pool must be).
    Token,
    /// The address authorized to change the rate.
    Admin,
    /// Ledger timestamp of the last accrual.
    LastTs,
    /// Gross annual interest rate in basis points.
    RateBps,
}

#[contract]
pub struct MockPool;

#[contractimpl]
impl MockPool {
    /// Configure the pool. `token` is the asset it accepts (and whose admin it
    /// must be, so it can mint interest). `admin` may change the rate.
    pub fn initialize(e: Env, token: Address, admin: Address) -> Result<(), PoolError> {
        if e.storage().instance().has(&Key::Token) {
            return Err(PoolError::AlreadyInitialized);
        }
        e.storage().instance().set(&Key::Token, &token);
        e.storage().instance().set(&Key::Admin, &admin);
        e.storage().instance().set(&Key::LastTs, &e.ledger().timestamp());
        // Default: 10% annual so demos show yield quickly.
        e.storage().instance().set(&Key::RateBps, &1_000u64);
        Ok(())
    }

    /// Set the gross annual interest rate in basis points (10_000 = 100%/yr).
    pub fn set_rate(e: Env, bps: u64) -> Result<(), PoolError> {
        let admin: Address = e
            .storage()
            .instance()
            .get(&Key::Admin)
            .ok_or(PoolError::NotInitialized)?;
        admin.require_auth();
        e.storage().instance().set(&Key::RateBps, &bps);
        Ok(())
    }

    /// The vault has already transferred `amount` of `asset` into this pool;
    /// settle interest so the new principal earns from now on.
    pub fn deposit(e: Env, asset: Address, amount: i128) -> Result<i128, PoolError> {
        require_asset(&e, &asset)?;
        accrue(&e);
        Ok(amount)
    }

    /// Redeem `amount` of `asset` from the pool to `to`. Solvency is enforced by
    /// the token transfer itself.
    pub fn withdraw(e: Env, asset: Address, to: Address, amount: i128) -> Result<i128, PoolError> {
        let token = require_asset(&e, &asset)?;
        accrue(&e);
        token::Client::new(&e, &token).transfer(&e.current_contract_address(), &to, &amount);
        Ok(amount)
    }

    /// Current withdrawable value: the pool's live token balance after accruing.
    pub fn balance(e: Env, asset: Address, _owner: Address) -> Result<i128, PoolError> {
        let token = require_asset(&e, &asset)?;
        accrue(&e);
        Ok(token::Client::new(&e, &token).balance(&e.current_contract_address()))
    }

    /// The current annual rate in basis points (read-only helper).
    pub fn rate(e: Env) -> Result<u64, PoolError> {
        e.storage()
            .instance()
            .get(&Key::RateBps)
            .ok_or(PoolError::NotInitialized)
    }
}

fn require_asset(e: &Env, asset: &Address) -> Result<Address, PoolError> {
    let token: Address = e
        .storage()
        .instance()
        .get(&Key::Token)
        .ok_or(PoolError::NotInitialized)?;
    if *asset != token {
        return Err(PoolError::WrongAsset);
    }
    Ok(token)
}

/// Simple-interest accrual on the pool's live token balance, minted to itself.
fn accrue(e: &Env) {
    let token: Address = match e.storage().instance().get(&Key::Token) {
        Some(t) => t,
        None => return,
    };
    let rate: u64 = e.storage().instance().get(&Key::RateBps).unwrap_or(0);
    let last: u64 = e.storage().instance().get(&Key::LastTs).unwrap_or(0);
    let now = e.ledger().timestamp();
    let bal = token::Client::new(e, &token).balance(&e.current_contract_address());
    if bal > 0 && now > last && rate > 0 {
        let elapsed = now - last;
        let interest =
            bal * (rate as i128) * (elapsed as i128) / (10_000i128 * (SECS_PER_YEAR as i128));
        if interest > 0 {
            token::StellarAssetClient::new(e, &token)
                .mint(&e.current_contract_address(), &interest);
        }
    }
    e.storage().instance().set(&Key::LastTs, &now);
}

#[cfg(test)]
mod test;
