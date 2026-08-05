// React hook for managing Freighter wallet connection state.

import { useEffect, useRef, useState } from "react";
import {
  connectWallet as doConnect,
  getPublicKey,
  getWalletNetwork,
  isFreighterInstalled,
  subscribeToAccountChanges,
  subscribeToNetworkChanges,
} from "../lib/freighter";
import { NETWORK_PASSPHRASE, NETWORK } from "../lib/config";

export function useWallet() {
  const [publicKey, setPublicKey] = useState<string | null>(null);
  const [isInstalled, setIsInstalled] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  // Non-null when the wallet's network differs from the app's configured one.
  const [networkMismatch, setNetworkMismatch] = useState<string | null>(null);
  // Set true when the Freighter account changed since the last render. Cleared
  // via acknowledgeAccountChange().
  const [accountChanged, setAccountChanged] = useState(false);
  // Tracks the latest key so subscription callbacks (which close over the
  // render's publicKey) never fire a spurious first-change event.
  const publicKeyRef = useRef<string | null>(null);

  // Compare the wallet's active network against the app's configured network.
  async function checkNetwork() {
    const details = await getWalletNetwork();
    if (details && details.networkPassphrase !== NETWORK_PASSPHRASE) {
      setNetworkMismatch(
        `Freighter is on "${details.network}", but this app is configured for ${NETWORK}. ` +
          `Switch networks in Freighter to transact.`,
      );
    } else {
      setNetworkMismatch(null);
    }
  }

  // Check Freighter installation and connection state on mount.
  useEffect(() => {
    async function check() {
      setIsLoading(true);
      const installed = await isFreighterInstalled();
      setIsInstalled(installed);
      if (installed) {
        const pk = await getPublicKey();
        setPublicKey(pk);
        if (pk) await checkNetwork();
      }
      setIsLoading(false);
    }
    check();
  }, []);

  // Poll for account/network switches while connected.
  useEffect(() => {
    if (!isInstalled || !publicKey) return;
    const unsubAccount = subscribeToAccountChanges((pk) => {
      if (pk !== publicKeyRef.current) {
        publicKeyRef.current = pk;
        setPublicKey(pk);
        setAccountChanged(true);
      }
    });
    const unsubNetwork = subscribeToNetworkChanges(() => {
      void checkNetwork();
    });
    return () => {
      unsubAccount();
      unsubNetwork();
    };
  }, [isInstalled, publicKey]);

  async function connect() {
    try {
      const pk = await doConnect();
      setPublicKey(pk);
      await checkNetwork();
      return pk;
    } catch (err: any) {
      throw new Error(err?.message || "Failed to connect wallet");
    }
  }

  function disconnect() {
    publicKeyRef.current = null;
    setPublicKey(null);
    setNetworkMismatch(null);
    setAccountChanged(false);
  }

  function acknowledgeAccountChange() {
    setAccountChanged(false);
  }

  return {
    publicKey,
    isConnected: Boolean(publicKey),
    isInstalled,
    isLoading,
    networkMismatch,
    accountChanged,
    connect,
    disconnect,
    acknowledgeAccountChange,
  };
}
