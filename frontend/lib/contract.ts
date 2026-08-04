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

const server = new rpc.Server(RPC_URL, {
  allowHttp: RPC_URL.startsWith("http://"),
});

export interface VaultStats {
  totalDeposits: bigint;
  currentYield: bigint;
  secondsUntilNextDraw: number;
  participants: string[];
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

/** Build, simulate, sign (Freighter), and submit a state-changing call. */
async function invokeWrite(
  contractId: string,
  method: string,
  args: xdr.ScVal[],
): Promise<{ hash: string; returnValue: unknown }> {
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
  const signedXdr = await signTransaction(prepared.toXDR());
  const signedTx = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE);

  const sent = await server.sendTransaction(signedTx);
  if (sent.status === "ERROR") {
    throw new Error(`Submit failed: ${JSON.stringify(sent.errorResult)}`);
  }

  // Poll for confirmation.
  const hash = sent.hash;
  let attempts = 0;
  while (attempts < 30) {
    const res = await server.getTransaction(hash);
    if (res.status === "SUCCESS") {
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
}

// ---------------------------------------------------------------------------
// Read API
// ---------------------------------------------------------------------------

export async function getVaultStats(): Promise<VaultStats> {
  const raw = await simulateRead<{
    total_deposits: bigint;
    current_yield: bigint;
    seconds_until_next_draw: bigint;
    participants: string[];
  }>(VAULT_ID, "get_vault_stats", []);

  return {
    totalDeposits: BigInt(raw.total_deposits),
    currentYield: BigInt(raw.current_yield),
    secondsUntilNextDraw: Number(raw.seconds_until_next_draw),
    participants: raw.participants ?? [],
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
  return invokeWrite(VAULT_ID, "deposit", [addr(from), i128(amount)]);
}

export async function withdraw(from: string, amount: bigint) {
  return invokeWrite(VAULT_ID, "withdraw", [addr(from), i128(amount)]);
}

export async function executePrizeDraw() {
  return invokeWrite(VAULT_ID, "execute_prize_draw", []);
}
