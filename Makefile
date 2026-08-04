# ==============================================================================
# Aqua Soroban Smart Contract Automation
# ==============================================================================

NETWORK          ?= testnet
ADMIN_IDENTITY   ?= aqua_admin

.PHONY: all help setup build test test-vault test-pool deploy initialize clean \
        install-deps frontend-install frontend-dev frontend-build

all: build test deploy

help: ## Display available commands
	@echo "Aqua - No-Loss Prize-Linked Savings on Stellar"
	@echo ""
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?##/ { \
	    printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@echo ""
	@echo "Variables (override with VAR=value):"
	@echo "  NETWORK          testnet | mainnet   (default: testnet)"
	@echo "  ADMIN_IDENTITY   stellar CLI key name (default: aqua_admin)"

# ------------------------------------------------------------------------------
# Contract targets
# ------------------------------------------------------------------------------

setup: ## Create and fund deployer identity on Testnet
	@bash scripts/01_setup_identities.sh $(ADMIN_IDENTITY) $(NETWORK)

build: ## Build all contracts to optimized WASM
	@echo "==> Building Aqua vault..."
	@stellar contract build --package aqua_vault
	@echo "==> Building mock pool..."
	@stellar contract build --package mock_pool
	@echo "OK  Build complete"

test: ## Run all contract unit tests
	@echo "==> Running all contract tests..."
	@cargo test --workspace --features testutils
	@echo "OK  All tests passed"

test-vault: ## Run only vault contract tests
	@cargo test -p aqua_vault --features testutils

test-pool: ## Run only mock pool tests
	@cargo test -p mock_pool --features testutils

deploy: build ## Deploy + initialize all contracts on Stellar Testnet
	@bash scripts/02_deploy.sh $(ADMIN_IDENTITY) $(NETWORK)
	@bash scripts/03_initialize.sh $(ADMIN_IDENTITY) $(NETWORK)

initialize: ## Initialize deployed contracts (reads .env.contract_id)
	@bash scripts/03_initialize.sh $(ADMIN_IDENTITY) $(NETWORK)

clean: ## Remove build artifacts and cached contract IDs
	@echo "==> Cleaning..."
	@cargo clean
	@rm -f .env.contract_id
	@echo "OK  Clean complete"

install-deps: ## Install Stellar CLI
	@command -v stellar >/dev/null 2>&1 || \
	    (echo "Installing stellar CLI via cargo..." && \
	     cargo install --locked stellar-cli --features opt)
	@stellar --version
	@echo "OK  Dependencies installed"

# ------------------------------------------------------------------------------
# Frontend targets
# ------------------------------------------------------------------------------

frontend-install: ## Install frontend npm dependencies
	@cd frontend && npm install

frontend-dev: frontend-install ## Start Next.js dev server
	@cd frontend && npm run dev

frontend-build: frontend-install ## Build Next.js for production
	@cd frontend && npm run build
