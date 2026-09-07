# LazyBoy — developer Makefile
# Wraps docker compose + cargo so the whole stack can be built and launched
# with a few targets. See `make help`.

.DEFAULT_GOAL := help

COMPOSE        ?= docker compose
COMPUTER_IMAGE ?= lazyboy/computer:local
WEB_DIR        ?= apps/web
DATA_DIR       ?= ./data

.PHONY: help env env-force \
        up logs ps health down purge \
        computer postgres postgres-down pg-collation \
        build build-api build-supervisor build-controld \
        fmt fmt-check clippy lint audit test clean \
        web \
        dev dev-supervisor dev-api

help: ## Show this help
	@echo "LazyBoy — make targets"
	@echo ""
	@echo "  Full stack (recommended, everything in Docker):"
	@echo "    make env            Create .env with generated tokens (idempotent)"
	@echo "    make up             Build all images + start postgres/computer/supervisor/api"
	@echo "    make health         Curl the API health endpoint"
	@echo "    make logs           Tail container logs"
	@echo "    make ps             Show container status"
	@echo "    make down           Stop + remove containers (keep postgres data)"
	@echo "    make purge          Stop + remove containers AND the postgres volume"
	@echo ""
	@echo "  Individual pieces:"
	@echo "    make computer       Build the heavy Debian desktop image (lazyboy/computer:local)"
	@echo "    make postgres       Start only postgres (127.0.0.1:5434) and wait for ready"
	@echo "    make postgres-down  Stop postgres"
	@echo "    make pg-collation   Repair a Postgres collation version mismatch (see docs)"
	@echo ""
	@echo "  Local dev (postgres in Docker, Rust services on the host):"
	@echo "    make dev            Prep .env + postgres + computer image, then print run steps"
	@echo "    make dev-supervisor Run lazyboy-supervisor on the host (terminal 1)"
	@echo "    make dev-api        Run lazyboy-api on the host (terminal 2)"
	@echo ""
	@echo "  Rust / web:"
	@echo "    make build          cargo build --release (whole workspace)"
	@echo "    make build-api      cargo build --release -p lazyboy-api"
	@echo "    make fmt            cargo fmt --all"
	@echo "    make fmt-check      Report files rustfmt would change (legacy drift exists)"
	@echo "    make clippy         cargo clippy (deny warnings, reads clippy.toml)"
	@echo "    make lint           The Rust gate: clippy with -D warnings"
	@echo "    make audit          cargo deny: RustSec advisories, licenses, sources"
	@echo "    make test           cargo test --workspace (DB tests need: make postgres)"
	@echo "    make web            Build the frontend in $(WEB_DIR) (needs node/npm)"
	@echo "    make clean          cargo clean"
	@echo ""
	@echo "  After 'make up': open http://127.0.0.1:3101 and sign in with LAZYBOY_APP_TOKEN."
	@echo "  Set XAI_API_KEY in .env for chat."

# --- Environment -----------------------------------------------------------

env: ## Create .env with independent random secrets (preserve existing .env)
	@python3 scripts/init-env.py

env-force: ## Refuse destructive key regeneration; existing vault keys must be preserved
	@echo "Refusing to overwrite .env: this can orphan saved passwords. See docs/security-and-harness-review.md for safe rotation."
	@exit 1

# --- Full Docker stack -----------------------------------------------------

up: env ## Build every image and start the whole stack in Docker
	@echo "building images + starting stack (first run is slow: builds desktop + rust + web)..."
	$(COMPOSE) up -d --build
	@echo ""
	@echo "stack launched. open http://127.0.0.1:3101 and sign in with LAZYBOY_APP_TOKEN."
	$(COMPOSE) ps

logs: ## Tail logs for all services
	$(COMPOSE) logs -f

ps: ## Show container status
	$(COMPOSE) ps

health: ## Check the API health endpoint
	@curl -fsS http://127.0.0.1:3101/api/health && echo "  <- ok"

down: ## Stop and remove containers (keeps postgres volume)
	$(COMPOSE) down

purge: ## Stop containers and delete the postgres data volume
	$(COMPOSE) down -v

# --- Individual pieces -----------------------------------------------------

computer: ## Build the Debian desktop image used to spawn bot computers
	docker build -f image/computer/Dockerfile -t $(COMPUTER_IMAGE) .

postgres: ## Start only postgres and wait until it is ready
	$(COMPOSE) -f docker-compose.yml -f docker-compose.dev.yml up -d postgres
	@echo "waiting for postgres (127.0.0.1:5434) to be ready..."
	@for i in $$(seq 1 40); do if $(COMPOSE) exec -T postgres pg_isready -U lazyboy >/dev/null 2>&1; then echo "postgres ready"; break; fi; sleep 0.5; done

postgres-down: ## Stop postgres
	$(COMPOSE) down postgres

