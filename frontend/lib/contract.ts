// Soroban contract interaction layer for the Aqua vault.
//
// Read calls are simulated (no signature, no fee). Write calls are built,
// simulated for footprint/fees, signed via Freighter, then submitted.

import {
  Address,
  BASE_FEE,
  Contract,
  nativeToScVal,
  scValToNative,
  rpc,
  TransactionBuilder,
  xdr,
} from "@stellar/stellar-sdk";
import {
  NETWORK_PASSPHRASE,
  RPC_URL,
  USDC_ID,
  VAULT_ID,
} from "./config";
import { signTransaction, getPublicKey } from "./freighter";
import { getLedgerCloseTime } from "./ledger";

const server = new rpc.Server(RPC_URL, {
  allowHttp: RPC_URL.startsWith("http://"),
});

export interface VaultStats {
  totalDeposits: bigint;
  currentYield: bigint;
  secondsUntilNextDraw: number;
  /** Gross annual yield-pool rate in basis points (10_000 = 100%). 0 = unknown. */
  annualRateBps: number;
  participants: string[];
  /** Close time (ms) of the ledger the stats were read at; null if unavailable. */
  ledgerCloseMs: number | null;
  /** `true` when the emergency circuit breaker is engaged (deposits/draws blocked). */
  paused: boolean;
}

/** The `DrawResult` enum returned by `execute_prize_draw` (union encoding). */
export type DrawResult =
  | { tag: "Awarded"; values: [DrawOutcome] }
  | { tag: "Skipped"; values: [] };

export interface DrawOutcome {
  winner: string;
  roll: bigint;
  total_weight: bigint;
  participants: string[];
}

/**
 * Decode the `DrawResult` union produced by `scValToNative` on the Soroban
 * `#[contracttype]` enum: `{ tag, values }`. `Awarded` carries the
 * `DrawOutcome` struct as `values[0]`; `Skipped` carries nothing.
 */
export function decodeDrawResult(ret: unknown): DrawResult | null {
  if (!ret || typeof ret !== "object") return null;
  const r = ret as { tag?: string; values?: unknown[] };
  if (r.tag === "Awarded") {
    const outcome = (r.values?.[0] ?? null) as DrawOutcome | null;
    if (!outcome || typeof outcome.winner !== "string") return null;
    return { tag: "Awarded", values: [outcome] };
  }
  if (r.tag === "Skipped") {
    return { tag: "Skipped", values: [] };
  }
  return null;
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

function i128(value: bigint): xdr.ScVal {
  return nativeToScVal(value, { type: "i128" });
}

function addr(a: string): xdr.ScVal {
  return new Address(a).toScVal();
}

/** Simulate a read-only contract call and decode the result to native JS. */
async function simulateRead<T>(
  contractId: string,
  method: string,
  args: xdr.ScVal[],
  sourceAccount?: string,
): Promise<T> {
  const source = sourceAccount || (await firstReadSource());
  const account = await server.getAccount(source).catch(() => null);
  // Reads don't need a real, funded account footprint; fall back to a dummy
  // sequence when the source isn't retrievable (e.g. no wallet connected yet).
  const acc =
    account ??
    new (require("@stellar/stellar-sdk").Account)(source, "0");

  const contract = new Contract(contractId);
  const tx = new TransactionBuilder(acc, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) {
    throw new Error(`Simulation failed for ${method}: ${sim.error}`);
  }
  const retval = sim.result?.retval;
  if (!retval) throw new Error(`No result from ${method}`);
  return scValToNative(retval) as T;
}

// A well-known funded testnet account used purely as a simulation source for
// reads when no wallet is connected. (GA...ANHUF is the all-zero placeholder.)
const READ_SOURCE =
  "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAANHUF";

async function firstReadSource(): Promise<string> {
  try {
    const pk = await getPublicKey();
    if (pk) return pk;
  } catch {
    /* wallet not connected — use placeholder */
  }
  return READ_SOURCE;
}

// ---------------------------------------------------------------------------
// Transaction lifecycle emitter (toast notifications)
// ---------------------------------------------------------------------------

export type TxOp = "deposit" | "withdraw" | "draw";
export type TxState =
  | "submitting"
  | "signing"
  | "submitted"
  | "confirmed"
  | "failed";

export interface TxLifecycleEvent {
  op: TxOp;
  state: TxState;
  txHash?: string;
  message?: string;
}

type TxLifecycleHandler = (event: TxLifecycleEvent) => void;

// Framework-free callback: the UI registers a handler (e.g. the ToastProvider)
// and every write op reports its progress through the pipeline from here.
let txLifecycleHandler: TxLifecycleHandler | null = null;

export function setTxLifecycleHandler(fn: TxLifecycleHandler | null) {
  txLifecycleHandler = fn;
}

function emitTx(event: TxLifecycleEvent) {
  txLifecycleHandler?.(event);
}

/** Build, simulate, sign (Freighter), and submit a state-changing call. */
async function invokeWrite(
  contractId: string,
  method: string,
  args: xdr.ScVal[],
  op: TxOp,
): Promise<{ hash: string; returnValue: unknown }> {
  emitTx({ op, state: "submitting", message: "Preparing transaction…" });
  try {
    const publicKey = await getPublicKey();
    if (!publicKey) throw new Error("Wallet not connected");

    const account = await server.getAccount(publicKey);
    const contract = new Contract(contractId);

    const built = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: NETWORK_PASSPHRASE,
    })
      .addOperation(contract.call(method, ...args))
      .setTimeout(60)
      .build();

    // Simulate to obtain the Soroban footprint + resource fees, then assemble.
    const sim = await server.simulateTransaction(built);
    if (rpc.Api.isSimulationError(sim)) {
      throw new Error(`Simulation failed for ${method}: ${sim.error}`);
    }
    const prepared = rpc.assembleTransaction(built, sim).build();

    // Sign via Freighter, rebuild from signed XDR, and submit.
    emitTx({ op, state: "signing", message: "Waiting for signature in Freighter…" });
    const signedXdr = await signTransaction(prepared.toXDR());
    const signedTx = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE);

    const sent = await server.sendTransaction(signedTx);
    if (sent.status === "ERROR") {
      throw new Error(`Submit failed: ${JSON.stringify(sent.errorResult)}`);
    }

    // Poll for confirmation.
    const hash = sent.hash;
    emitTx({ op, state: "submitted", txHash: hash, message: "Broadcast to network…" });
    let attempts = 0;
    while (attempts < 30) {
      const res = await server.getTransaction(hash);
      if (res.status === "SUCCESS") {
        emitTx({ op, state: "confirmed", txHash: hash, message: "Transaction confirmed" });
        return {
          hash,
          returnValue: res.returnValue ? scValToNative(res.returnValue) : null,
        };
      }
      if (res.status === "FAILED") {
        throw new Error(`Transaction ${hash} failed on-chain`);
      }
      await new Promise((r) => setTimeout(r, 1000));
      attempts++;
    }
    throw new Error(`Transaction ${hash} not confirmed in time`);
  } catch (err: any) {
    // Components push the `failed` toast via useToast() so the message lands
    // next to their inline error without duplicating it here.
    throw err;
  }
}

