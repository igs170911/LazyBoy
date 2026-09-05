#!/usr/bin/env bash
# Render docs/diagrams.html slides to docs/diagrams/*.png
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
html="$root/docs/diagrams.html"
out="$root/docs/diagrams"
brave="/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
mkdir -p "$out"
if [[ ! -x "$brave" ]]; then
  echo "Need Brave at $brave" >&2
  exit 1
fi
for p in map chat sleep look click teach schedule keys folders; do
  "$brave" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --force-device-scale-factor=2 \
    --window-size=1280,720 \
    --screenshot="$out/$p.png" \
    "file://$html?p=$p"
  echo "wrote $out/$p.png"
done
