#!/usr/bin/env bash
# Render docs/hero.html into both README banners
#
# The banner only frames a real capture (see scripts/capture-hero.mjs), so this
# step needs any headless Chromium: CHROME=/path/to/chrome, a browser on PATH,
# the macOS Brave/Chrome install, or a Playwright download.
#
# Each language gets its own frame copy and its own screenshot, so the English
# README never shows a Chinese screen and the Chinese README never shows English.
#
#   scripts/render-readme-hero.sh
#     -> docs/readme-hero.png, docs/readme-hero.zh-TW.png
#   HERO_LANG=en scripts/render-readme-hero.sh          # one language only
#   HERO_LANG=en OUT=/tmp/preview.png scripts/render-readme-hero.sh
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
html="$root/docs/hero.html"
size="${SIZE:-1280,760}"
# Deliberately not LANG, which is the system locale and already set.
langs="${HERO_LANG:-en zh-TW}"

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

for lang in $langs; do
  case "$lang" in
    zh-TW) name="readme-hero.zh-TW" ;;
    en) name="readme-hero" ;;
    *) echo "unknown HERO_LANG: $lang (expected: en, zh-TW)" >&2; exit 1 ;;
  esac
  out="${OUT:-$root/docs/$name.png}"
  args=(--headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2
        --window-size="$size" --screenshot="$out")
  # Root needs the sandbox off; nowhere else does.
  [ "$(id -u)" = "0" ] && args+=(--no-sandbox)
  if ! noise="$("$browser" "${args[@]}" "file://$html?lang=$lang" 2>&1)"; then
    echo "$noise" >&2
    exit 1
  fi
  python3 - "$out" <<'PY'
from pathlib import Path
import struct, sys
p = Path(sys.argv[1])
data = p.read_bytes()
w, h = struct.unpack(">II", data[16:24])
print(f"wrote {p} ({w}x{h}, {p.stat().st_size} bytes)")
PY
done
