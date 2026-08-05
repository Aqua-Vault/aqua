import { defineConfig } from "@playwright/test";
import { POOL_ID, USDC_ID, VAULT_ID } from "./e2e/fixtures";

const PORT = 3001;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `npm run dev -- --port ${PORT}`,
    port: PORT,
    reuseExistingServer: !process.env.CI,
    env: {
      NEXT_PUBLIC_NETWORK: "testnet",
      NEXT_PUBLIC_VAULT_ID: VAULT_ID,
      NEXT_PUBLIC_POOL_ID: POOL_ID,
      NEXT_PUBLIC_USDC_ID: USDC_ID,
    },
  },
});
