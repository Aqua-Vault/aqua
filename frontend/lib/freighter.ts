// Thin wrapper over @stellar/freighter-api (v2, string-returning API) with
// graceful fallbacks so the UI can render even when the extension isn't
// installed. This version's functions return bare strings/booleans and throw
// on failure, so we wrap them defensively.

import {
  isConnected as fIsConnected,
  isAllowed as fIsAllowed,
  setAllowed as fSetAllowed,
  requestAccess as fRequestAccess,
  getPublicKey as fGetPublicKey,
  getNetworkDetails as fGetNetworkDetails,
  signTransaction as fSignTransaction,
} from "@stellar/freighter-api";
import { NETWORK_PASSPHRASE } from "./config";

export async function isFreighterInstalled(): Promise<boolean> {
  try {
    return await fIsConnected();
  } catch {
    return false;
  }
}

/** Prompt the user to connect and return their public key, or throw. */
export async function connectWallet(): Promise<string> {
  const installed = await isFreighterInstalled();
  if (!installed) {
    throw new Error(
      "Freighter wallet not detected. Install it from freighter.app to continue.",
    );
  }
  // requestAccess pops the approval prompt and returns the public key.
  const pk = await fRequestAccess();
  if (!pk) throw new Error("Access to Freighter was denied");
  try {
    await fSetAllowed();
  } catch {
    /* already allowed — non-fatal */
  }
  return pk;
}

/** Return the connected public key without prompting, or null if unavailable. */
export async function getPublicKey(): Promise<string | null> {
  try {
    const allowed = await fIsAllowed();
    if (!allowed) return null;
    const pk = await fGetPublicKey();
    return pk || null;
  } catch {
    return null;
  }
}

/** Current network the wallet is pointed at (for mismatch warnings). */
export async function getWalletNetwork(): Promise<{
  network: string;
  networkPassphrase: string;
} | null> {
  try {
    const d = await fGetNetworkDetails();
    return { network: d.network, networkPassphrase: d.networkPassphrase };
  } catch {
    return null;
  }
}

/** Sign a transaction XDR and return the signed XDR string. */
export async function signTransaction(xdrStr: string): Promise<string> {
  const signed = await fSignTransaction(xdrStr, {
    networkPassphrase: NETWORK_PASSPHRASE,
  });
  if (!signed) throw new Error("Signing was rejected");
  return signed;
}
