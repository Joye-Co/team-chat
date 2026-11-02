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
# an AppIcon.icns there replaces assets/brand/AppIcon.icns. The DMG window uses
# assets/brand/dmg/background.tiff and fixed icon positions (scripts/dmg-layout.py).
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

skip_build=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) skip_build=1 ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

[ "$(uname -s)" = Darwin ] || die "build-dmg.sh runs on macOS only"
need jq codesign hdiutil python3 shasum plutil
cargo_env

NAME="$(product name)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
[ -n "$VERSION" ] || die "no version in [workspace.package] of Cargo.toml"
BUNDLE_VERSION="${VERSION%%-*}"
MACOS_MIN="${MACOS_MIN:-13.0}"
PACKAGE="${DESKTOP_PACKAGE:-desktop}"
BRAND="$ROOT/assets/brand"
[ -s "$BRAND/AppIcon.icns" ] && [ -s "$BRAND/dmg/background.tiff" ] || die "missing assets/brand files; run bun assets/brand/render.mjs"

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

log "adding the icon"
# assets/brand/AppIcon.icns is built from docs/design/brand/icon.svg by assets/brand/render.mjs.
cp "$BRAND/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"

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
mkdir -p "$STAGE/.background"
cp "$BRAND/dmg/background.tiff" "$STAGE/.background/background.tiff"
cp "$BRAND/AppIcon.icns" "$STAGE/.VolumeIcon.icns"
mkdir -p "$DIST"
DMG="$DIST/$NAME-$VERSION.dmg"
RW="$WORK/rw.dmg"
rm -f "$DMG" "$DMG.sha256"
ok=0
for attempt in 1 2 3; do
  # hdiutil sometimes fails with "Resource busy" when other builds run; retry.
  if hdiutil create -quiet -volname "$NAME" -srcfolder "$STAGE" -fs HFS+ -format UDRW -ov "$RW"; then
    ok=1
    break
  fi
  log "hdiutil create failed (attempt $attempt); retrying"
  sleep $((attempt * 2))
done
[ "$ok" = 1 ] || die "hdiutil create failed"

# Finder layout: written straight into .DS_Store (no Finder, no AppleScript, no GUI).
# The window is the 660 x 400 pt background; icon centers match assets/brand/src/dmg-background.html.
MNT="$WORK/mnt"
mkdir -p "$MNT"
hdiutil attach -quiet -nobrowse -noautoopen -readwrite -mountpoint "$MNT" "$RW"
detach_rw() { hdiutil detach -quiet "$MNT" 2>/dev/null || hdiutil detach -quiet -force "$MNT"; }
if ! python3 -I "$ROOT/scripts/dmg-layout.py" "$MNT" --volume-name "$NAME" \
  --background .background/background.tiff --window 660x400 --icon-size 128 --text-size 13 \
  --item "$NAME.app=180,186" --item "Applications=480,186"; then
  detach_rw
  die "dmg-layout.py failed"
fi
# The volume icon needs the custom-icon flag on the volume root; SetFile ships with Xcode tools.
if command -v SetFile >/dev/null 2>&1; then
  SetFile -a C "$MNT" || log "SetFile failed; the volume keeps the default icon"
else
  log "no SetFile; the volume keeps the default icon"
fi
sync
detach_rw
hdiutil convert -quiet "$RW" -format UDZO -imagekey zlib-level=9 -ov -o "$DMG"
hdiutil verify -quiet "$DMG"
write_sha256 "$DMG"

log "wrote $DMG"
printf '%s\n' "$DMG"
