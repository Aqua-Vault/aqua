// Pure countdown math for the ledger-anchored draw ticker. Free of React so it
// is unit-testable.

/** Seconds remaining until the next draw is eligible.
 *
 * `chainSeconds` is `secondsUntilNextDraw` read from the chain. The chain's
 * ledger closed at `ledgerCloseMs`, and the current client wall clock is
 * `nowMs`. The remaining time is the chain value reduced by the wall-clock
 * elapsed time since that ledger closed, clamped to `[0, chainSeconds]`.
 *
 * When ledger time is unavailable, callers pass `nowMs === ledgerCloseMs`,
 * which degrades to pure wall-clock anchoring (the pre-fix behavior).
 */
export function computeCountdownRemaining(
  chainSeconds: number,
  ledgerCloseMs: number,
  nowMs: number,
): number {
  if (chainSeconds <= 0) return 0;
  const elapsed = (nowMs - ledgerCloseMs) / 1000;
  return Math.max(0, Math.min(chainSeconds, chainSeconds - elapsed));
}
