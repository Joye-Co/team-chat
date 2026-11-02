#!/usr/bin/env bash
# Checks a DMG made by build-dmg.sh: checksum, mount (hidden from Finder), bundle layout,
# Info.plist, icon, signature, /Applications link; then detaches. Does not launch the app.
#
# Usage: scripts/test-dmg.sh [dist/<name>-<version>.dmg]
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need jq hdiutil codesign plutil shasum

NAME="$(product name)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
DMG="${1:-$DIST/$NAME-$VERSION.dmg}"
[ -f "$DMG" ] || die "no DMG at $DMG"

fail=0
check() {
  local what="$1"; shift
  if "$@" >/dev/null 2>&1; then printf 'ok    %s\n' "$what"; else printf 'FAIL  %s\n' "$what"; fail=1; fi
}

check "sha256 file matches" bash -c "cd '$(dirname "$DMG")' && shasum -a 256 -c '$(basename "$DMG").sha256'"
check "hdiutil verify" hdiutil verify -quiet "$DMG"

MNT="$(mktemp -d "${TMPDIR:-/tmp}/test-dmg.XXXXXX")"
cleanup() {
  hdiutil detach -quiet "$MNT" 2>/dev/null || hdiutil detach -quiet -force "$MNT" 2>/dev/null || true
  rmdir "$MNT" 2>/dev/null || true
}
trap cleanup EXIT
hdiutil attach -quiet -nobrowse -noautoopen -readonly -mountpoint "$MNT" "$DMG"

APP="$MNT/$NAME.app"
PLIST="$APP/Contents/Info.plist"
pl() { plutil -extract "$1" raw -o - "$PLIST"; }

check "app bundle present" test -d "$APP"
check "Applications link" test "$(readlink "$MNT/Applications")" = /Applications
check "only the app and the link at the top" test "$(find "$MNT" -mindepth 1 -maxdepth 1 ! -name '.*' | wc -l | tr -d ' ')" = 2
check "executable" test -x "$APP/Contents/MacOS/$NAME"
check "arm64 slice" bash -c "lipo -archs '$APP/Contents/MacOS/$NAME' | grep -qw arm64"
check "machine daemon" test -x "$APP/Contents/MacOS/$(product slug)-daemon"
check "icon" test -s "$APP/Contents/Resources/AppIcon.icns"
check "Info.plist lint" plutil -lint -s "$PLIST"
check "CFBundleIdentifier" test "$(pl CFBundleIdentifier)" = "$BUNDLE_ID"
check "CFBundleName" test "$(pl CFBundleName)" = "$NAME"
check "CFBundleDisplayName" test "$(pl CFBundleDisplayName)" = "$NAME"
check "CFBundleShortVersionString" test "$(pl CFBundleShortVersionString)" = "${VERSION%%-*}"
check "LSMinimumSystemVersion" test -n "$(pl LSMinimumSystemVersion)"
check "NSHighResolutionCapable" test "$(pl NSHighResolutionCapable)" = true
check "URL scheme" test "$(pl CFBundleURLTypes.0.CFBundleURLSchemes.0)" = "$SCHEME"
check "codesign --verify" codesign --verify --deep --strict "$APP"
if [ "${SIGN_IDENTITY:--}" = - ]; then
  check "ad-hoc signature" bash -c "codesign -dv '$APP' 2>&1 | grep -q 'Signature=adhoc'"
else
  check "stable designated requirement" bash -c "codesign -d -r- '$APP' 2>&1 | grep -q 'designated => identifier'"
fi

if [ "$fail" = 0 ]; then log "DMG checks passed: $DMG"; else die "DMG checks failed: $DMG"; fi
