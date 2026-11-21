#!/bin/bash
# Builds ClearVox.app (ad-hoc signed) into ./build
#   ./build.sh               native architecture
#   UNIVERSAL=1 ./build.sh   Apple Silicon + Intel (used for releases)
set -euo pipefail
cd "$(dirname "$0")"

ARGS=(-c release)
[ "${UNIVERSAL:-0}" = "1" ] && ARGS+=(--arch arm64 --arch x86_64)

swift build "${ARGS[@]}"
BIN="$(swift build "${ARGS[@]}" --show-bin-path)/ClearVox"

APP=build/ClearVox.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/ClearVox"
cp Resources/Info.plist "$APP/Contents/Info.plist"
cp ../assets/ClearVox.icns "$APP/Contents/Resources/ClearVox.icns"
codesign --force --sign - "$APP"
echo "Built $APP"
