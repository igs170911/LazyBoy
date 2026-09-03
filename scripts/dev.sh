#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
docker compose up -d postgres
echo "waiting for postgres..."
for _ in $(seq 1 40); do
  if docker compose exec -T postgres pg_isready -U lazyboy >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done
if [[ -f "$root/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$root/.env"
  set +a
fi
export DATABASE_URL="${DATABASE_URL:-postgres://lazyboy:lazyboy@127.0.0.1:5434/lazyboy}"
export SANDBOX_SUPERVISOR_TOKEN="${SANDBOX_SUPERVISOR_TOKEN:-dev-token}"
export SANDBOX_SUPERVISOR_URL="${SANDBOX_SUPERVISOR_URL:-http://127.0.0.1:7092}"
export SANDBOX_PROVIDER="${SANDBOX_PROVIDER:-docker}"
export DATA_DIR="${DATA_DIR:-$root/data}"
export API_BIND="${API_BIND:-0.0.0.0:3101}"
export LAZYBOY_WEB_DIR="$root/apps/web"
mkdir -p "$DATA_DIR"
echo "start supervisor in another terminal: cargo run -p lazyboy-supervisor"
echo "then: cargo run -p lazyboy-api"
echo "listening on 0.0.0.0:3101"
