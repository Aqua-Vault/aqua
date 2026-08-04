// Network + contract configuration, sourced from environment variables.
// Fill these in .env.local after running scripts/deploy.sh

export const NETWORK = (process.env.NEXT_PUBLIC_NETWORK || "testnet") as
  | "testnet"
  | "mainnet";

export const RPC_URL =
  NETWORK === "mainnet"
    ? "https://mainnet.sorobanrpc.com"
    : "https://soroban-testnet.stellar.org";

export const NETWORK_PASSPHRASE =
  NETWORK === "mainnet"
    ? "Public Global Stellar Network ; September 2015"
    : "Test SDF Network ; September 2015";

export const VAULT_ID = process.env.NEXT_PUBLIC_VAULT_ID || "";
export const POOL_ID = process.env.NEXT_PUBLIC_POOL_ID || "";
export const USDC_ID = process.env.NEXT_PUBLIC_USDC_ID || "";

export const STELLAR_EXPERT_URL =
  process.env.NEXT_PUBLIC_STELLAR_EXPERT_URL ||
  `https://stellar.expert/explorer/${NETWORK}`;

// Stellar Asset Contracts use 7 decimals.
export const USDC_DECIMALS = 7;

// True only when every required contract address is configured.
export const IS_CONFIGURED = Boolean(VAULT_ID && POOL_ID && USDC_ID);

export function explorerTxUrl(hash: string): string {
  return `${STELLAR_EXPERT_URL}/tx/${hash}`;
}

export function explorerContractUrl(id: string): string {
  return `${STELLAR_EXPERT_URL}/contract/${id}`;
}
