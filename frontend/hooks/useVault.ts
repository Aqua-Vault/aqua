// React hook that loads and refreshes vault + user state from the chain.

import { useCallback, useEffect, useState } from "react";
import {
  getUserBalance,
  getUsdcBalance,
  getVaultStats,
  VaultStats,
} from "../lib/contract";
import { IS_CONFIGURED } from "../lib/config";

export interface VaultState {
  stats: VaultStats | null;
  userBalance: bigint;
  usdcBalance: bigint;
  loading: boolean;
  error: string | null;
}

const EMPTY: VaultState = {
  stats: null,
  userBalance: BigInt(0),
  usdcBalance: BigInt(0),
  loading: true,
  error: null,
};

export function useVault(publicKey: string | null, pollMs = 10000) {
  const [state, setState] = useState<VaultState>(EMPTY);

  const refresh = useCallback(async () => {
    if (!IS_CONFIGURED) {
      setState((s) => ({
        ...s,
        loading: false,
        error: "Contracts not configured. Set NEXT_PUBLIC_* env vars.",
      }));
      return;
    }
    try {
      const stats = await getVaultStats();
      let userBalance = BigInt(0);
      let usdcBalance = BigInt(0);
      if (publicKey) {
        [userBalance, usdcBalance] = await Promise.all([
          getUserBalance(publicKey).catch(() => BigInt(0)),
          getUsdcBalance(publicKey).catch(() => BigInt(0)),
        ]);
      }
      setState({ stats, userBalance, usdcBalance, loading: false, error: null });
    } catch (err: any) {
      setState((s) => ({
        ...s,
        loading: false,
        error: err?.message || "Failed to load vault state",
      }));
    }
  }, [publicKey]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, pollMs);
    return () => clearInterval(id);
  }, [refresh, pollMs]);

  return { ...state, refresh };
}
