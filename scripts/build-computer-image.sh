#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
docker build -f image/computer/Dockerfile -t lazyboy/computer:local .
echo "built lazyboy/computer:local"
