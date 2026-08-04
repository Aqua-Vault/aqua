// Persist recent prize-draw winners locally so the "Live Draw Feed" survives
// reloads. On-chain event indexing would replace this in production.

export interface DrawRecord {
  winner: string;
  prize: string; // raw i128 as string
  roll: string; // raw roll as string
  txHash: string;
  timestamp: number;
}

const KEY = "aqua:draws";
const MAX = 25;

export function loadDraws(): DrawRecord[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as DrawRecord[]) : [];
  } catch {
    return [];
  }
}

export function recordDraw(d: DrawRecord): DrawRecord[] {
  const existing = loadDraws().filter((x) => x.txHash !== d.txHash);
  const next = [d, ...existing].slice(0, MAX);
  try {
    window.localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* storage full / unavailable — non-fatal */
  }
  return next;
}
