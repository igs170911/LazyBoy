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
        computer postgres postgres-down \
        build build-api build-supervisor build-controld \
        fmt clippy test clean \
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
	@echo "    make clippy         cargo clippy (deny warnings)"
	@echo "    make test           cargo test --workspace"
	@echo "    make web            Build the frontend in $(WEB_DIR) (needs node/npm)"
	@echo "    make clean          cargo clean"
	@echo ""
	@echo "  After 'make up': open http://127.0.0.1:3101 and sign in with LAZYBOY_APP_TOKEN."
	@echo "  Set XAI_API_KEY in .env for chat."

# --- Environment -----------------------------------------------------------

env: ## Create .env from example with fresh random tokens (no-op if .env exists)
	@if [ -f .env ]; then \
	  echo ".env already exists — leaving it untouched (use 'make env-force' to regenerate tokens)."; \
	else \
	  cp .env.example .env; \
	  APP=$$(openssl rand -hex 32); SUP=$$(openssl rand -hex 32); \
	  sed "s|^LAZYBOY_APP_TOKEN=.*|LAZYBOY_APP_TOKEN=$$APP|" .env > .env.tmp; \
	  sed "s|^SANDBOX_SUPERVISOR_TOKEN=.*|SANDBOX_SUPERVISOR_TOKEN=$$SUP|" .env.tmp > .env.tmp2; \
	  mv .env.tmp2 .env; rm -f .env.tmp; \
	  echo "created .env with generated LAZYBOY_APP_TOKEN / SANDBOX_SUPERVISOR_TOKEN (64 hex chars)."; \
	  echo "next: set XAI_API_KEY in .env, then run 'make up'."; \
	fi

env-force: ## Regenerate .env (wipes XAI_API_KEY — you will set it again)
	@cp .env.example .env; \
	 APP=$$(openssl rand -hex 32); SUP=$$(openssl rand -hex 32); \
	 sed "s|^LAZYBOY_APP_TOKEN=.*|LAZYBOY_APP_TOKEN=$$APP|" .env > .env.tmp; \
	 sed "s|^SANDBOX_SUPERVISOR_TOKEN=.*|SANDBOX_SUPERVISOR_TOKEN=$$SUP|" .env.tmp > .env.tmp2; \
	 mv .env.tmp2 .env; rm -f .env.tmp; \
	 echo "regenerated .env tokens (XAI_API_KEY reset — set it again)."

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
	$(COMPOSE) up -d postgres
	@echo "waiting for postgres (127.0.0.1:5434) to be ready..."
	@for i in $$(seq 1 40); do if $(COMPOSE) exec -T postgres pg_isready -U lazyboy >/dev/null 2>&1; then echo "postgres ready"; break; fi; sleep 0.5; done

postgres-down: ## Stop postgres
	$(COMPOSE) down postgres

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

clippy: ## Run clippy, denying warnings
	cargo clippy --all-targets -- -D warnings

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
	 echo "starting api on $$API_BIND (web dir $$LAZYBOY_WEB_DIR)"; \
	 exec cargo run -p lazyboy-api
