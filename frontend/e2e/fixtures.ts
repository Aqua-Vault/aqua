// Stable XDR + mock fixtures for the Playwright E2E suite. Everything here runs
// in the Node test context (never in the browser page) so we can build real,
// byte-valid Soroban XDR with @stellar/stellar-sdk and feed it to the RPC mock.
//
// The Soroban RPC layer in `lib/contract.ts` is stubbed by intercepting every
// POST to the configured testnet RPC URL and answering `getLedgerEntries`,
// `simulateTransaction`, `sendTransaction` and `getTransaction` with these
// fixtures — no wallet, no network, no deployed contract required.

import {
  Account,
  Contract,
  Keypair,
  TransactionBuilder,
  nativeToScVal,
  xdr,
} from "@stellar/stellar-sdk";

export const PASSPHRASE = "Test SDF Network ; September 2015";
export const VAULT_ID =
  "CACMVW2KK4H5FZDFF2AUCAKQTEJMZZWJUIZF23XMRVYQBSXYLHZ6BKWN";
export const POOL_ID = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAK3IM";
export const USDC_ID =
  "CAAACAQDAQCQMBYIBEFAWDANBYHRAEISCMKBKFQXDAMRUGY4DUPB6N4O";

// The wallet connected by the mocked Freighter extension.
export const USER_PUBKEY = Keypair.fromRawEd25519Seed(
  Buffer.from(new Uint8Array(32).fill(7)),
).publicKey();
export const OTHER_PUBKEY = Keypair.fromRawEd25519Seed(
  Buffer.from(new Uint8Array(32).fill(9)),
).publicKey();

// ---------------------------------------------------------------------------
// XDR builders (validated by round-tripping through xdr.fromXDR in tests)
// ---------------------------------------------------------------------------

function resourceFee(): xdr.SorobanTransactionData {
  return new xdr.SorobanTransactionData({
    ext: new (xdr as any).ExtensionPoint(0),
    resources: new xdr.SorobanResources({
      footprint: new xdr.LedgerFootprint({ readOnly: [], readWrite: [] }),
      instructions: 0,
      readBytes: 0,
      writeBytes: 0,
    }),
    resourceFee: new xdr.Int64(0),
  });
}

export function txDataB64(): string {
  return resourceFee().toXDR("base64");
}

/** A valid TransactionEnvelope XDR for a read call (never parsed in detail). */
export function envelopeB64(): string {
  const acct = new Account(USER_PUBKEY, "1");
  const tx = new TransactionBuilder(acct, {
    fee: "100",
    networkPassphrase: PASSPHRASE,
  })
    .addOperation(new Contract(VAULT_ID).call("get_vault_stats"))
    .setTimeout(30)
    .build();
  return tx.toEnvelope().toXDR("base64");
}

/** A valid TransactionResult XDR (success, empty op results). */
export function resultB64(): string {
  const txSuccess = [...(xdr as any).TransactionResultResult._switches.keys()].find(
    (k: any) => k.name === "txSuccess",
  );
  return new xdr.TransactionResult({
    feeCharged: new xdr.Int64(0),
    result: new (xdr as any).TransactionResultResult(txSuccess, []),
    ext: new (xdr as any).TransactionResultExt(0),
  }).toXDR("base64");
}

/** A valid TransactionMeta v2 XDR (no sorobanMeta => no returnValue). */
export function metaB64(): string {
  const v2 = new xdr.TransactionMetaV2({
    txChangesBefore: [],
    operations: [],
    txChangesAfter: [],
  });
  return new (xdr as any).TransactionMeta(2, v2).toXDR("base64");
}

/** A TransactionMeta v3 XDR carrying a Soroban return value. */
export function metaWithReturnValueB64(retvalB64: string): string {
  const v3 = new xdr.TransactionMetaV3({
    ext: new (xdr as any).ExtensionPoint(0),
    txChangesBefore: [],
    operations: [],
    txChangesAfter: [],
    sorobanMeta: new xdr.SorobanTransactionMeta({
      ext: new (xdr as any).SorobanTransactionMetaExt(0),
      events: [],
      returnValue: xdr.ScVal.fromXDR(retvalB64, "base64"),
      diagnosticEvents: [],
    }),
  });
  return new (xdr as any).TransactionMeta(3, v3).toXDR("base64");
}

/** A valid account LedgerEntryData XDR for the given public key. */
export function accountEntryB64(pubkey: string): string {
  const kp = Keypair.fromPublicKey(pubkey);
  const account = new xdr.AccountEntry({
    accountId: xdr.PublicKey.publicKeyTypeEd25519(kp.xdrAccountId().ed25519()),
    balance: new xdr.Int64(0),
    seqNum: new xdr.Int64(1),
    numSubEntries: 0,
    inflationDest: null,
    flags: 0,
    homeDomain: "",
    thresholds: Buffer.from([1, 1, 1, 1]),
    signers: [],
    ext: new (xdr as any).AccountEntryExt(0),
  });
  return xdr.LedgerEntryData.account(account).toXDR("base64");
}

export function i128B64(value: bigint): string {
  return nativeToScVal(value, { type: "i128" }).toXDR("base64");
}

export function statsScValB64(state: {
  totalDeposits: bigint;
  currentYield: bigint;
  secondsUntilNextDraw: number;
  participants: string[];
}): string {
  return nativeToScVal({
    total_deposits: state.totalDeposits,
    current_yield: state.currentYield,
    seconds_until_next_draw: state.secondsUntilNextDraw,
    participants: state.participants,
  }).toXDR("base64");
}

export function drawOutcomeScValB64(state: {
  winner: string;
  roll: number;
  totalWeight: bigint;
  participants: string[];
}): string {
  return nativeToScVal({
    winner: state.winner,
    roll: state.roll,
    total_weight: state.totalWeight,
    participants: state.participants,
  }).toXDR("base64");
}

export function addressScValB64(pubkey: string): string {
  return nativeToScVal(pubkey).toXDR("base64");
}

export function u64B64(value: number): string {
  return nativeToScVal(value).toXDR("base64");
}

export function simulateSuccessB64(retvalB64: string) {
  return {
    transactionData: txDataB64(),
    minResourceFee: "100",
    results: [{ auth: [], xdr: retvalB64 }],
    cost: { cpuInsns: "1000", memBytes: "1000" },
    events: [],
  };
}
