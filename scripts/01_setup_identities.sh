#!/usr/bin/env bash
set -euo pipefail

# Create and fund the deployer identity on Stellar Testnet.
# Usage: 01_setup_identities.sh [IDENTITY] [NETWORK]

IDENTITY="${1:-aqua_admin}"
NETWORK="${2:-testnet}"

echo "==> Setting up identity '${IDENTITY}' on '${NETWORK}'..."

# Add network configuration if not already present.
if ! stellar network ls 2>/dev/null | grep -q "${NETWORK}"; then
  echo "==> Adding network config for ${NETWORK}..."
  stellar network add "${NETWORK}" \
    --rpc-url "https://soroban-testnet.stellar.org" \
    --network-passphrase "Test SDF Network ; September 2015"
fi

# Generate identity if it doesn't exist.
if ! stellar keys ls 2>/dev/null | grep -qx "${IDENTITY}"; then
  echo "==> Generating keypair for ${IDENTITY}..."
  stellar keys generate "${IDENTITY}" --global --network "${NETWORK}"
fi

# Fund the account via Friendbot (idempotent; ignore "already funded").
echo "==> Funding ${IDENTITY} via Friendbot..."
stellar keys fund "${IDENTITY}" --network "${NETWORK}" || true

PUBLIC_KEY=$(stellar keys address "${IDENTITY}")
echo "==> Identity ready. Address: ${PUBLIC_KEY}"
