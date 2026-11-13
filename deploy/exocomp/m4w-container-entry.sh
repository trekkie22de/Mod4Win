#!/bin/sh
set -eu

export DISPLAY="${DISPLAY:-:99}"
export WINEDEBUG="${WINEDEBUG:--all}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml,winemenubuilder.exe,winedbg.exe=d}"
export XDG_RUNTIME_DIR="/tmp/xdg-runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 0700 "$XDG_RUNTIME_DIR"

rm -f /tmp/.X99-lock /tmp/.X11-unix/X99 2>/dev/null || true

Xvfb "$DISPLAY" -screen 0 800x600x24 -maxclients 2048 -nolisten tcp -ac >/dev/null 2>&1 &
XVFB_PID=$!

cleanup() {
    kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Warte kurz, bis der Xvfb-Socket bereitsteht
i=0
while [ ! -S "/tmp/.X11-unix/X${DISPLAY#:}" ] && [ "$i" -lt 50 ]; do
    sleep 0.05
    i=$((i + 1))
done

exec "$@"
