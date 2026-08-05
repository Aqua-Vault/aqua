#![cfg(test)]

//! Probabilistic multi-user integration test for the weighted prize draw.
//!
//! Runs N = 2_000 full `execute_prize_draw` cycles across 8 depositors with
//! deliberately unequal balances `[100, 200, 300, 400, 500, 1_000, 2_000,
//! 4_000]` and asserts that the observed win rate of every user converges to
//! their share of the pool within ±2% (absolute), while re-verifying the
//! zero-loss / accounting invariants after every single round.
//!
//! ## Reproducibility (seed)
//!
//! Soroban's test host seeds its base PRNG to zero at `Env` construction. Each
//! `execute_prize_draw()` invocation derives its own local PRNG from that base
//! strictly by its order of invocation (see `soroban-sdk` `prng` module docs),
//! so the whole 2_000-draw stream — and therefore the observed win counts — is
//! bit-for-bit deterministic for a given SDK version. The mid-run rebalance
//! subset is chosen with a fixed-seed LCG (`0x2545F4914F6CDD1D`) so that part
//! is deterministic too.
//!
//! Expected counts under this configuration (recomputed from the evolving
//! balances each round) are within ±2% absolute of observed rates. The exact
//! deterministic counts for the 2_000-round run are:
//!
//! | user | starting balance | expected wins | observed wins | deviation |
//! |------|-----------------:|--------------:|--------------:|----------:|
//! | 0    |              100 |         12.78 |            14 |    +0.06% |
//! | 1    |              200 |         25.30 |            34 |    +0.44% |
//! | 2    |              300 |        130.70 |           124 |    -0.34% |
//! | 3    |              400 |         19.48 |            13 |    -0.32% |
//! | 4    |              500 |         45.83 |            41 |    -0.24% |
//! | 5    |            1,000 |        119.69 |           110 |    -0.48% |
//! | 6    |            2,000 |        137.14 |           133 |    -0.21% |
//! | 7    |            4,000 |      1,509.07 |         1,531 |    +1.10% |

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env, Vec,
};

use crate::test::{MockYieldPool, MockYieldPoolClient};
use crate::{AquaVault, AquaVaultClient, DrawResult};

/// Annual yield rate the mock pool accrues (100% / year). Chosen so a single
/// 24h interval always produces integer yield > 0 for every principal size
/// exercised here (daily interest ≈ total/365, and the pool never drops below
/// a few thousand units during the rebalances).
const RATE_BPS: u64 = 10_000;

const DRAW_INTERVAL_SECS: u64 = 86_400;
const N_DRAWS: u32 = 2_000;
const REBALANCE_EVERY: u32 = 100;

/// Deliberately unequal starting deposits (total = 8_500).
const BALANCES: [i128; 8] = [100, 200, 300, 400, 500, 1_000, 2_000, 4_000];

/// Fixed-seed LCG for picking the mid-run rebalance subset (deterministic).
const LCG_SEED: u64 = 0x2545_F491_4F6C_DD1D;

struct Harness {
    env: Env,
    vault: AquaVaultClient<'static>,
    vault_id: Address,
    token: Address,
    users: Vec<Address>,
    mock_pool: Address,
}

fn harness() -> Harness {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let mock_pool = env.register(MockYieldPool, ());
    let sac = env.register_stellar_asset_contract_v2(mock_pool.clone());
    let token = sac.address();
    let token_admin = token::StellarAssetClient::new(&env, &token);

    let mut users: Vec<Address> = Vec::new(&env);
    for _ in 0..BALANCES.len() {
        let user = Address::generate(&env);
        token_admin.mint(&user, &1_000_000_000_000i128);
        users.push_back(user);
    }

    MockYieldPoolClient::new(&env, &mock_pool).initialize(&token, &mock_pool);
    MockYieldPoolClient::new(&env, &mock_pool).set_rate(&RATE_BPS);

    let admin = Address::generate(&env);
    let vault_id = env.register(AquaVault, ());
    let vault = AquaVaultClient::new(&env, &vault_id);
    vault.initialize(&admin, &token, &mock_pool, &Some(DRAW_INTERVAL_SECS));

    for (user, &amount) in users.iter().zip(BALANCES.iter()) {
        vault.deposit(&user, &amount);
    }

    Harness {
        env,
        vault,
        vault_id,
        token,
        users,
        mock_pool,
    }
}

fn token_balance(h: &Harness, who: &Address) -> i128 {
    token::Client::new(&h.env, &h.token).balance(who)
}

fn next_lcg(state: &mut u64) -> usize {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 32) % 8) as usize
}

