// Browser + network mocks for the E2E suite.
//
// Freighter: @stellar/freighter-api v2 does NOT call `window.freighter.*`
// methods (except `isConnected`); it posts `FREIGHTER_EXTERNAL_MSG_REQUEST`
// messages to `window.postMessage` and waits for a matching
// `FREIGHTER_EXTERNAL_MSG_RESPONSE` message event. We stub postMessage to
// answer those requests with canned responses (note: the response matcher in
// freighter-api reads `messagedId`, a typo for `messageId`, so we echo both).
//
// Soroban RPC: `lib/contract.ts` talks to a single testnet RPC URL via
// jsonrpc POST. We intercept every such request and answer the four methods the
// app uses (`getLedgerEntries`, `simulateTransaction`, `sendTransaction`,
// `getTransaction`) with valid XDR fixtures built from @stellar/stellar-sdk.

import type { Page, Route } from "@playwright/test";
import { StrKey, xdr, scValToNative } from "@stellar/stellar-sdk";
import {
  PASSPHRASE,
  USER_PUBKEY,
  accountEntryB64,
  addressScValB64,
  drawOutcomeScValB64,
  envelopeB64,
  i128B64,
  metaB64,
  metaWithReturnValueB64,
  resultB64,
  simulateSuccessB64,
  statsScValB64,
} from "./fixtures";

const HASH = "aa".repeat(32);

export interface RpcState {
  totalDeposits: bigint;
  currentYield: bigint;
  secondsUntilNextDraw: number;
  participants: string[];
  vaultBalances: Record<string, bigint>;
  walletBalances: Record<string, bigint>;
  admin: string;
  pendingMetaB64: string | null;
}

export function defaultState(pubkey: string, admin: string): RpcState {
  return {
    totalDeposits: 100_000_000n,
    currentYield: 2_500_000n,
    secondsUntilNextDraw: 0,
    participants: [pubkey],
    vaultBalances: { [pubkey]: 0n },
    walletBalances: { [pubkey]: 50_000_000n },
    admin,
    pendingMetaB64: null,
  };
}

// ---------------------------------------------------------------------------
// Freighter mock (runs in the browser page)
// ---------------------------------------------------------------------------

const FREIGHTER_INIT = ([pubkey, passphrase]: string[]) => {
  let connected = false;
  const respond = (data: any, payload: any) => {
    setTimeout(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          data: {
            source: "FREIGHTER_EXTERNAL_MSG_RESPONSE",
            messagedId: data.messageId,
            messageId: data.messageId,
            ...payload,
          },
          source: window,
        }),
      );
    }, 0);
  };
  const originalPostMessage = window.postMessage.bind(window);
  window.postMessage = ((data: any, ...rest: any[]) => {
    if (data && data.source === "FREIGHTER_EXTERNAL_MSG_REQUEST") {
      switch (data.type) {
        case "REQUEST_CONNECTION_STATUS":
          respond(data, { isConnected: true });
          break;
        case "REQUEST_ACCESS":
          connected = true;
          respond(data, { publicKey: pubkey, error: "" });
          break;
        case "REQUEST_PUBLIC_KEY":
          respond(data, { publicKey: connected ? pubkey : "", error: "" });
          break;
        case "REQUEST_NETWORK_DETAILS":
          respond(data, { network: "TESTNET", networkPassphrase: passphrase, error: "" });
          break;
        case "REQUEST_ALLOWED_STATUS":
          respond(data, { isAllowed: connected, error: "" });
          break;
        case "SET_ALLOWED_STATUS":
          respond(data, { isAllowed: true, error: "" });
          break;
        case "SUBMIT_TRANSACTION":
          respond(data, { signedTransaction: data.transactionXdr, error: "" });
          break;
        default:
          respond(data, { error: "" });
      }
      return;
    }
    return originalPostMessage(data, ...rest);
  }) as typeof window.postMessage;
};

// ---------------------------------------------------------------------------
// Soroban RPC mock (runs in the Node test context)
// ---------------------------------------------------------------------------

function transactionMethodName(txB64: string): string {
  const env = xdr.TransactionEnvelope.fromXDR(txB64, "base64");
  const op = env.v1().tx().operations()[0];
  const invoke = (op.body().value() as xdr.InvokeHostFunctionOp)
    .hostFunction()
    .invokeContract();
  return invoke.functionName().toString();
}

function transactionArgs(txB64: string): xdr.ScVal[] {
  const env = xdr.TransactionEnvelope.fromXDR(txB64, "base64");
  const op = env.v1().tx().operations()[0];
  const args = (op.body().value() as xdr.InvokeHostFunctionOp)
    .hostFunction()
    .invokeContract()
    .args();
  return Array.from(args);
}

function addressFromScVal(arg: xdr.ScVal): string {
  return scValToNative(arg);
}

function pubkeyFromLedgerKey(keyB64: string): string | null {
  try {
    const key = xdr.LedgerKey.fromXDR(keyB64, "base64");
    const bytes = key.account().accountId().ed25519();
    return StrKey.encodeEd25519PublicKey(Buffer.from(bytes));
  } catch {
    return null;
  }
}

