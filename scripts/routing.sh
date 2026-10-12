#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
case "${1:-start}" in
  start) docker compose -p lazyhermes-routing -f "$project_dir/routing/compose.yaml" up --build -d ;;
  stop) docker compose -p lazyhermes-routing -f "$project_dir/routing/compose.yaml" stop ;;
  status) docker compose -p lazyhermes-routing -f "$project_dir/routing/compose.yaml" ps ;;
  *) printf '用法：scripts/routing.sh start|stop|status\n' >&2; exit 2 ;;
esac
