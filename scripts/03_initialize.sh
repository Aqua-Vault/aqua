#!/usr/bin/env bash
set -euo pipefail

# Initialize the deployed mock pool and Aqua vault using the IDs written by
# 02_deploy.sh. Reads .env.contract_id (POOL_ID, USDC_ID, VAULT_ID).
# Usage: 03_initialize.sh [IDENTITY] [NETWORK]

IDENTITY="${1:-aqua_admin}"
NETWORK="${2:-testnet}"

if [ ! -f ".env.contract_id" ]; then
  echo "Error: .env.contract_id not found. Run 'make deploy' first." >&2
  exit 1
fi

# shellcheck disable=SC1091
source .env.contract_id

: "${POOL_ID:?POOL_ID missing from .env.contract_id}"
: "${USDC_ID:?USDC_ID missing from .env.contract_id}"
: "${VAULT_ID:?VAULT_ID missing from .env.contract_id}"

ADMIN_ADDRESS=$(stellar keys address "${IDENTITY}")

# Annual yield rate in basis points (1000 = 10%) and draw interval in seconds.
RATE_BPS="${RATE_BPS:-1000}"
DRAW_INTERVAL="${DRAW_INTERVAL:-86400}"

echo "=== Initializing Aqua (${NETWORK}) ==="
echo "    Admin:     ${ADMIN_ADDRESS}"
echo "    Pool:      ${POOL_ID}"
echo "    USDC:      ${USDC_ID}"
echo "    Vault:     ${VAULT_ID}"
echo "    Rate:      ${RATE_BPS} bps"
echo "    Interval:  ${DRAW_INTERVAL}s"
echo

# 1. Initialize the mock pool (token + admin), then set its yield rate.
echo "==> Initializing mock pool..."
stellar contract invoke \
  --id "${POOL_ID}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}" \
  -- initialize \
  --token "${USDC_ID}" \
  --admin "${ADMIN_ADDRESS}"

echo "==> Setting pool rate to ${RATE_BPS} bps..."
stellar contract invoke \
  --id "${POOL_ID}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}" \
  -- set_rate --bps "${RATE_BPS}"

# 2. Initialize the vault, wiring in the USDC asset and yield pool.
echo "==> Initializing Aqua vault..."
stellar contract invoke \
  --id "${VAULT_ID}" \
  --source "${IDENTITY}" \
  --network "${NETWORK}" \
  -- initialize \
  --admin "${ADMIN_ADDRESS}" \
  --asset "${USDC_ID}" \
  --yield_pool "${POOL_ID}" \
  --draw_interval "${DRAW_INTERVAL}"

echo
echo "=== Initialization complete ==="
echo "Set these in frontend/.env.local:"
echo "  NEXT_PUBLIC_NETWORK=${NETWORK}"
echo "  NEXT_PUBLIC_VAULT_ID=${VAULT_ID}"
echo "  NEXT_PUBLIC_POOL_ID=${POOL_ID}"
echo "  NEXT_PUBLIC_USDC_ID=${USDC_ID}"
