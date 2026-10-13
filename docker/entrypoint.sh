#!/bin/sh
# Starts as root only to join the Docker socket's group (its GID differs per host, and under rootless Docker it is
# the host user's group), then runs LazyBoy as the lazyboy user.
set -eu
socket=/var/run/docker.sock
if [ ! -S "$socket" ]; then
  echo "找不到 Docker socket（$socket）。請用 compose.yaml 啟動，或加上 -v /var/run/docker.sock:/var/run/docker.sock。" >&2
  exit 1
fi
if [ "$(id -u)" != 0 ]; then exec "$@"; fi
# A volume first used by an older image, or files copied in by root, go back to the service's user.
if [ "$(stat -c %u /data)" != 1000 ]; then chown -R lazyboy:lazyboy /data; fi
# setpriv keeps root's environment; the Docker CLI keeps its settings under $HOME.
export HOME=/data
exec setpriv --reuid lazyboy --regid lazyboy --groups "1000,$(stat -c %g "$socket")" --inh-caps=-all -- "$@"
