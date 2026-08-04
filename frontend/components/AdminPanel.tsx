import { useState } from "react";
import { executePrizeDraw } from "../lib/contract";
import { recordDraw } from "../lib/history";

interface Props {
  publicKey: string | null;
  canDraw: boolean;
  onDrawComplete: () => void;
}

// Decode the DrawOutcome struct returned by execute_prize_draw. The prize
// amount isn't in the struct, so we surface the roll + winner and let the
// feed pull the prize from the emitted event / stats refresh.
function parseOutcome(ret: any): { winner: string; roll: string } | null {
  if (!ret) return null;
  const winner = ret.winner ?? ret[0];
  const roll = ret.roll ?? ret[1];
  if (!winner) return null;
  return { winner: String(winner), roll: roll != null ? String(roll) : "0" };
}

export default function AdminPanel({
  publicKey,
  canDraw,
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
      const outcome = parseOutcome(res.returnValue);
      if (outcome) {
        recordDraw({
          winner: outcome.winner,
          prize: "0", // refreshed from stats; struct omits prize amount
          roll: outcome.roll,
          txHash: res.hash,
          timestamp: Date.now(),
        });
      }
      setMsg({
        kind: "ok",
        text: `Draw executed! Winner: ${
          outcome ? outcome.winner.slice(0, 6) + "…" : "see feed"
        }`,
      });
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

      {!canDraw && (
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
