#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
case "${1:-}" in
  --check-only) exec python3 scripts/doctor.py ;;
  ""|--with-routing) ;;
  *) printf '用法：scripts/bootstrap.sh [--check-only|--with-routing]\n' >&2; exit 2 ;;
esac
python3 scripts/doctor.py
hermes_commit=$(tr -d '\n' < HERMES_VERSION)
if ! test -d vendor/hermes-agent/.git; then
  mkdir -p vendor
  git clone https://github.com/NousResearch/hermes-agent.git vendor/hermes-agent
fi
if test "$(git -C vendor/hermes-agent rev-parse HEAD)" != "$hermes_commit"; then
  git -C vendor/hermes-agent fetch origin "$hermes_commit"
  git -C vendor/hermes-agent checkout --detach "$hermes_commit"
fi
uv sync --frozen
npm ci --prefix web
npm run build --prefix web
docker build -t lazyhermes-desktop:local box
docker build -t "${LAZYHERMES_IMAGE:-lazyhermes-bot:local}" .
if test "${1:-}" = --with-routing; then ./scripts/routing.sh start; fi
python3 scripts/doctor.py --ready
printf '安裝完成。執行 scripts/start.sh，再打開 http://localhost:8790\n'
