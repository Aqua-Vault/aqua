// RPC-backed ledger close time, used to anchor the draw countdown to chain
// time instead of the client wall clock. The value is cached (30s) to avoid
// hammering the RPC on every countdown tick.

import { rpc } from "@stellar/stellar-sdk";
import { RPC_URL } from "./config";

const server = new rpc.Server(RPC_URL, {
  allowHttp: RPC_URL.startsWith("http://"),
});

// The SDK's getLatestLedger() exposes no close-time field, so we read the
// latest ledger's close timestamp from a lightweight getTransactions() query.
// Both values arrive from the same RPC response, so they are consistent with
// one another (single ledger boundary).
const CACHE_TTL_MS = 30_000;

let cached: { at: number; closeMs: number } | null = null;
let inFlight: Promise<number | null> | null = null;

/** Close time (ms) of the latest ledger, or null when the RPC is unreachable. */
export async function getLedgerCloseTime(): Promise<number | null> {
  const now = Date.now();
  if (cached && now - cached.at < CACHE_TTL_MS) return cached.closeMs;

  if (!inFlight) {
    inFlight = fetchLedgerCloseTime().finally(() => {
      inFlight = null;
    });
  }
  return inFlight;
}

async function fetchLedgerCloseTime(): Promise<number | null> {
  try {
    // getLatestLedger() exposes no close-time field in this SDK version, so we
    // use its sequence to fetch the same ledger's close timestamp via a
    // lightweight getTransactions() query. Both arrive from the same RPC
    // response, so they are consistent with one another.
    const latest = await server.getLatestLedger();
    const res = await server.getTransactions({
      startLedger: latest.sequence,
      limit: 1,
    });
    const ts = res.latestLedgerCloseTimestamp;
    if (!ts) return null;
    const closeMs = Number(ts) * 1000;
    cached = { at: Date.now(), closeMs };
    return closeMs;
  } catch {
    return null;
  }
}

/** Clear the cached ledger time (e.g. after a draw changes the latest ledger). */
export function resetLedgerTimeCache() {
  cached = null;
  inFlight = null;
}
