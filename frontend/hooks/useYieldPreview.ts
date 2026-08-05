// Forward-looking prize preview: given the pool's annual rate (bps) and the
// seconds until the next draw, estimate the next prize from the mock pool's
// simple-interest model. Pure math is in `lib/yield.ts` (unit-tested).

import { useMemo } from "react";
import type { VaultStats } from "../lib/contract";
import { IS_CONFIGURED } from "../lib/config";
import { projectYield, apyPercent } from "../lib/yield";

export { projectYield, apyPercent };

export interface YieldPreview {
  /** Gross annual yield as a percentage string (e.g. "10.00"). */
  apyPct: string;
  /** Projected next prize in raw 7-decimal units. */
  projectedNextPrize: bigint;
  /** Seconds until the next draw, straight from stats. */
  secondsLeft: number;
  loading: boolean;
}

/** Hook deriving the projected next prize from the 10s `useVault` poll. */
export function useYieldPreview(
  stats: VaultStats | null,
): YieldPreview {
  const loading = !IS_CONFIGURED || !stats;

  return useMemo(() => {
    const total = stats?.totalDeposits ?? BigInt(0);
    const rateBps = stats?.annualRateBps ?? 0;
    const secondsLeft = stats?.secondsUntilNextDraw ?? 0;
    const currentYield = stats?.currentYield ?? BigInt(0);
    return {
      apyPct: apyPercent(rateBps),
      projectedNextPrize: projectYield(total, rateBps, secondsLeft, currentYield),
      secondsLeft,
      loading,
    };
  }, [
    stats?.totalDeposits,
    stats?.annualRateBps,
    stats?.secondsUntilNextDraw,
    stats?.currentYield,
    loading,
  ]);
}
