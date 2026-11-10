#!/usr/bin/env bash
# Builds the iPhone client for the simulator without an Xcode project:
#   1. chat-ffi as a static library for the simulator, and for the host (the binding generator
#      reads the host library)
#   2. the Swift bindings (UniFFI)
#   3. the app bundle with swiftc
# Output: .tmp/ios/<name>.app, printed on the last line. Run it from anywhere.
#
#   apps/ios/build-sim.sh            build
#   apps/ios/build-sim.sh --run      build, install on the booted simulator and launch it signed
#                                    in to this worktree's sandbox (bun scripts/sandbox/up.ts)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/.tmp/ios"
GEN="$OUT/gen"
TRIPLE=aarch64-apple-ios-sim
MIN_OS=17.0

NAME="$(jq -r .name "$ROOT/product.json")"
BUNDLE_ID="$(jq -r .bundleId "$ROOT/product.json").mobile"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
APP="$OUT/$NAME.app"

. "$HOME/.cargo/env"
command -v sccache >/dev/null 2>&1 && export RUSTC_WRAPPER=sccache
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.tmp/target-ios}"
# The C parts (SQLite, ring) take their minimum iOS version from here.
export IPHONEOS_DEPLOYMENT_TARGET="$MIN_OS"

cd "$ROOT"
cargo build -p chat-ffi --target "$TRIPLE"
cargo build -p chat-ffi

rm -rf "$GEN" "$APP"
mkdir -p "$GEN" "$APP"
cargo run -q -p chat-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$CARGO_TARGET_DIR/debug/libchat_ffi.dylib" --language swift --out-dir "$GEN"

SDK="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator swiftc \
  -target "arm64-apple-ios$MIN_OS-simulator" -sdk "$SDK" \
  -parse-as-library -module-name Mobile \
  -I "$GEN" -Xcc "-fmodule-map-file=$GEN/chat_ffiFFI.modulemap" \
  "$ROOT"/apps/ios/Sources/*.swift "$GEN/chat_ffi.swift" \
  "$CARGO_TARGET_DIR/$TRIPLE/debug/libchat_ffi.a" \
  -framework SystemConfiguration -framework Security -framework CoreFoundation \
  -o "$APP/Mobile"

cat > "$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
<key>CFBundleExecutable</key><string>Mobile</string>
<key>CFBundleName</key><string>$NAME</string>
<key>CFBundleDisplayName</key><string>$NAME</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleDevelopmentRegion</key><string>zh-Hans</string>
<key>LSRequiresIPhoneOS</key><true/>
<key>MinimumOSVersion</key><string>$MIN_OS</string>
<key>UIDeviceFamily</key><array><integer>1</integer></array>
<key>CFBundleSupportedPlatforms</key><array><string>iPhoneSimulator</string></array>
<key>UILaunchScreen</key><dict/>
<key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationPortrait</string></array>
</dict></plist>
EOF
codesign --force --sign - "$APP" >/dev/null 2>&1

if [[ "${1:-}" == "--run" ]]; then
  SANDBOX="$ROOT/.tmp/sandbox"
  [[ -f "$SANDBOX/server-url" && -f "$SANDBOX/owner-token" ]] || { echo "no sandbox: run bun scripts/sandbox/up.ts first" >&2; exit 1; }
  xcrun simctl install booted "$APP"
  xcrun simctl terminate booted "$BUNDLE_ID" >/dev/null 2>&1 || true
  # The token goes to the app through the environment and is never printed.
  SIMCTL_CHILD_TC_SERVER_URL="$(cat "$SANDBOX/server-url")" SIMCTL_CHILD_TC_TOKEN="$(cat "$SANDBOX/owner-token")" \
    xcrun simctl launch booted "$BUNDLE_ID" >/dev/null
fi
echo "$APP"
