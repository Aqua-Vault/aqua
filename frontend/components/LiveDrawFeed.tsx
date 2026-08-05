import { DrawRecord } from "../lib/history";
import { explorerTxUrl } from "../lib/config";
import { formatUsd, shortenAddress, shortenHash } from "../lib/format";

interface Props {
  draws: DrawRecord[];
}

export default function LiveDrawFeed({ draws }: Props) {
  return (
    <div className="card">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-white">Live Draw Feed</h3>
        <span className="flex items-center gap-1.5 text-xs text-slate-400">
          <span className="h-2 w-2 animate-pulse rounded-full bg-emerald-400" />
          Live
        </span>
      </div>

      {draws.length === 0 ? (
        <p className="mt-4 text-sm text-slate-400">
          No draws yet. Winners will appear here with a link to Stellar Expert.
        </p>
      ) : (
        <ul className="mt-4 divide-y divide-white/5">
          {draws.map((d) => (
            <li
              key={d.txHash}
              className="flex items-center justify-between py-3"
            >
              <div className="flex items-center gap-3">
                <div className="flex h-9 w-9 items-center justify-center rounded-full bg-aqua-500/15 text-aqua-200">
                  🏆
                </div>
                <div>
                  <div className="font-mono text-sm text-white">
                    {shortenAddress(d.winner)}
                  </div>
                  <div className="text-xs text-slate-500">
                    {new Date(d.timestamp).toLocaleString()}
                  </div>
                </div>
              </div>
              <div className="text-right">
                {d.prize !== "0" && (
                  <div className="text-sm font-semibold text-emerald-300">
                    {formatUsd(d.prize)}
                  </div>
                )}
                {d.roll !== "0" && (
                  <div className="text-xs text-slate-500">
                    roll {d.roll}
                  </div>
                )}
                {d.txHash ? (
                  <a
                    href={explorerTxUrl(d.txHash)}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-xs text-aqua-300 hover:text-aqua-200"
                  >
                    {shortenHash(d.txHash)} ↗
                  </a>
                ) : (
                  <span className="text-xs italic text-slate-600">
                    unverified on-chain
                  </span>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
