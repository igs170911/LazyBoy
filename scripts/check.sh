#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
uv run pytest -q
npm run build --prefix web
if test "${1:-}" = --native; then
  uv run python tests/native_continuation_flow.py
  uv run python tests/native_media_flow.py
  uv run python tests/native_protocols_flow.py
  uv run python tests/native_routing_flow.py
fi
