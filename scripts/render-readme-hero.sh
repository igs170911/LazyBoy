#!/usr/bin/env bash
# Render docs/hero.html to docs/readme-hero.png
# Same idea as Rakazo's README banner: a designed HTML frame (logo + headline
# + product window), then a browser screenshot. Not an image-model generation,
# so every character on the screen stays exact.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
html="$root/docs/hero.html"
out="$root/docs/readme-hero.png"
brave="/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
if [[ ! -x "$brave" ]]; then
  echo "Need Brave at $brave (or edit this script to your Chromium)." >&2
  exit 1
fi
"$brave" \
  --headless=new \
  --disable-gpu \
  --hide-scrollbars \
  --force-device-scale-factor=2 \
  --window-size=1280,640 \
  --screenshot="$out" \
  "file://$html"
python3 - "$out" <<'PY'
from pathlib import Path
import struct, sys
p = Path(sys.argv[1])
data = p.read_bytes()
w, h = struct.unpack(">II", data[16:24])
print(f"wrote {p} ({w}x{h}, {p.stat().st_size} bytes)")
PY
