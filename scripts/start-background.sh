#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
mkdir -p .run
nohup setsid ./scripts/start.sh > .run/server.log 2>&1 < /dev/null &
uv run python scripts/control.py wait
