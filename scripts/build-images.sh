#!/usr/bin/env bash
# Builds the bot computer images: lazyhermes-desktop:local, then the bot image ($LAZYHERMES_IMAGE) on top of it.
# Debian packages come from the fastest mirror (scripts/debian-mirror.py; DEBIAN_MIRROR overrides). The mirror
# travels as a build secret, so a different one never invalidates Docker's cache. Extra arguments (for example
# --build-arg ...) go to both builds.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
desktop=lazyhermes-desktop:local
bot=${LAZYHERMES_IMAGE:-lazyhermes-bot:local}
mirror=$(python3 scripts/debian-mirror.py)
secret=$(mktemp)
trap 'rm -f "$secret"' EXIT
printf '%s' "$mirror" > "$secret"
docker build --secret id=debian_mirror,src="$secret" "$@" -t "$desktop" box
docker build --secret id=debian_mirror,src="$secret" --build-arg DESKTOP_IMAGE="$desktop" "$@" -t "$bot" .
