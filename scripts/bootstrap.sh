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
uv sync --frozen
npm ci --prefix web
npm run build --prefix web
./scripts/build-images.sh
if test "${1:-}" = --with-routing; then ./scripts/routing.sh start; fi
python3 scripts/doctor.py --ready
printf '安裝完成。執行 make start，再打開 http://localhost:8790\n'
