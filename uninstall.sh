#!/bin/bash
# Removes ClearVox. Pass --blackhole to also remove the BlackHole 2ch driver
# (other apps may use it, so it's kept by default).
set -euo pipefail

APP="/Applications/ClearVox.app"
BUNDLE_ID="com.clearvox.mac"
BLACKHOLE_DRIVER="/Library/Audio/Plug-Ins/HAL/BlackHole2ch.driver"

echo "Uninstalling ClearVox…"

# Quitting normally lets ClearVox hand the default mic back to your real microphone.
if pgrep -x ClearVox >/dev/null; then
    osascript -e 'tell application "ClearVox" to quit' >/dev/null 2>&1 || true
    sleep 2
    pkill -x ClearVox 2>/dev/null || true
fi

if [ -d "$APP" ]; then
    rm -rf "$APP" 2>/dev/null || sudo rm -rf "$APP"
    echo "✓ Removed $APP"
fi

defaults delete "$BUNDLE_ID" >/dev/null 2>&1 || true
rm -f "$HOME/Library/Preferences/$BUNDLE_ID.plist"
tccutil reset Microphone "$BUNDLE_ID" >/dev/null 2>&1 || true
echo "✓ Removed settings and microphone permission"

if [ "${1:-}" = "--blackhole" ] && [ -d "$BLACKHOLE_DRIVER" ]; then
    echo "Removing BlackHole (needs your password)…"
    sudo rm -rf "$BLACKHOLE_DRIVER"
    sudo killall coreaudiod 2>/dev/null || true
    echo "✓ Removed BlackHole 2ch"
fi

echo
echo "Done. If an app still shows \"BlackHole 2ch\" as its microphone, switch it back to your real mic."
