import { VaultStats } from "../lib/contract";
import { formatUsd } from "../lib/format";
import CountdownTimer from "./CountdownTimer";

interface Props {
  stats: VaultStats | null;
  ledgerCloseMs: number | null;
  loading: boolean;
}

export default function StatsBar({ stats, ledgerCloseMs, loading }: Props) {
  const prize = stats?.currentYield ?? BigInt(0);
  const tvl = stats?.totalDeposits ?? BigInt(0);
  const participants = stats?.participants.length ?? 0;

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
      {/* Prize pool — the headline number. */}
      <div className="card relative overflow-hidden">
        <div className="absolute inset-0 animate-shimmer bg-gradient-to-r from-transparent via-aqua-400/10 to-transparent bg-[length:200%_100%]" />
        <div className="relative">
          <div className="stat-label">Current Prize Pool</div>
          <div className="stat-value text-aqua-200">
            {loading && !stats ? "…" : formatUsd(prize)}
          </div>
          <div className="mt-1 text-xs text-slate-400">
            100% of yield · winner takes all
          </div>
        </div>
      </div>

      {/* Countdown */}
      <div className="card">
        <div className="stat-label">Next Draw In</div>
        <div className="stat-value">
          {loading && !stats ? (
            "…"
          ) : (
            <CountdownTimer
              initialSeconds={stats?.secondsUntilNextDraw ?? 0}
              ledgerCloseMs={ledgerCloseMs}
            />
          )}
        </div>
        <div className="mt-1 text-xs text-slate-400">
          CAP-0074 on-chain random draw
        </div>
      </div>

      {/* TVL */}
      <div className="card">
        <div className="stat-label">Total Value Locked</div>
        <div className="stat-value">
          {loading && !stats ? "…" : formatUsd(tvl)}
        </div>
        <div className="mt-1 text-xs text-slate-400">
          {participants} saver{participants === 1 ? "" : "s"} · fully withdrawable
        </div>
      </div>
    </div>
  );
}
