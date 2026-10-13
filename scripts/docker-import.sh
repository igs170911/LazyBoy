#!/usr/bin/env bash
# Copies the data of a LazyBoy installed with make install (.data, or $LAZYHERMES_DATA_DIR) into the Docker
# volume, so `make up` continues with the same bots, conversations and settings. The original folder is only read.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
source=$(cd "${LAZYHERMES_DATA_DIR:-.data}" 2>/dev/null && pwd) || { printf '找不到資料目錄 %s。\n' "${LAZYHERMES_DATA_DIR:-.data}" >&2; exit 1; }
volume=${LAZYBOY_VOLUME:-lazyboy-data}
container=${LAZYBOY_CONTAINER:-lazyboy}
image=${LAZYBOY_SERVER_IMAGE:-lazyboy:local}
fail() { printf '%s\n' "$1" >&2; exit 1; }

[ -f "$source/settings.json" ] || fail "$source 不是 LazyBoy 的資料目錄（沒有 settings.json）。"
if command -v flock >/dev/null 2>&1 && [ -f "$source/service.lock" ] && ! flock -n "$source/service.lock" true; then
  fail '原本的 LazyBoy 還在執行，請先執行 make stop。'
fi
[ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" = true ] && fail 'Docker 中的 LazyBoy 正在執行，請先執行 make down。'
docker image inspect "$image" >/dev/null 2>&1 || fail "找不到映像 $image，請先執行 make build。"
docker volume create "$volume" >/dev/null
if docker run --rm -v "$volume":/to --entrypoint test "$image" -e /to/settings.json; then
  fail "Docker volume $volume 已經有 LazyBoy 的資料，為了不覆蓋，停止匯入。要重新匯入請先刪除它：docker volume rm $volume"
fi
# Computers made by the host install mount host folders; stopped now, they are rebuilt on the volume when used.
ids=$(docker ps -q --filter label=lazyhermes.managed=true --filter label=lazyhermes.storage=host)
legacy=$(docker ps -q --filter label=lazyhermes.managed=true | grep -vxF -f <(docker ps -q --filter label=lazyhermes.storage) || true)
if [ -n "$ids$legacy" ]; then docker stop -t 20 $ids $legacy >/dev/null; fi
printf '正在把 %s 複製到 Docker volume %s…\n' "$source" "$volume"
docker run --rm --user 0:0 -v "$source":/from:ro -v "$volume":/to --entrypoint sh "$image" -c \
  'cp -a /from/. /to/ && rm -rf /to/ipc /to/service.lock && chown -R 1000:1000 /to'
printf '匯入完成。執行 make up 啟動；原本的 %s 沒有改動，確認一切正常後可以自行刪除。\n' "$source"
