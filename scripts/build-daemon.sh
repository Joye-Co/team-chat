#!/usr/bin/env bash
# Release build of the machine daemon into dist/<slug>-daemon-<version>-<target> plus .sha256.
#
# Usage: scripts/build-daemon.sh [--skip-build]
# Env: CARGO_TARGET_DIR, DIST (default dist/), DAEMON_PACKAGE (default daemon)
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

skip_build=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) skip_build=1 ;;
    -h | --help) sed -n '2,5p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

need jq shasum
cargo_env

SLUG="$(product slug)"
VERSION="$(workspace_version)"
[ -n "$VERSION" ] || die "no version in [workspace.package] of Cargo.toml"
PACKAGE="${DAEMON_PACKAGE:-daemon}"
TARGET="$(rustc -vV | awk '/^host:/ { print $2 }')"
BIN="$(bin_name "$PACKAGE")"

if [ "$skip_build" = 0 ]; then
  log "building $PACKAGE $VERSION (release, $TARGET)"
  if [ "$(uname -s)" = Darwin ]; then export MACOSX_DEPLOYMENT_TARGET="${MACOS_MIN:-13.0}"; fi
  cargo build --release --locked -p "$PACKAGE" --bin "$BIN" --manifest-path "$ROOT/Cargo.toml"
fi
BIN_PATH="$CARGO_TARGET_DIR/release/$BIN"
[ -x "$BIN_PATH" ] || die "no binary at $BIN_PATH"

mkdir -p "$DIST"
OUT="$DIST/$SLUG-daemon-$VERSION-$TARGET"
rm -f "$OUT" "$OUT.sha256"
cp "$BIN_PATH" "$OUT"
chmod 755 "$OUT"
if [ "$(uname -s)" = Darwin ]; then
  codesign --force --sign - --identifier "$(product bundleId).daemon" "$OUT"
fi
write_sha256 "$OUT"

log "wrote $OUT"
printf '%s\n' "$OUT"
