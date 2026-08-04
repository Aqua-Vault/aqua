#!/usr/bin/env bash
set -euo pipefail

# Deploy the mock yield pool, a test USDC SAC, and the Aqua vault to Testnet.
# Writes all resulting IDs to .env.contract_id for the initialize step.
# Usage: 02_deploy.sh [IDENTITY] [NETWORK]

IDENTITY="${1:-aqua_admin}"
NETWORK="${2:-testnet}"

POOL_WASM="target/wasm32v1-none/release/mock_pool.wasm"
VAULT_WASM="target/wasm32v1-none/release/aqua_vault.wasm"

for w in "${POOL_WASM}" "${VAULT_WASM}"; do
  if [ ! -f "${w}" ]; then
    echo "Error: WASM not found at ${w}. Run 'make build' first." >&2
    exit 1
  fi
done

ADMIN_ADDRESS=$(stellar keys address "${IDENTITY}")

echo "=== Aqua Deployment (${NETWORK}) ==="
echo "Deployer: ${IDENTITY} (${ADMIN_ADDRESS})"
echo

# 1. Deploy the mock yield pool.
echo "==> Deploying mock pool..."
POOL_ID=$(stellar contract deploy \
  --wasm "${POOL_WASM}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}")
echo "    Mock pool: ${POOL_ID}"

# 2. Issue a test USDC Stellar Asset Contract whose admin is the pool,
#    so the pool can mint simulated interest to itself.
echo "==> Issuing test USDC SAC (admin = pool)..."
USDC_ID=$(stellar contract asset deploy \
  --asset "USDC:${POOL_ID}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}")
echo "    Test USDC: ${USDC_ID}"

# 3. Deploy the Aqua vault.
echo "==> Deploying Aqua vault..."
VAULT_ID=$(stellar contract deploy \
  --wasm "${VAULT_WASM}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}")
echo "    Vault:     ${VAULT_ID}"

# Persist IDs for the initialize step and the frontend.
{
  echo "POOL_ID=${POOL_ID}"
  echo "USDC_ID=${USDC_ID}"
  echo "VAULT_ID=${VAULT_ID}"
} | tee .env.contract_id

echo
echo "==> Deployment complete. IDs saved to .env.contract_id"
