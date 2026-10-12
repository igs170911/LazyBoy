#!/bin/bash
# The browser helper is already staged in this standalone repository.
set -euo pipefail
box_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
mkdir -p "$box_dir/playwright"
docker build -t "${1:-lazyhermes-desktop:local}" "$box_dir"
