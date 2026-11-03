#!/usr/bin/env bash
# Checks a DMG made by build-dmg.sh: checksum, mount (hidden from Finder), bundle layout,
# Info.plist, icon, signature, /Applications link; then detaches. Does not launch the app.
#
# Usage: scripts/test-dmg.sh [dist/<name>-<version>.dmg] [--same-as <other.dmg>]
#   --same-as   also check that the app in <other.dmg> (another build) has the same
#               designated requirement
# With the signing files of build-dmg.sh (SIGNING_DIR), the app, its daemon and the DMG
# must be signed by that certificate, and the app's designated requirement must be
# 'identifier "<id>" and certificate leaf = H"<sha1>"', which no build changes.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need jq hdiutil codesign plutil shasum

NAME="$(product name)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
DMG=""
OTHER=""
while [ $# -gt 0 ]; do
  case "$1" in
    --same-as) OTHER="${2:-}"; shift 2 ;;
    -*) die "unknown option: $1" ;;
    *) DMG="$1"; shift ;;
  esac
done
DMG="${DMG:-$DIST/$NAME-$VERSION.dmg}"
[ -z "$OTHER" ] || [ -f "$OTHER" ] || die "no DMG at $OTHER"
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
DAEMON="$APP/Contents/MacOS/$(product slug)-daemon"
if [ "$(signing_mode)" = cert ]; then
  FP="$(signing_cert_sha1)"
  APP_DR="identifier \"$BUNDLE_ID\" and certificate leaf = H\"$FP\""
  DAEMON_DR="identifier \"$BUNDLE_ID.daemon\" and certificate leaf = H\"$FP\""
  check "app requirement names the certificate" test "$(designated_requirement "$APP")" = "$APP_DR"
  check "daemon requirement names the certificate" test "$(designated_requirement "$DAEMON")" = "$DAEMON_DR"
  check "app satisfies the requirement" codesign --verify --deep --strict "-R=$APP_DR" "$APP"
  check "daemon satisfies the requirement" codesign --verify --strict "-R=$DAEMON_DR" "$DAEMON"
  check "DMG signature" codesign --verify --strict "$DMG"
  check "DMG signed by the certificate" test "$(designated_requirement "$DMG")" = "identifier \"$BUNDLE_ID.dmg\" and certificate leaf = H\"$FP\""
else
  check "ad-hoc signature" bash -c "codesign -dv '$APP' 2>&1 | grep -q 'Signature=adhoc'"
fi
if [ -n "$OTHER" ]; then
  MNT2="$(mktemp -d "${TMPDIR:-/tmp}/test-dmg.XXXXXX")"
  hdiutil attach -quiet -nobrowse -noautoopen -readonly -mountpoint "$MNT2" "$OTHER"
  dr1="$(designated_requirement "$APP")"
  dr2="$(designated_requirement "$MNT2/$NAME.app")"
  hdiutil detach -quiet "$MNT2" 2>/dev/null || hdiutil detach -quiet -force "$MNT2" 2>/dev/null || true
  rmdir "$MNT2" 2>/dev/null || true
  check "same designated requirement as $(basename "$OTHER")" bash -c "[ -n \"\$1\" ] && [ \"\$1\" = \"\$2\" ] && [[ \"\$1\" != cdhash* ]]" _ "$dr1" "$dr2"
  printf '      requirement: %s\n' "$dr1"
fi

if [ "$fail" = 0 ]; then log "DMG checks passed: $DMG"; else die "DMG checks failed: $DMG"; fi
