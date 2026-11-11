#!/usr/bin/env bash
# Builds the iPhone client for the simulator:
#   1. chat-ffi as a static library for the simulator, and for the host (the binding generator
#      reads the host library)
#   2. the Swift bindings (UniFFI)
#   3. the Xcode project from project.yml (XcodeGen: brew install xcodegen)
#   4. the app and its two extensions with xcodebuild
# Output: the .app, printed on the last line. Run it from anywhere.
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
APP="$OUT/derived/Build/Products/Debug-iphonesimulator/Mobile.app"

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
command -v sccache >/dev/null 2>&1 && export RUSTC_WRAPPER=sccache
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.tmp/target-ios}"
# The C parts (SQLite, ring) take their minimum iOS version from here.
export IPHONEOS_DEPLOYMENT_TARGET="$MIN_OS"

cd "$ROOT"
cargo build --locked -p chat-ffi --target "$TRIPLE"
cargo build --locked -p chat-ffi

rm -rf "$GEN"
mkdir -p "$GEN"
cargo run --locked -q -p chat-ffi --features cli --bin uniffi-bindgen -- \
  generate --library "$CARGO_TARGET_DIR/debug/libchat_ffi.dylib" --language swift --out-dir "$GEN"

# What project.yml cannot know: the product (product.json), the version and where the core is.
cat > "$OUT/Product.xcconfig" <<EOF
PRODUCT_DISPLAY_NAME = $NAME
APP_BUNDLE_ID = $BUNDLE_ID
APP_VERSION = $VERSION
IPHONEOS_DEPLOYMENT_TARGET = $MIN_OS
CORE_GEN_DIR = $GEN
CORE_LIB_DIR = $CARGO_TARGET_DIR/$TRIPLE/debug
EOF

command -v xcodegen >/dev/null 2>&1 || { echo "xcodegen is missing: brew install xcodegen" >&2; exit 1; }
xcodegen generate --quiet --spec "$ROOT/apps/ios/project.yml"
# Rust targets one architecture here, so build only that one.
xcodebuild -quiet -project "$ROOT/apps/ios/Mobile.xcodeproj" -scheme Mobile -configuration Debug \
  -sdk iphonesimulator -destination "generic/platform=iOS Simulator" -derivedDataPath "$OUT/derived" \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=NO build

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
