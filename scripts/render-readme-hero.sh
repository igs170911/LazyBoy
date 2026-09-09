#!/usr/bin/env bash
# Render docs/hero.html to docs/readme-hero.png
#
# The banner only frames a real capture (see scripts/capture-hero.mjs), so this
# step needs any headless Chromium: CHROME=/path/to/chrome, a browser on PATH,
# the macOS Brave/Chrome install, or a Playwright download.
#
#   scripts/render-readme-hero.sh              # the group shot (docs/hero/room.png)
#   SHOT=agent scripts/render-readme-hero.sh   # the single-agent shot
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
html="$root/docs/hero.html"
out="${OUT:-$root/docs/readme-hero.png}"
shot="${SHOT:-room}"
size="${SIZE:-1280,760}"

browser=""
for candidate in "${CHROME:-}" \
  "$(command -v chromium 2>/dev/null || true)" \
  "$(command -v chromium-browser 2>/dev/null || true)" \
  "$(command -v google-chrome 2>/dev/null || true)" \
  "$(command -v brave-browser 2>/dev/null || true)" \
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser" \
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"; do
  if [ -n "$candidate" ] && [ -x "$candidate" ]; then browser="$candidate"; break; fi
done
if [ -z "$browser" ]; then
  for cached in "$HOME"/.cache/ms-playwright/chromium-*/chrome-*/chrome; do
    if [ -x "$cached" ]; then browser="$cached"; break; fi
  done
fi
if [ -z "$browser" ]; then
  echo "No Chromium found. Set CHROME=/path/to/chrome and try again." >&2
  exit 1
fi

args=(--headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2
      --window-size="$size" --screenshot="$out")
# Root needs the sandbox off; nowhere else does.
[ "$(id -u)" = "0" ] && args+=(--no-sandbox)
"$browser" "${args[@]}" "file://$html?shot=$shot"

python3 - "$out" <<'PY'
from pathlib import Path
import struct, sys
p = Path(sys.argv[1])
data = p.read_bytes()
w, h = struct.unpack(">II", data[16:24])
print(f"wrote {p} ({w}x{h}, {p.stat().st_size} bytes)")
PY
