#!/usr/bin/env bash
# Builds the desktop app in release mode, wraps it in <name>.app, ad-hoc signs it and makes
# dist/<name>-<version>.dmg plus its .sha256.
#
# Usage: scripts/build-dmg.sh [--skip-build]
#   --skip-build   reuse the release binary already in CARGO_TARGET_DIR
# Env: CARGO_TARGET_DIR, DIST (default dist/), MACOS_MIN (default 13.0),
#      DESKTOP_PACKAGE (default desktop)
#
# Files in apps/desktop/resources/ (if it exists) are copied into Contents/Resources;
# an AppIcon.icns there replaces the generated icon.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

skip_build=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) skip_build=1 ;;
    -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

[ "$(uname -s)" = Darwin ] || die "build-dmg.sh runs on macOS only"
need jq codesign hdiutil sips iconutil python3 shasum plutil
cargo_env

NAME="$(product name)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
[ -n "$VERSION" ] || die "no version in [workspace.package] of Cargo.toml"
BUNDLE_VERSION="${VERSION%%-*}"
MACOS_MIN="${MACOS_MIN:-13.0}"
PACKAGE="${DESKTOP_PACKAGE:-desktop}"

xml() { sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' <<<"$1"; }

BIN="$(bin_name "$PACKAGE")"
if [ "$skip_build" = 0 ]; then
  log "building $PACKAGE $VERSION (release)"
  MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN" cargo build --release --locked -p "$PACKAGE" --bin "$BIN" --manifest-path "$ROOT/Cargo.toml"
fi
BIN_PATH="$CARGO_TARGET_DIR/release/$BIN"
[ -x "$BIN_PATH" ] || die "no binary at $BIN_PATH"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/build-dmg.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/stage"
APP="$STAGE/$NAME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

log "assembling $NAME.app"
cp "$BIN_PATH" "$APP/Contents/MacOS/$NAME"
chmod 755 "$APP/Contents/MacOS/$NAME"
printf 'APPL????' >"$APP/Contents/PkgInfo"

cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
  <key>CFBundleName</key><string>$(xml "$NAME")</string>
  <key>CFBundleDisplayName</key><string>$(xml "$NAME")</string>
  <key>CFBundleIdentifier</key><string>$(xml "$BUNDLE_ID")</string>
  <key>CFBundleExecutable</key><string>$(xml "$NAME")</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$(xml "$BUNDLE_VERSION")</string>
  <key>CFBundleVersion</key><string>$(xml "$BUNDLE_VERSION")</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>$(xml "$MACOS_MIN")</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.business</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>$(xml "$BUNDLE_ID")</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>CFBundleURLSchemes</key><array><string>$(xml "$SCHEME")</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST
plutil -lint -s "$APP/Contents/Info.plist" || die "Info.plist is not valid"

log "drawing the icon"
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
python3 -I "$ROOT/scripts/make-icon.py" "$WORK/icon-1024.png" 1024
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$WORK/icon-1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$WORK/icon-1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

if [ -d "$ROOT/apps/desktop/resources" ]; then
  log "copying apps/desktop/resources"
  cp -R "$ROOT/apps/desktop/resources/." "$APP/Contents/Resources/"
fi

log "ad-hoc signing"
xattr -cr "$APP"
codesign --force --deep --sign - --identifier "$BUNDLE_ID" "$APP"
codesign --verify --deep --strict "$APP"

log "making the DMG"
ln -s /Applications "$STAGE/Applications"
mkdir -p "$DIST"
DMG="$DIST/$NAME-$VERSION.dmg"
rm -f "$DMG" "$DMG.sha256"
ok=0
for attempt in 1 2 3; do
  # hdiutil sometimes fails with "Resource busy" when other builds run; retry.
  if hdiutil create -quiet -volname "$NAME" -srcfolder "$STAGE" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG"; then
    ok=1
    break
  fi
  log "hdiutil create failed (attempt $attempt); retrying"
  sleep $((attempt * 2))
done
[ "$ok" = 1 ] || die "hdiutil create failed"
hdiutil verify -quiet "$DMG"
write_sha256 "$DMG"

log "wrote $DMG"
printf '%s\n' "$DMG"
