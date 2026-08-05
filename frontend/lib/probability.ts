// Pure win-probability math for the ProbabilityCalculator. Kept free of React
// (and free of formatting) so it is trivially unit-testable.

/** Win probability (%) of a depositor holding `balance` against `total` pool
 * principal. Returns 0 when either side is non-positive; the calculator
 * component renders a friendly empty state for that case. */
export function computeWinProbability(balance: bigint, total: bigint): number {
  if (total <= BigInt(0) || balance <= BigInt(0)) return 0;
  return (Number(balance) / Number(total)) * 100;
}

/** The user's share of the pool as a fraction in `[0, 1]`. */
export function computePoolShare(balance: bigint, total: bigint): number {
  if (total <= BigInt(0) || balance <= BigInt(0)) return 0;
  return Number(balance) / Number(total);
}
