#!/usr/bin/env bash
# Starts LazyBoy in Docker (compose.yaml) and prints the address to open, with a one-time sign-in link.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
container=${LAZYBOY_CONTAINER:-lazyboy}
port=${LAZYBOY_PORT:-8790}
fail() { printf '%s\n' "$1" >&2; exit 1; }

api=$(docker version --format '{{.Server.APIVersion}}' 2>/dev/null) || fail '無法連線到 Docker。請確認 Docker 已啟動，而且目前的使用者可以執行 docker。'
docker compose version >/dev/null 2>&1 || fail '找不到 docker compose。請安裝 Docker Compose 外掛（docker-compose-plugin）。'
# Bot computers mount their folders from the data volume with volume-subpath: Docker Engine 26 (API 1.45).
major=${api%%.*} minor=${api#*.}
if [ "$major" -lt 1 ] || { [ "$major" -eq 1 ] && [ "${minor%%.*}" -lt 45 ]; }; then
  fail "Docker 版本太舊（API $api）。在 Docker 中執行 LazyBoy 需要 Docker Engine 26 以上，請先更新 Docker。"
fi
for image in "${LAZYBOY_SERVER_IMAGE:-lazyboy:local}" "${LAZYHERMES_IMAGE:-lazyhermes-bot:local}"; do
  docker image inspect "$image" >/dev/null 2>&1 || fail "找不到映像 $image，請先執行 make build。"
done
# The socket of the Docker this shell uses: /var/run/docker.sock, or the user's own under rootless Docker.
endpoint=$(docker context inspect --format '{{.Endpoints.docker.Host}}' 2>/dev/null || true)
endpoint=${DOCKER_HOST:-$endpoint}
case "$endpoint" in unix://*) export DOCKER_SOCKET=${DOCKER_SOCKET:-${endpoint#unix://}} ;; esac

# This host's IPv4 addresses, without Docker's and VMs' internal bridges; the HTTPS certificate covers them.
addresses=$(ip -4 -o addr show scope global 2>/dev/null | awk '$2 !~ /^(docker|br-|veth|virbr|cni|flannel)/ { sub(/\/.*/, "", $4); print $4 }' || true)
export LAZYHERMES_HOSTNAME=${LAZYHERMES_HOSTNAME:-$(hostname 2>/dev/null || true)}
export LAZYHERMES_PUBLIC_HOSTS=${LAZYHERMES_PUBLIC_HOSTS:-$(printf '%s\n' $addresses | paste -sd, -)}
https_port=${LAZYBOY_HTTPS_PORT:-8791}

docker compose up -d --remove-orphans
printf '等待 LazyBoy 啟動'
for _ in $(seq 1 60); do
  state=$(docker inspect --format '{{.State.Health.Status}}' "$container" 2>/dev/null || true)
  [ "$state" = healthy ] && break
  [ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" = true ] || { printf '\n'; docker logs --tail 30 "$container" >&2; fail 'LazyBoy 沒有啟動成功，上面是最後的日誌。'; }
  printf '.'; sleep 2
done
printf '\n'
[ "$state" = healthy ] || { docker logs --tail 30 "$container" >&2; fail 'LazyBoy 兩分鐘內沒有就緒，上面是最後的日誌。'; }
token=$(docker exec "$container" cat /data/access-token)
printf 'LazyBoy 已啟動。\n\n'
printf '  這台電腦：http://localhost:%s/#token=%s\n' "$port" "$token"
for address in $addresses; do
  printf '  其他裝置：https://%s:%s/#token=%s\n' "$address" "$https_port" "$token"
done
printf '\n網址裡已含登入碼，打開一次之後瀏覽器會記住。登入碼請勿外流；make docker-token 可以再顯示一次。\n'
printf '其他裝置第一次用 HTTPS 前，要先安裝並信任 LazyBoy 的憑證：在「設定 → 其他裝置與 App」掃描 QR code 就能完成。\n'
