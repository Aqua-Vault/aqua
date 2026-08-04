import { useEffect, useState } from "react";
import { formatCountdown } from "../lib/format";

interface Props {
  // Seconds until next draw, as read from the chain at `anchorMs`.
  initialSeconds: number;
  anchorMs: number;
}

// Client-side ticking countdown that decrements from the last chain reading.
export default function CountdownTimer({ initialSeconds, anchorMs }: Props) {
  const [display, setDisplay] = useState(initialSeconds);

  useEffect(() => {
    function tick() {
      const elapsed = (Date.now() - anchorMs) / 1000;
      setDisplay(Math.max(0, initialSeconds - elapsed));
    }
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [initialSeconds, anchorMs]);

  const ready = display <= 0;

  return (
    <span className={ready ? "text-emerald-300" : "text-white"}>
      {formatCountdown(display)}
    </span>
  );
}
