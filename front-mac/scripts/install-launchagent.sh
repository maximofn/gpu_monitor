#!/usr/bin/env bash
# Install / reinstall the LaunchAgent that starts "GPU Monitor.app" at login.
# Usage:
#   ./scripts/install-launchagent.sh           # install + load
#   ./scripts/install-launchagent.sh uninstall # unload + remove
set -euo pipefail

LABEL="com.maximofn.gpu-monitor-tray"
SRC="$(cd "$(dirname "$0")" && pwd)/${LABEL}.plist"
DST="$HOME/Library/LaunchAgents/${LABEL}.plist"
APP="$(cd "$(dirname "$0")/.." && pwd)/build/GPU Monitor.app"

uid="$(id -u)"
domain="gui/${uid}"
target="${domain}/${LABEL}"

cmd="${1:-install}"

case "$cmd" in
    install)
        if [[ ! -d "$APP" ]]; then
            echo "error: bundle not found at: $APP" >&2
            echo "       run ./scripts/build-app.sh first." >&2
            exit 1
        fi

        # If already loaded, bootout first so changes to the plist take effect.
        if launchctl print "$target" >/dev/null 2>&1; then
            echo "==> bootout existing $LABEL"
            launchctl bootout "$target" || true
        fi

        echo "==> install $DST"
        mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
        cp "$SRC" "$DST"

        echo "==> bootstrap $target"
        launchctl bootstrap "$domain" "$DST"
        launchctl enable "$target"
        launchctl kickstart -k "$target"

        echo
        echo "Loaded. The tray will autostart on login."
        echo "Logs: ~/Library/Logs/gpu-monitor-tray.{out,err}.log"
        ;;
    uninstall)
        if launchctl print "$target" >/dev/null 2>&1; then
            echo "==> bootout $target"
            launchctl bootout "$target" || true
        fi
        if [[ -f "$DST" ]]; then
            echo "==> remove $DST"
            rm -f "$DST"
        fi
        echo "Uninstalled."
        ;;
    *)
        echo "usage: $0 [install|uninstall]" >&2
        exit 2
        ;;
esac
