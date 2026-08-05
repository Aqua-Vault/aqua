import { useEffect, useState } from "react";
import { computeCountdownRemaining } from "../lib/countdown";
import { formatCountdown } from "../lib/format";

interface Props {
  // Seconds until next draw, as read from the chain.
  initialSeconds: number;
  // Close time (ms) of the ledger that produced `initialSeconds`. When null
  // (ledger fetch failed), the countdown degrades to wall-clock anchoring.
  ledgerCloseMs: number | null;
}

// Client-side ticking countdown anchored to ledger close time rather than the
// wall clock, so client clock skew can't make it hit zero early (or late)
// relative to the chain's `e.ledger().timestamp()` gate in execute_prize_draw.
export default function CountdownTimer({
  initialSeconds,
  ledgerCloseMs,
}: Props) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [initialSeconds, ledgerCloseMs]);

  // Wall-clock fallback: treat "now" as the ledger close anchor, so the
  // display decrements exactly as before when RPC ledger time is unavailable.
  const anchorMs = ledgerCloseMs ?? now;
  const remaining = computeCountdownRemaining(initialSeconds, anchorMs, now);
  const ready = remaining <= 0;

  return (
    <span className={ready ? "text-emerald-300" : "text-white"}>
      {formatCountdown(remaining)}
    </span>
  );
}
