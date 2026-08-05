import { useState } from "react";
import { decodeDrawResult, executePrizeDraw } from "../lib/contract";
import { recordDraw } from "../lib/history";

interface Props {
  publicKey: string | null;
  canDraw: boolean;
  paused: boolean;
  onDrawComplete: () => void;
}

export default function AdminPanel({
  publicKey,
  canDraw,
  paused,
  onDrawComplete,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(
    null,
  );

  async function handleDraw() {
    if (!publicKey) {
      setMsg({ kind: "err", text: "Connect a wallet first" });
      return;
    }
    setBusy(true);
    setMsg(null);
    try {
      const res = await executePrizeDraw();
      const draw = decodeDrawResult(res.returnValue);
      if (draw?.tag === "Awarded") {
        const outcome = draw.values[0];
        // The prize amount isn't in the struct; surface the roll + winner and
        // let the feed pull the prize from the emitted event / stats refresh.
        recordDraw({
          winner: outcome.winner,
          prize: "0",
          roll: String(outcome.roll),
          txHash: res.hash,
          timestamp: Date.now(),
        });
        setMsg({
          kind: "ok",
          text: `Draw executed! Winner: ${outcome.winner.slice(0, 6)}…`,
        });
      } else if (draw?.tag === "Skipped") {
        setMsg({
          kind: "ok",
          text: "No yield this round — draw skipped and timer re-armed.",
        });
      } else {
        setMsg({ kind: "ok", text: "Draw executed!" });
      }
      onDrawComplete();
    } catch (err: any) {
      setMsg({ kind: "err", text: err?.message || "Draw failed" });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card border-amber-500/20">
      <div className="flex items-center gap-2">
        <span className="rounded-md bg-amber-500/20 px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-amber-300">
          Testnet
        </span>
        <h3 className="text-lg font-semibold text-white">Trigger Prize Draw</h3>
      </div>

      <p className="mt-2 text-sm text-slate-400">
        Manually run the CAP-0074 weighted random draw. Anyone may call it once
        the interval elapses; the admin can force it early for demos.
      </p>

      {!canDraw && paused && (
        <p className="mt-2 text-xs text-amber-300/80">
          The vault is paused — draws are disabled until the admin unpauses.
        </p>
      )}

      {!canDraw && !paused && (
        <p className="mt-2 text-xs text-amber-300/80">
          The draw interval hasn&apos;t elapsed yet — this may revert with{" "}
          <code className="font-mono">TooEarly</code> unless you&apos;re the
          admin.
        </p>
      )}
      <button
        className="btn-primary mt-4 w-full bg-amber-500 hover:bg-amber-400 active:bg-amber-600"
        onClick={handleDraw}
        disabled={busy}
      >
        {busy ? "Drawing…" : "Execute Prize Draw"}
      </button>

      {msg && (
        <p
          className={`mt-3 text-sm ${
            msg.kind === "ok" ? "text-emerald-300" : "text-rose-300"
          }`}
        >
          {msg.text}
        </p>
      )}
    </div>
  );
}
