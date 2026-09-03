#!/usr/bin/env bash
# Boot a real Linux desktop in the computer container.
# Primary screen is DISPLAY :1. Extra Team bots get their own screens via lazyboy-screen.
# Do not switch this back to a kiosk, --app, or HTML landing page.
set -uo pipefail
export DISPLAY="${DISPLAY:-:1}"
export HOME="${HOME:-/home/lazyboy}"
export SHELL=/bin/zsh
export TERM="${TERM:-xterm-256color}"
export LANG="${LANG:-zh_TW.UTF-8}"
export LC_ALL="${LC_ALL:-zh_TW.UTF-8}"
export LANGUAGE="${LANGUAGE:-zh_TW:zh:en}"
mkdir -p "$HOME" /tmp/lazyboy /tmp/.X11-unix
export PATH="$HOME/.local/bin:/usr/local/bin:$PATH"
cd "$HOME"

if [[ -n "${LAZYBOY_CONTROL_TOKEN:-}" ]]; then
  lazyboy-controld >/tmp/lazyboy/control.log 2>&1 &
fi

if command -v dbus-launch >/dev/null 2>&1; then
  eval "$(dbus-launch --sh-syntax)"
fi

lazyboy-screen boot-primary || exit 1
pid=""
if [[ -f /tmp/lazyboy/xvfb-1.pid ]]; then
  pid="$(cat /tmp/lazyboy/xvfb-1.pid)"
fi
if [[ -z "$pid" ]]; then
  echo "Xvfb pid missing" >&2
  exit 1
fi
while kill -0 "$pid" 2>/dev/null; do
  sleep 2
done
echo "Xvfb exited" >&2
exit 1