function handle(route: Route, state: RpcState) {
  const request = route.request();
  const body = request.postDataJSON();
  const method: string = body?.method ?? "";
  const id: unknown = body?.id;

  const ok = (result: unknown) => {
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ jsonrpc: "2.0", id, result }),
    });
  };

  switch (method) {
    case "getLedgerEntries": {
      const keys: string[] = body.params?.keys ?? [];
      const pubkey = keys.length > 0 ? pubkeyFromLedgerKey(keys[0]) : null;
      if (!pubkey) {
        ok({ entries: [] });
        return;
      }
      ok({
        entries: [
          {
            key: keys[0],
            xdr: accountEntryB64(pubkey),
            lastModifiedLedgerSeq: 0,
            liveUntilLedgerSeq: 0,
          },
        ],
      });
      return;
    }

    case "simulateTransaction": {
      const txB64: string = body.params?.transaction ?? "";
      const methodName = transactionMethodName(txB64);
      const args = transactionArgs(txB64);

      switch (methodName) {
        case "get_vault_stats":
          ok(
            simulateSuccessB64(
              statsScValB64({
                totalDeposits: state.totalDeposits,
                currentYield: state.currentYield,
                secondsUntilNextDraw: state.secondsUntilNextDraw,
                participants: state.participants,
              }),
            ),
          );
          return;
        case "get_user_balance": {
          const user = addressFromScVal(args[0]);
          ok(simulateSuccessB64(i128B64(state.vaultBalances[user] ?? 0n)));
          return;
        }
        case "balance": {
          const user = addressFromScVal(args[0]);
          ok(simulateSuccessB64(i128B64(state.walletBalances[user] ?? 0n)));
          return;
        }
        case "get_admin":
          ok(simulateSuccessB64(addressScValB64(state.admin)));
          return;
        case "deposit": {
          const user = addressFromScVal(args[0]);
          const amount = scValToNative(args[1]) as bigint;
          state.vaultBalances[user] = (state.vaultBalances[user] ?? 0n) + amount;
          state.walletBalances[user] = (state.walletBalances[user] ?? 0n) - amount;
          state.totalDeposits += amount;
          if (!state.participants.includes(user)) state.participants.push(user);
          const retval = i128B64(state.vaultBalances[user]);
          state.pendingMetaB64 = metaWithReturnValueB64(retval);
          ok(simulateSuccessB64(retval));
          return;
        }
        case "withdraw": {
          const user = addressFromScVal(args[0]);
          const amount = scValToNative(args[1]) as bigint;
          state.vaultBalances[user] = (state.vaultBalances[user] ?? 0n) - amount;
          state.walletBalances[user] = (state.walletBalances[user] ?? 0n) + amount;
          state.totalDeposits -= amount;
          const retval = i128B64(state.vaultBalances[user]);
          state.pendingMetaB64 = metaWithReturnValueB64(retval);
          ok(simulateSuccessB64(retval));
          return;
        }
        case "execute_prize_draw": {
          const winner =
            state.participants.find((p) => (state.vaultBalances[p] ?? 0n) > 0n) ??
            USER_PUBKEY;
          const roll = 1234;
          const outcome = drawOutcomeScValB64({
            winner,
            roll,
            totalWeight: state.totalDeposits,
            participants: state.participants,
          });
          state.currentYield = 0n;
          state.secondsUntilNextDraw = 86400;
          state.pendingMetaB64 = metaWithReturnValueB64(outcome);
          ok(simulateSuccessB64(outcome));
          return;
        }
        default:
          ok(simulateSuccessB64(i128B64(0n)));
          return;
      }
    }

    case "sendTransaction":
      ok({
        status: "PENDING",
        hash: HASH,
        latestLedger: 1,
        latestLedgerCloseTime: "1",
        latestLedgerCloseTimestamp: "1",
      });
      return;

    case "getTransaction":
      ok({
        status: "SUCCESS",
        hash: HASH,
        latestLedger: 1,
        latestLedgerCloseTime: "1",
        latestLedgerCloseTimestamp: "1",
        oldestLedger: 1,
        oldestLedgerCloseTime: "1",
        applicationOrder: 1,
        feeBump: false,
        ledger: 1,
        createdAt: "1",
        envelopeXdr: envelopeB64(),
        resultXdr: resultB64(),
        resultMetaXdr: state.pendingMetaB64 ?? metaB64(),
      });
      state.pendingMetaB64 = null;
      return;

    default:
      ok({
        error: {
          code: -32601,
          message: `Method not found: ${method}`,
        },
      });
      return;
  }
}

export async function installMocks(page: Page, state: RpcState) {
  await page.addInitScript(FREIGHTER_INIT, [USER_PUBKEY, PASSPHRASE]);
  await page.route(/soroban-testnet\.stellar\.org/, (route) =>
    handle(route, state),
  );
}
