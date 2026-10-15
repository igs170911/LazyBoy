#!/usr/bin/env bash
# Builds the bot computer images: lazyhermes-desktop:local, then the bot image ($LAZYHERMES_IMAGE) on top of it.
# With --server it also builds LazyBoy's own image ($LAZYBOY_SERVER_IMAGE) for running LazyBoy in Docker.
# Images are built for the Docker engine's own platform (linux/amd64 or linux/arm64), checked before anything
# else; PLATFORM=linux/arm64 picks another one. Debian packages come from the fastest mirror
# (scripts/debian-mirror.py; DEBIAN_MIRROR overrides). The mirror travels as a build secret, so a different one
# never invalidates Docker's cache. Other arguments (for example --build-arg ...) go to the bot image builds.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
server=false
if [ "${1:-}" = --server ]; then server=true; shift; fi
desktop=lazyhermes-desktop:local
bot=${LAZYHERMES_IMAGE:-lazyhermes-bot:local}
server_image=${LAZYBOY_SERVER_IMAGE:-lazyboy:local}

engine=$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}' 2>/dev/null) || {
  printf '無法連線到 Docker。請確認 Docker 已啟動，而且目前的使用者可以執行 docker。\n' >&2; exit 1; }
case "$engine" in linux/x86_64) engine=linux/amd64 ;; linux/aarch64) engine=linux/arm64 ;; esac
platform=${PLATFORM:-$engine}
case "$platform" in
  linux/amd64|linux/arm64) ;;
  *) printf '不支援的平台：%s。LazyBoy 的映像只支援 linux/amd64 與 linux/arm64。\n' "$platform" >&2; exit 1 ;;
esac
if [ "$platform" = "$engine" ]; then
  printf '建置平台：%s（這台 Docker 的原生平台）\n' "$platform"
else
  printf '建置平台：%s（這台 Docker 是 %s，需要 QEMU 模擬，會慢很多，而且建好的映像在這台執行也會很慢）\n' "$platform" "$engine"
fi

# Hermes at the commit LazyBoy is tested with (HERMES_VERSION).
hermes_commit=$(tr -d '\n' < HERMES_VERSION)
if ! test -d vendor/hermes-agent/.git; then
  mkdir -p vendor
  git clone https://github.com/NousResearch/hermes-agent.git vendor/hermes-agent
fi
if test "$(git -C vendor/hermes-agent rev-parse HEAD)" != "$hermes_commit"; then
  git -C vendor/hermes-agent fetch origin "$hermes_commit"
  git -C vendor/hermes-agent checkout --detach "$hermes_commit"
fi

# The mirror test is plain Python; without python3 on the host it runs in a throwaway container.
if command -v python3 >/dev/null 2>&1; then
  mirror=$(python3 scripts/debian-mirror.py)
else
  mirror=$(docker run --rm -i -e DEBIAN_MIRROR python:3.14.8-slim-trixie python - < scripts/debian-mirror.py)
fi
secret=$(mktemp)
trap 'rm -f "$secret"' EXIT
printf '%s' "$mirror" > "$secret"
docker build --platform "$platform" --secret id=debian_mirror,src="$secret" "$@" -t "$desktop" box
docker build --platform "$platform" --secret id=debian_mirror,src="$secret" --build-arg DESKTOP_IMAGE="$desktop" "$@" -t "$bot" .
if $server; then
  docker build --platform "$platform" -f docker/server.Dockerfile -t "$server_image" .
fi
