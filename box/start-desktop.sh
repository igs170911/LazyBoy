#!/bin/bash
# Boot the box desktop: one TigerVNC X server (framebuffer + VNC in a single
# event-driven process), a minimal XFCE (xfwm4 + xfdesktop + panel, no session
# manager), AT-SPI for element-level automation, noVNC, then Chromium.
# Never a kiosk, --app, or bare HTML shell: the human and the agent share a
# normal Linux desktop.
set -euo pipefail
export DISPLAY="${DISPLAY:-:1}"
# Traditional Chinese desktop + Taipei time; everything launched from here
# (panel, box-chrome, terminals) inherits these.
export LANG="${LANG:-zh_TW.UTF-8}"
export LC_ALL="${LC_ALL:-zh_TW.UTF-8}"
export LANGUAGE="${LANGUAGE:-zh_TW:zh:en}"
export TZ="${TZ:-Asia/Taipei}"
# AT-SPI: GTK apps and Chromium publish their accessibility trees so the
# agent can read element names/coordinates instead of guessing pixels.
export GTK_MODULES="${GTK_MODULES:-atk-bridge}"
export GTK_A11Y="${GTK_A11Y:-atspi}"
export GNOME_ACCESSIBILITY="${GNOME_ACCESSIBILITY:-1}"
export NO_AT_BRIDGE=0
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME=/home/box/.config
export XDG_CACHE_HOME=/tmp/box-cache
export XDG_RUNTIME_DIR=/tmp/box-runtime

ROOT=/tmp/lazyboy
SCREEN_GEOMETRY="${LAZYBOY_SCREEN_GEOMETRY:-1280x720}"
mkdir -p /workspace /home/box /tmp "$ROOT" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR" /tmp/.X11-unix
chmod 1777 /workspace
if test "$(id -u)" = 0; then chmod 1777 /tmp /tmp/.X11-unix; fi
chmod 700 "$XDG_RUNTIME_DIR"
rm -f "$ROOT/ready"

# XFCE configuration is re-seeded from the image on every boot so the desktop
# always looks the way the image defines it; only /home/box/chrome-profile
# (Chromium) is meant to carry state across boots.
hydrate_xfce() {
  local xml="$XDG_CONFIG_HOME/xfce4/xfconf/xfce-perchannel-xml"
  mkdir -p "$xml" "$XDG_CONFIG_HOME/xfce4/panel" "$XDG_CONFIG_HOME/glib-2.0/settings"
  local f
  for f in xfce4-panel.xml xfwm4.xml xfce4-desktop.xml; do
    cp -f "/usr/share/lazyboy/xfce-skel/$f" "$xml/$f"
  done
  cp -f /etc/xdg/xfce4/helpers.rc "$XDG_CONFIG_HOME/xfce4/helpers.rc"
}

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
  eval "$(dbus-launch --sh-syntax)"
fi
# Tools started later over `docker exec` (box-a11y, box-chrome) must join this
# bus to reach the AT-SPI registry that the desktop apps registered with.
printf 'DBUS_SESSION_BUS_ADDRESS=%s\n' "$DBUS_SESSION_BUS_ADDRESS" >"$ROOT/dbus.env"

display_number="${DISPLAY#:}"
display_number="${display_number%%.*}"
if [[ ! "$display_number" =~ ^[0-9]+$ ]]; then
  echo "Unsupported local display: $DISPLAY" >&2
  exit 1
fi
if ! xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
  lock_file="/tmp/.X${display_number}-lock"
  lock_pid=""
  if [[ -f "$lock_file" ]]; then
    lock_pid=$(tr -d '[:space:]' < "$lock_file")
  fi
  if [[ "$lock_pid" =~ ^[0-9]+$ ]] && kill -0 "$lock_pid" 2>/dev/null; then
    echo "Display lock is held by live process $lock_pid" >&2
    exit 1
  fi
  rm -f -- "$lock_file" "/tmp/.X11-unix/X${display_number}"
fi

# Only websockify (same container) talks to VNC; keep the passwordless port
# off the Docker bridge so sibling containers cannot attach.
# There is no password to brute force here. Failed local viewer handshakes
# must not lock out websockify; access is controlled by the host HTTP proxy.
Xtigervnc "$DISPLAY" -geometry "$SCREEN_GEOMETRY" -depth 24 \
  -rfbport 5900 -localhost -SecurityTypes None -UseBlacklist 0 -AlwaysShared -AcceptCutText -SendCutText \
  -IdleTimeout 0 -MaxIdleTime 0 -MaxDisconnectionTime 0 -MaxConnectionTime 0 -s 0 -dpms \
  -desktop LazyBoy -ac -noreset >/tmp/xvnc.log 2>&1 &
for _ in $(seq 1 30); do
  if xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done
if ! xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
  cat /tmp/xvnc.log >&2
  exit 1
fi

# Virtual desktops must stay visible while the human is away.
/usr/local/bin/keep-desktop-awake

# Accessibility bus + registry. toolkit-accessibility is what Chromium and GTK
# consult before exposing their trees over AT-SPI.
for c in /usr/libexec/at-spi-bus-launcher /usr/lib/at-spi2-core/at-spi-bus-launcher; do
  if [[ -x "$c" ]]; then
    "$c" --launch-immediately >/tmp/atspi-bus.log 2>&1 &
    break
  fi
done
for c in /usr/libexec/at-spi2-registryd /usr/lib/at-spi2-core/at-spi2-registryd; do
  if [[ -x "$c" ]]; then
    "$c" >/tmp/atspi-registry.log 2>&1 &
    break
  fi
done
HOME=/home/box gsettings set org.gnome.desktop.interface toolkit-accessibility true 2>/tmp/gsettings.log || true

hydrate_xfce
xfconfd --daemon >/tmp/xfconfd.log 2>&1 || true
sleep 0.2
xfwm4 --compositor=off --display="$DISPLAY" --sm-client-disable >/tmp/xfwm4.log 2>&1 &
sleep 0.3
xfdesktop --disable-wm-check --sm-client-disable >/tmp/xfdesktop.log 2>&1 &
xfce4-panel --disable-wm-check --sm-client-disable >/tmp/xfce4-panel.log 2>&1 &

# Xvnc keeps the VNC clipboard in sync with the X selections through vncconfig.
if command -v vncconfig >/dev/null 2>&1; then
  vncconfig -nowin -display "$DISPLAY" >/tmp/vncconfig.log 2>&1 &
fi
websockify --heartbeat=30 --web /usr/share/novnc 6080 127.0.0.1:5900 >/tmp/novnc.log 2>&1 &

# Chromium: --restore-last-session brings back the previous tabs, so only the
# very first boot of a profile gets the welcome page (otherwise it doubled up).
if [[ -d /home/box/chrome-profile/Default/Sessions ]]; then
  box-chrome >/tmp/lazyboy-browser.log 2>&1 &
else
  box-chrome /usr/share/lazyboy/welcome.html >/tmp/lazyboy-browser.log 2>&1 &
fi

echo "[lazyboy-box] desktop up display=$DISPLAY geometry=$SCREEN_GEOMETRY vnc=6080 xfce chrome" >/tmp/start-desktop.log
touch "$ROOT/ready"
trap 'node /usr/local/bin/stop-browser.mjs || python3 /usr/local/bin/stop-browser.py; exit 0' TERM INT
while true; do
  sleep 3600 &
  wait $!
done
