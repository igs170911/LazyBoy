#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
mkdir -p .run
# Keep the previous run's log (a restart must not erase the errors that led to it); start fresh past 20 MB.
if [ -f .run/server.log ] && [ "$(stat -c %s .run/server.log 2>/dev/null || echo 0)" -gt 20971520 ]; then mv .run/server.log .run/server.log.1; fi
printf '\n==== %s start ====\n' "$(date '+%F %T')" >> .run/server.log
nohup setsid ./scripts/start.sh >> .run/server.log 2>&1 < /dev/null &
uv run python scripts/control.py wait
