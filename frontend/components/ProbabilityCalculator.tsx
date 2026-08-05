import { useMemo } from "react";
import {
  computePoolShare,
  computeWinProbability,
} from "../lib/probability";
import { formatUsd } from "../lib/format";

interface Props {
  userBalance: bigint;
  totalDeposits: bigint;
  /** Optional not-yet-submitted deposit amount to preview projected odds. */
  projectedDeposit?: bigint;
}

const fmt = (percent: number) => `${percent.toFixed(2)}%`;

// Real-time win-probability panel. Reads straight from useVault state and the
// ActionPanel's live deposit input, so odds update as the user types — no
// division-by-zero, no NaN/Infinity.
export default function ProbabilityCalculator({
  userBalance,
  totalDeposits,
  projectedDeposit,
}: Props) {
  const empty = totalDeposits <= BigInt(0);

  const currentPct = useMemo(
    () => computeWinProbability(userBalance, totalDeposits),
    [userBalance, totalDeposits],
  );
  const projectedPct = useMemo(() => {
    if (projectedDeposit === undefined || projectedDeposit <= BigInt(0)) {
      return null;
    }
    return computeWinProbability(
      userBalance + projectedDeposit,
      totalDeposits + projectedDeposit,
    );
  }, [userBalance, totalDeposits, projectedDeposit]);
  const share = useMemo(
    () => computePoolShare(userBalance, totalDeposits),
    [userBalance, totalDeposits],
  );

  if (empty) {
    return (
      <div
        role="status"
        aria-live="polite"
        className="rounded-xl border border-aqua-500/20 bg-aqua-500/5 p-4 text-sm text-aqua-100/80"
      >
        Be the first depositor —{" "}
        <span className="font-semibold text-white">100%</span> until someone
        joins.
      </div>
    );
  }

  return (
    <div
      role="status"
      aria-live="polite"
      className="rounded-xl border border-aqua-500/20 bg-aqua-500/5 p-4"
    >
      <div className="stat-label">Your Win Probability</div>
      <div className="mt-2 grid grid-cols-3 gap-3 text-center">
        <div>
          <div className="text-xs text-slate-400">Current</div>
          <div className="mt-1 text-lg font-bold text-aqua-200">
            {fmt(currentPct)}
          </div>
        </div>
        <div>
          <div className="text-xs text-slate-400">With deposit</div>
          <div className="mt-1 text-lg font-bold text-white">
            {projectedPct === null ? "—" : fmt(projectedPct)}
          </div>
        </div>
        <div>
          <div className="text-xs text-slate-400">Pool share</div>
          <div className="mt-1 text-lg font-bold text-white">
            {fmt(share * 100)}
          </div>
        </div>
      </div>
      {projectedPct !== null && (
        <p className="mt-2 text-xs text-slate-400">
          Odds if you deposit {formatUsd(projectedDeposit ?? BigInt(0))}
        </p>
      )}
    </div>
  );
}
