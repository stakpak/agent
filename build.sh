#!/bin/bash
# Build Agent! locally without an Apple Developer account.
# Requires only Xcode Command Line Tools (xcode-select --install).
#
# Usage:
#   ./build.sh              # Debug build
#   ./build.sh release      # Release build
#   ./build.sh clean        # Clean build folder
#
# The app lands in build/Debug/ or build/Release/.
# Note: without a developer account, the app is ad-hoc signed and
# won't have entitlements for sandboxing, iCloud, or push notifications.
# The Launch Agent and Launch Daemon helpers also won't register with
# launchd (SMAppService requires a valid team ID). Everything else works.

set -euo pipefail
cd "$(dirname "$0")"

CONFIG="${1:-Debug}"
BUILD_DIR="$(pwd)/build"

if [ "$CONFIG" = "clean" ]; then
    rm -rf "$BUILD_DIR"
    echo "Cleaned build folder."
    exit 0
fi

echo "Building Agent! ($CONFIG) — no Apple account required..."

xcodebuild \
    -project Agent.xcodeproj \
    -scheme "Agent!" \
    -configuration "$CONFIG" \
    -destination 'platform=macOS' \
    -derivedDataPath "$BUILD_DIR/DerivedData" \
    CODE_SIGN_IDENTITY="-" \
    CODE_SIGN_STYLE="Manual" \
    DEVELOPMENT_TEAM="" \
    PROVISIONING_PROFILE_SPECIFIER="" \
    CODE_SIGN_ENTITLEMENTS="" \
    ENABLE_APP_SANDBOX=NO \
    AD_HOC_CODE_SIGNING_ALLOWED=YES \
    build 2>&1 | tail -20

APP_PATH="$BUILD_DIR/DerivedData/Build/Products/$CONFIG/Agent!.app"

if [ -d "$APP_PATH" ]; then
    echo ""
    echo "BUILD SUCCEEDED"
    echo "App: $APP_PATH"
    echo ""
    echo "To run:  open \"$APP_PATH\""
    echo "To copy: cp -R \"$APP_PATH\" /Applications/"
else
    echo ""
    echo "BUILD FAILED — check output above for errors."
    exit 1
fi
