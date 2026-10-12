#!/bin/bash
set -euo pipefail
# Keep Hermes and its child tools on the same persistent home after dropping UID.
export HOME=/home/box HERMES_REAL_HOME=/home/box XDG_CONFIG_HOME=/home/box/.config
mkdir -p /hermes /workspace
if test "$(id -u)" = 0 && test -n "${LAZYHERMES_UID:-}" && test "$LAZYHERMES_UID" != 0; then
  bot_uid=$LAZYHERMES_UID
  bot_gid=${LAZYHERMES_GID:-$LAZYHERMES_UID}
  if ! getent group "$bot_gid" >/dev/null; then groupadd --gid "$bot_gid" bot; fi
  if ! getent passwd "$bot_uid" >/dev/null; then
    useradd --uid "$bot_uid" --gid "$bot_gid" --home-dir /home/box --no-create-home --shell /bin/bash bot
  fi
  bot_user=$(getent passwd "$bot_uid" | cut -d: -f1)
  printf '%s ALL=(ALL) NOPASSWD:ALL\n' "$bot_user" > /etc/sudoers.d/lazyhermes-bot
  chmod 440 /etc/sudoers.d/lazyhermes-bot
  mkdir -p /tmp/.X11-unix
  chmod 1777 /tmp/.X11-unix
  chown -R "$bot_uid:$bot_gid" /hermes /home/box /workspace
  exec setpriv --reuid "$bot_uid" --regid "$bot_gid" --init-groups /usr/local/bin/start-bot.sh
fi
# /tmp survives `docker restart`; markers from the previous boot must not be
# mistaken for this desktop's readiness or session bus.
rm -f /tmp/lazyboy/ready /tmp/lazyboy/dbus.env
/usr/local/bin/start-desktop.sh &
desktop_pid=$!
for attempt in $(seq 1 90); do
  if test -f /tmp/lazyboy/ready; then break; fi
  if ! kill -0 "$desktop_pid" 2>/dev/null; then wait "$desktop_pid"; exit 1; fi
  sleep 0.5
done
if ! test -f /tmp/lazyboy/ready; then echo 'Desktop did not become ready' >&2; exit 1; fi
source /tmp/lazyboy/dbus.env
export DBUS_SESSION_BUS_ADDRESS
export PYTHONPATH=/opt/hermes
# tini only signals its direct child. Stay in front of both services so
# `docker stop` reaches start-desktop.sh, which checkpoints and closes Chromium.
python /opt/lazyhermes/hermes_host.py &
host_pid=$!
shutdown() {
  trap - TERM INT
  kill -TERM "$host_pid" "$desktop_pid" 2>/dev/null || true
  wait "$desktop_pid" || true
  wait "$host_pid" || true
  exit 0
}
trap shutdown TERM INT
status=0
wait "$host_pid" || status=$?
kill -TERM "$desktop_pid" 2>/dev/null || true
wait "$desktop_pid" || true
exit "$status"