# A pgvector image rebuilt on another glibc leaves every database recording the old
# collation version; Postgres then refuses CREATE DATABASE and `cargo test` hangs on
# PoolTimedOut. Stop the api first, then reindex + refresh each database in place.
pg-collation: ## Repair a Postgres collation version mismatch after an image update
	$(COMPOSE) exec -T postgres psql -X -v ON_ERROR_STOP=1 -U lazyboy -d template1 -c "REINDEX DATABASE template1;" -c "ALTER DATABASE template1 REFRESH COLLATION VERSION;"
	$(COMPOSE) exec -T postgres psql -X -v ON_ERROR_STOP=1 -U lazyboy -d postgres -c "REINDEX DATABASE postgres;" -c "ALTER DATABASE postgres REFRESH COLLATION VERSION;"
	$(COMPOSE) exec -T postgres psql -X -v ON_ERROR_STOP=1 -U lazyboy -d lazyboy -c "REINDEX DATABASE lazyboy;" -c "ALTER DATABASE lazyboy REFRESH COLLATION VERSION;"

# --- Rust / web ------------------------------------------------------------

build: ## Release build of the whole workspace
	cargo build --release

build-api: ## Release build of the API binary
	cargo build --release -p lazyboy-api

build-supervisor: ## Release build of the supervisor binary
	cargo build --release -p lazyboy-supervisor

build-controld: ## Release build of the controld binary
	cargo build --release -p lazyboy-controld

fmt: ## Format all Rust code
	cargo fmt --all

fmt-check: ## Check formatting without touching files
	cargo fmt --all --check

# Lint policy lives in [workspace.lints] in the root Cargo.toml; the thresholds
# (e.g. too-many-arguments-threshold) live in clippy.toml, which cargo-clippy
# only reads from the directory it is started in - keep running this at the root.
clippy: ## Run clippy over the workspace, denying warnings
	cargo clippy --workspace --all-targets -- -D warnings

lint: clippy ## The Rust quality gate used by CI

# cargo-deny is a separate CLI: cargo install --locked cargo-deny
audit: ## Supply-chain check (RustSec advisories, licenses, dependency sources)
	@command -v cargo-deny >/dev/null 2>&1 || { \
	  echo "cargo-deny is not installed: cargo install --locked cargo-deny"; exit 1; }
	@echo "(advisories need the RustSec advisory DB, cloned on first run)"
	cargo deny check

test: ## Run the test suite
	cargo test --workspace

web: ## Build the frontend (needs node/npm)
	cd $(WEB_DIR) && npm install && npm run build

clean: ## Remove cargo build artifacts
	cargo clean

# --- Local dev (postgres in Docker, Rust on the host) ----------------------

dev: env postgres computer ## Prep local dev, then print the two run commands
	@echo ""
	@echo "prep complete. run these in TWO terminals:"
	@echo "  terminal 1:  make dev-supervisor"
	@echo "  terminal 2:  make dev-api"
	@echo "then open http://127.0.0.1:3101"

dev-supervisor: ## Run lazyboy-supervisor on the host (terminal 1)
	@test -f .env || { echo "run 'make env' first"; exit 1; }
	@set -a; . ./.env; set +a; \
	 export DATA_DIR="$${DATA_DIR:-$$PWD/data}"; mkdir -p "$$DATA_DIR"; \
	 export SUPERVISOR_BIND="$${SUPERVISOR_BIND:-127.0.0.1:7091}"; \
	 export LAZYBOY_COMPUTER_IMAGE="$${LAZYBOY_COMPUTER_IMAGE:-$(COMPUTER_IMAGE)}"; \
	 echo "starting supervisor on $$SUPERVISOR_BIND (log: stderr)"; \
	 exec cargo run -p lazyboy-supervisor

dev-api: ## Run lazyboy-api on the host (terminal 2)
	@test -f .env || { echo "run 'make env' first"; exit 1; }
	@set -a; . ./.env; set +a; \
	 export DATABASE_URL="$${DATABASE_URL:-postgres://lazyboy:lazyboy@127.0.0.1:5434/lazyboy}"; \
	 export SANDBOX_PROVIDER="$${SANDBOX_PROVIDER:-docker}"; \
	 export SANDBOX_SUPERVISOR_URL="$${SANDBOX_SUPERVISOR_URL:-http://127.0.0.1:7091}"; \
	 export DATA_DIR="$${DATA_DIR:-$$PWD/data}"; \
	 export API_BIND="$${API_BIND:-0.0.0.0:3101}"; \
	 export LAZYBOY_WEB_DIR="$${LAZYBOY_WEB_DIR:-$$PWD/$(WEB_DIR)}"; \
	 mkdir -p "$$DATA_DIR"; \
	 export ORT_DYLIB_PATH="$${ORT_DYLIB_PATH:-$$(./scripts/fetch-onnxruntime.sh "$$DATA_DIR/onnxruntime")}"; \
	 echo "starting api on $$API_BIND (web dir $$LAZYBOY_WEB_DIR)"; \
	 exec cargo run -p lazyboy-api