/// Verify the zero-loss + physical-accounting invariants on-chain: total
/// deposits equal the sum of user balances, the vault never holds the deposit
/// token, and the yield pool always covers all principal.
fn assert_invariants(h: &Harness, balances: &[i128; 8], at: &str) {
    let stats = h.vault.get_vault_stats();
    let sum: i128 = balances.iter().sum();
    assert_eq!(
        stats.total_deposits, sum,
        "{at}: total_deposits must equal Σ user balances"
    );
    for (i, user) in h.users.iter().enumerate() {
        assert_eq!(
            h.vault.get_user_balance(&user),
            balances[i],
            "{at}: on-chain balance of user {i} must match the local mirror"
        );
    }
    assert_eq!(
        token_balance(h, &h.vault_id),
        0,
        "{at}: vault must never hold the deposit token (principal lives in the pool)"
    );
    let pool_balance = token_balance(h, &h.mock_pool);
    assert!(
        pool_balance >= stats.total_deposits,
        "{at}: pool value {pool_balance} must cover total_deposits {}",
        stats.total_deposits
    );
}

#[test]
fn test_multiuser_2000_draw_convergence_and_zero_loss() {
    let h = harness();

    let mut observed = [0u32; 8];
    // Running expected win count per user = Σ per-round (balance/total), so the
    // mid-run rebalances that change balances are accounted for exactly.
    let mut expected = [0f64; 8];
    // Local mirror of on-chain balances; only ever changed by a rebalance.
    let mut balances = BALANCES;
    let mut lcg = LCG_SEED;

    for round in 0..N_DRAWS {
        // Let the draw interval elapse, then run a full draw.
        h.env
            .ledger()
            .set_timestamp(h.env.ledger().timestamp() + DRAW_INTERVAL_SECS);
        let result = h.vault.execute_prize_draw();
        let winner = match result {
            DrawResult::Awarded(outcome) => outcome.winner,
            DrawResult::Skipped => panic!("round {round}: draw unexpectedly skipped"),
        };

        // Selection used the balances as of this round; draws never move
        // principal, so the mirror is exact.
        let total: i128 = balances.iter().sum();
        assert!(total > 0, "round {round}: pool must be non-empty");
        for (i, bal) in balances.iter().enumerate() {
            expected[i] += *bal as f64 / total as f64;
        }

        // Zero-loss + accounting invariant after every round: the draw must
        // leave total_deposits exactly equal to the Σ of user balances.
        assert_eq!(
            h.vault.get_vault_stats().total_deposits,
            total,
            "round {round}: draw must never touch principal"
        );

        let winner_idx = h
            .users
            .iter()
            .position(|u| u == winner)
            .expect("winner must be one of the depositors");
        observed[winner_idx] += 1;

        // Every 100 rounds, fully exit a deterministic 2-user subset and
        // re-deposit a fraction. This is the mid-run partial-withdrawal proof:
        // full principal must always come back before any re-deposit happens.
        if (round + 1) % REBALANCE_EVERY == 0 {
            let i = next_lcg(&mut lcg);
            let j = next_lcg(&mut lcg);
            let (a, b) = if i == j { (i, (i + 1) % 8) } else { (i, j) };
            for k in [a, b] {
                let user = h.users.get(k as u32).unwrap();
                let bal = balances[k];
                assert!(bal > 0);
                h.vault.withdraw(&user, &bal);
                let redeposit = (bal / 2).max(1);
                h.vault.deposit(&user, &redeposit);
                balances[k] = redeposit;
            }
            assert_invariants(&h, &balances, "after a mid-run rebalance");
        }
    }

    // Convergence: every observed win rate is within ±2% (absolute) of its
    // theoretical rate computed from the round-by-round balances.
    for i in 0..BALANCES.len() {
        let observed_rate = observed[i] as f64 / N_DRAWS as f64;
        let expected_rate = expected[i] / N_DRAWS as f64;
        assert!(
            (observed_rate - expected_rate).abs() < 0.02,
            "user {i}: expected rate {:.4} but observed {:.4} ({} / {N_DRAWS} wins) — outside ±2%",
            expected_rate,
            observed_rate,
            observed[i]
        );
    }

    // Final on-chain verification, then every user exits with their full
    // remaining principal (the ultimate zero-loss check after 2,000 rounds).
    assert_invariants(&h, &balances, "after 2,000 rounds");
    for (i, user) in h.users.iter().enumerate() {
        let bal = balances[i];
        if bal > 0 {
            h.vault.withdraw(&user, &bal);
        }
    }
    let stats = h.vault.get_vault_stats();
    assert_eq!(stats.total_deposits, 0, "everyone fully exited");
    assert_eq!(
        stats.participants.len(),
        0,
        "no depositors remain registered"
    );
    assert_eq!(
        token_balance(&h, &h.mock_pool),
        0,
        "pool drained of principal"
    );
    assert_eq!(
        token_balance(&h, &h.vault_id),
        0,
        "vault still holds nothing"
    );
}
