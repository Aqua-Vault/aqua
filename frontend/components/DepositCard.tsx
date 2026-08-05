import { VaultStats } from "../lib/contract";
import { formatUsd, winProbability } from "../lib/format";
import { computePoolShare } from "../lib/probability";

interface Props {
  publicKey: string | null;
  userBalance: bigint;
  stats: VaultStats | null;
}

export default function DepositCard({ publicKey, userBalance, stats }: Props) {
  const tvl = stats?.totalDeposits ?? BigInt(0);
  const prob = winProbability(userBalance, tvl);
  const share = computePoolShare(userBalance, tvl) * 100;
  const hasDeposit = userBalance > BigInt(0);

  return (
    <div className="card">
      <h3 className="text-lg font-semibold text-white">Your Position</h3>

      {!publicKey ? (
        <p className="mt-4 text-sm text-slate-400">
          Connect your wallet to see your deposit and win probability.
        </p>
      ) : (
        <div className="mt-4 space-y-4">
          <div>
            <div className="stat-label">Active Deposit</div>
            <div className="stat-value">{formatUsd(userBalance)}</div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="rounded-xl bg-ink-900/60 p-4">
              <div className="stat-label">Win Probability</div>
              <div className="mt-1 text-xl font-bold text-aqua-200">
                {hasDeposit ? `${prob}%` : "—"}
              </div>
            </div>
            <div className="rounded-xl bg-ink-900/60 p-4">
              <div className="stat-label">Pool Share</div>
              <div className="mt-1 text-xl font-bold text-white">
                {hasDeposit ? `${share.toFixed(2)}%` : "—"}
              </div>
            </div>
          </div>

          {hasDeposit && (
            <div className="rounded-xl border border-aqua-500/20 bg-aqua-500/5 p-3">
              <p className="text-xs text-aqua-100/80">
                Every USDC you hold is a lottery ticket. Your odds scale with
                your deposit — and your principal stays 100% withdrawable.
              </p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
