// Pure forward-looking yield math for the prize preview. Free of React and of
// any contract/config imports so it is trivially unit-testable (node --test
// with type stripping).

const SECS_PER_YEAR = 31_536_000n;
const BPS_DENOM = 10_000n;

/**
 * Simple-interest projection of the prize that will be awarded next draw:
 *   projected = currentYield + (totalDeposits × rateBps × secondsLeft) / (10_000 × 31_536_000)
 * All arithmetic is `bigint` (rates are small integers), so there is no float
 * drift; the result stays in the vault's 7-decimal USDC units (`USDC_DECIMALS`).
 */
export function projectYield(
  totalDeposits: bigint,
  rateBps: number,
  secondsLeft: number,
  currentYield: bigint = BigInt(0),
): bigint {
  if (totalDeposits <= BigInt(0) || rateBps <= 0 || secondsLeft <= 0) {
    return BigInt(0);
  }
  const accrued =
    (totalDeposits *
      BigInt(Math.trunc(rateBps)) *
      BigInt(Math.trunc(secondsLeft))) /
    (BPS_DENOM * SECS_PER_YEAR);
  return currentYield + accrued;
}

/** Human-readable gross APY percentage given a bps rate (10_000 = 100%). */
export function apyPercent(rateBps: number): string {
  if (rateBps <= 0) return "0.00";
  return (rateBps / 100).toFixed(2);
}
