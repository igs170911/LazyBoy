#!/usr/bin/env bash
# Prove Cua Driver can control the LazyBoy XFCE + Xvfb desktop.
# Host usage:  scripts/cua-smoke-test.sh --docker [--repeat 10]
# In-container: lazyboy-cua-smoke --repeat 10
set -euo pipefail

repeat=10
ready_timeout=120
image="${COMPUTER_IMAGE:-lazyboy/computer:local}"
name=""
docker_mode=0

usage() {
  echo "usage: $0 [--docker] [--repeat N] [--image NAME]" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --docker) docker_mode=1; shift ;;
    --repeat) [[ $# -ge 2 ]] || usage; repeat="$2"; shift 2 ;;
    --image) [[ $# -ge 2 ]] || usage; image="$2"; shift 2 ;;
    --ready-timeout) [[ $# -ge 2 ]] || usage; ready_timeout="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done

[[ "$repeat" =~ ^[1-9][0-9]*$ ]] || usage
[[ "$ready_timeout" =~ ^[1-9][0-9]*$ ]] || usage

if [[ "$docker_mode" -eq 0 ]] && [[ -x /usr/local/bin/lazyboy-cua-smoke ]] && [[ -S /tmp/lazyboy/cua.sock || -f /tmp/lazyboy/ready ]]; then
  exec /usr/local/bin/lazyboy-cua-smoke --repeat "$repeat" --ready-timeout "$ready_timeout"
fi

if [[ "$docker_mode" -eq 0 ]]; then
  echo "not inside a LazyBoy desktop; passing --docker to run the computer image" >&2
  docker_mode=1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required" >&2
  exit 1
fi

if ! docker image inspect "$image" >/dev/null 2>&1; then
  echo "building $image (first desktop image build is slow)" >&2
  root="$(cd "$(dirname "$0")/.." && pwd)"
  docker build -f "$root/image/computer/Dockerfile" -t "$image" "$root"
fi

cleanup() {
  if [[ -n "$name" ]]; then docker rm -f "$name" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT

name=$(docker run -d --shm-size=512m \
  -e DISPLAY=:1 -e LAZYBOY_COMPUTER_DRIVER=cua \
  -e LAZYBOY_CONTROL_TOKEN=cua-smoke-local-only \
  "$image")

echo "waiting for desktop + cua-driver in $name"
for _ in $(seq 1 "$ready_timeout"); do
  if docker exec -u 1000:1000 "$name" test -f /tmp/lazyboy/ready \
    && docker exec -u 1000:1000 "$name" test -S /tmp/lazyboy/cua.sock; then
    break
  fi
  if ! docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null | grep -q true; then
    echo "computer container exited" >&2
    docker logs "$name" >&2 || true
    exit 1
  fi
  sleep 1
done

if ! docker exec -u 1000:1000 "$name" test -f /tmp/lazyboy/ready; then
  echo "desktop did not become ready" >&2
  docker exec -u 1000:1000 "$name" sh -c 'ls -la /tmp/lazyboy; tail -n 80 /tmp/lazyboy/*.log 2>/dev/null' >&2 || true
  exit 1
fi

echo "running $repeat Cua smoke iterations"
set +e
docker exec -u 1000:1000 "$name" /usr/local/bin/lazyboy-cua-smoke --repeat "$repeat" --ready-timeout "$ready_timeout"
code=$?
if [[ "$code" -eq 0 ]]; then
  docker exec -u 1000:1000 "$name" python3 /usr/local/bin/lazyboy-cua-adapter-test --repeat "$repeat"
  code=$?
fi
if [[ "$code" -eq 0 ]]; then
  docker exec -u 1000:1000 "$name" python3 /usr/local/bin/lazyboy-cua-isolation-test
  code=$?
fi
if [[ "$code" -eq 0 ]]; then
  docker pause "$name" >/dev/null && docker unpause "$name" >/dev/null
  code=$?
fi
if [[ "$code" -eq 0 ]]; then
  docker exec -u 1000:1000 "$name" python3 /usr/local/bin/lazyboy-cua-adapter-test --check-persistence
  code=$?
fi
if [[ "$code" -eq 0 ]]; then
  # Chromium writes its cookie database on a ~30 s timer and does not flush on
  # SIGTERM, so restarting right away races that timer: the check below would
  # then measure flush timing instead of profile persistence. Wait until the
  # fixture cookie is on disk before restarting.
  echo "waiting for the browser to persist its cookie"
  for _ in $(seq 1 90); do
    if docker exec -u 1000:1000 "$name" sh -c \
      'grep -qa cuaSmoke "$HOME"/.browser-profiles/displays/*/Default/Cookies 2>/dev/null'; then
      break
    fi
    sleep 1
  done
  docker restart "$name" >/dev/null
  code=$?
fi
if [[ "$code" -eq 0 ]]; then
  echo "waiting for desktop after restart"
  for _ in $(seq 1 "$ready_timeout"); do
    if docker exec -u 1000:1000 "$name" test -f /tmp/lazyboy/ready \
      && docker exec -u 1000:1000 "$name" test -S /tmp/lazyboy/cua.sock; then
      break
    fi
    sleep 1
  done
  docker exec -u 1000:1000 "$name" python3 /usr/local/bin/lazyboy-cua-adapter-test --check-persistence
  code=$?
fi
set -e

out="${CUA_SMOKE_OUT:-/tmp/lazyboy-cua-smoke-last}"
mkdir -p "$out"
docker cp "$name:/tmp/lazyboy/cua-smoke-report/." "$out/" 2>/dev/null || true
docker exec -u 1000:1000 "$name" sh -c 'tail -n 80 /tmp/lazyboy/screen-1-cua.log 2>/dev/null || true' \
  >"$out/cua-driver.log" || true

echo "smoke artifacts copied to $out"
exit "$code"