// ---------------------------------------------------------------------------
// Read API
// ---------------------------------------------------------------------------

export async function getVaultStats(): Promise<VaultStats> {
  const [raw, ledgerCloseMs] = await Promise.all([
    simulateRead<{
      total_deposits: bigint;
      current_yield: bigint;
      seconds_until_next_draw: bigint;
      annual_rate_bps: bigint;
      participants: string[];
      paused: boolean;
    }>(VAULT_ID, "get_vault_stats", []),
    getLedgerCloseTime(),
  ]);

  return {
    totalDeposits: BigInt(raw.total_deposits),
    currentYield: BigInt(raw.current_yield),
    secondsUntilNextDraw: Number(raw.seconds_until_next_draw),
    annualRateBps: Number(raw.annual_rate_bps ?? 0),
    participants: raw.participants ?? [],
    ledgerCloseMs,
    paused: Boolean(raw.paused),
  };
}

export async function getUserBalance(user: string): Promise<bigint> {
  const raw = await simulateRead<bigint>(
    VAULT_ID,
    "get_user_balance",
    [addr(user)],
    user,
  );
  return BigInt(raw);
}

export async function getAdmin(): Promise<string> {
  return simulateRead<string>(VAULT_ID, "get_admin", []);
}

/** Read a raw USDC token balance for an address. */
export async function getUsdcBalance(user: string): Promise<bigint> {
  const raw = await simulateRead<bigint>(
    USDC_ID,
    "balance",
    [addr(user)],
    user,
  );
  return BigInt(raw);
}

// ---------------------------------------------------------------------------
// Write API
// ---------------------------------------------------------------------------

export async function deposit(from: string, amount: bigint) {
  return invokeWrite(VAULT_ID, "deposit", [addr(from), i128(amount)], "deposit");
}

export async function withdraw(from: string, amount: bigint) {
  return invokeWrite(VAULT_ID, "withdraw", [addr(from), i128(amount)], "withdraw");
}

export async function executePrizeDraw() {
  return invokeWrite(VAULT_ID, "execute_prize_draw", [], "draw");
}
