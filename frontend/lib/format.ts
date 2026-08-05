import { USDC_DECIMALS } from "./config";

const SCALE = BigInt(10) ** BigInt(USDC_DECIMALS);

/** Convert a raw i128 stroop-style amount (bigint) into a human decimal string. */
export function fromStroops(raw: bigint | string, maxFractionDigits = 2): string {
  const v = typeof raw === "string" ? BigInt(raw) : raw;
  const whole = v / SCALE;
  const frac = v % SCALE;
  if (maxFractionDigits === 0) return whole.toString();

  const fracStr = frac
    .toString()
    .padStart(USDC_DECIMALS, "0")
    .slice(0, maxFractionDigits)
    .replace(/0+$/, "");

  return fracStr.length > 0 ? `${whole}.${fracStr}` : whole.toString();
}

/** Convert a human decimal string (e.g. "12.5") into a raw i128 bigint. */
export function toStroops(amount: string): bigint {
  const trimmed = amount.trim();
  if (!trimmed) return BigInt(0);
  const [whole, frac = ""] = trimmed.split(".");
  const fracPadded = frac.padEnd(USDC_DECIMALS, "0").slice(0, USDC_DECIMALS);
  return BigInt(whole || "0") * SCALE + BigInt(fracPadded || "0");
}

/** Format a USD amount with a currency prefix and thousands separators. */
export function formatUsd(raw: bigint | string, maxFractionDigits = 2): string {
  const s = fromStroops(raw, maxFractionDigits);
  const [whole, frac] = s.split(".");
  const withCommas = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return frac ? `$${withCommas}.${frac}` : `$${withCommas}`;
}

/** Format seconds as a compact HH:MM:SS-ish countdown. */
export function formatCountdown(totalSeconds: number): string {
  if (totalSeconds <= 0) return "Ready to draw";
  const d = Math.floor(totalSeconds / 86400);
  const h = Math.floor((totalSeconds % 86400) / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = Math.floor(totalSeconds % 60);
  const pad = (n: number) => n.toString().padStart(2, "0");
  if (d > 0) return `${d}d ${pad(h)}:${pad(m)}:${pad(s)}`;
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

/** Shorten a Stellar address / contract id for display. */
export function shortenAddress(addr: string, chars = 4): string {
  if (addr.length <= chars * 2 + 3) return addr;
  return `${addr.slice(0, chars)}…${addr.slice(-chars)}`;
}

/** Shorten a 64-hex transaction hash for display. */
export function shortenHash(hash: string, chars = 6): string {
  if (hash.length <= chars * 2 + 3) return hash;
  return `${hash.slice(0, chars)}…${hash.slice(-chars)}`;
}

/** Win probability as a percentage string. */
export function winProbability(userDeposit: bigint, tvl: bigint): string {
  if (tvl <= BigInt(0)) return "0.00";
  // Scale up for precision, then format.
  const pct = (Number(userDeposit) / Number(tvl)) * 100;
  return pct.toFixed(2);
}
