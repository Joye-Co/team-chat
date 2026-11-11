#!/usr/bin/env bash
# Builds the Windows release of the desktop app and packs dist/<name>-<version>-setup.exe
# (NSIS, scripts/windows/installer.nsi) plus its .sha256. Runs in Git Bash on Windows
# (x64, MSVC toolchain, NSIS): the Windows job of CI and of the Release workflow.
#
# Usage: scripts/build-windows-installer.sh [--skip-build]
#   --skip-build   reuse the release binaries already in CARGO_TARGET_DIR
# Env: CARGO_TARGET_DIR, DIST (default dist/),
#      DAEMON     1 (default) builds and ships the machine daemon; 0 leaves it out
#      WEBVIEW2   1 (default) ships Microsoft's WebView2 Evergreen bootstrapper, which the
#                 installer runs only when the runtime is missing; 0 leaves it out
#      WEBVIEW2_BOOTSTRAPPER  path of an already downloaded bootstrapper
#      WINDOWS_SIGN_THUMBPRINT, WINDOWS_SIGN_CMD, WINDOWS_SIGN_TIMESTAMP_URL: code signing,
#                 see scripts/windows/sign.sh. Without them nothing is signed and Windows
#                 shows the SmartScreen warning for the installer.
#      WINDOWS_BIN_DIR  folder with the built programs (default CARGO_TARGET_DIR/release);
#                 with --skip-build this lets the packing step run on another system
#      TC_SERVERS_JSON, TC_OPS_DIR  the server URLs the app offers at sign-in, as for the DMG
#                 (scripts/release.sh): { "staging": "<url>", "production": "<url>" } in
#                 TC_SERVERS_JSON, else deploy/servers.json of the ops checkout. The build
#                 embeds only these URLs, never the rest of an ops checkout. Without them the
#                 app asks for the server address at sign-in.
#      REQUIRE_SERVERS=1  stop when there are no server URLs (the Release workflow)
#
# The installer holds <name>.exe, <slug>-update.exe (apps/update-helper) and, with DAEMON=1,
# <slug>-daemon.exe. Names, the bundle id and the URL scheme come from product.json.
# With DAEMON=1 it also writes dist/<slug>-daemon-<version>-x86_64-pc-windows-msvc.exe and its
# .sha256: the same daemon alone, for machines without the app (scripts/build-daemon.sh makes
# the files of the other systems).
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

skip_build=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) skip_build=1 ;;
    -h | --help) sed -n '2,29p' "$0"; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

need jq curl
cargo_env

is_windows() { case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) return 0 ;; *) return 1 ;; esac; }
# native <path>: the path as programs of this system take it.
native() { if is_windows; then cygpath -w "$1"; else printf '%s' "$1"; fi; }

find_makensis() {
  if command -v makensis >/dev/null 2>&1; then command -v makensis; return; fi
  local p
  for p in "/c/Program Files (x86)/NSIS/makensis.exe" "/c/Program Files/NSIS/makensis.exe"; do
    if [ -x "$p" ]; then printf '%s' "$p"; return; fi
  done
}
MAKENSIS="$(find_makensis)"
[ -n "$MAKENSIS" ] || die "missing tool: makensis (NSIS 3; on Windows: choco install nsis)"

NAME="$(product name)"
SLUG="$(product slug)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
valid_release_version "$VERSION" || die "version '$VERSION' is not MAJOR.MINOR.PATCH: the installer and the app's updater take no other form"
# The folder under %LOCALAPPDATA% of the app's caches, which the uninstaller removes: the name
# in apps/desktop/src/ui/web_embed.rs (webview2) and src/diagrams/cache.rs (diagrams). It is an
# infra name and does not follow the product name.
CACHE_DIR="team-chat"
for f in apps/desktop/src/ui/web_embed.rs apps/desktop/src/diagrams/cache.rs; do
  grep -q "\.join(\"$CACHE_DIR\")" "$ROOT/$f" || die "$f no longer keeps its cache in %LOCALAPPDATA%/$CACHE_DIR: update CACHE_DIR here"
done
ICON="$ROOT/assets/brand/AppIcon.ico"
[ -s "$ICON" ] || die "missing assets/brand/AppIcon.ico; run bun assets/brand/ico.mjs"
WITH_DAEMON="${DAEMON:-1}"
WITH_WEBVIEW2="${WEBVIEW2:-1}"

if [ "$skip_build" = 0 ]; then
  is_windows || die "the release build runs on Windows (use --skip-build with WINDOWS_BIN_DIR elsewhere)"
  host="$(rustc -vV | awk '/^host:/ { print $2 }')"
  [ "$host" = x86_64-pc-windows-msvc ] || die "expected the x86_64-pc-windows-msvc toolchain, found $host"
  # As release_ops of scripts/release.sh: an ops folder that holds servers.json alone.
  ops="$ROOT/.tmp/release-ops"
  rm -rf "$ops" && mkdir -p "$ops/deploy"
  if servers_json >"$ops/deploy/servers.json" 2>/dev/null && jq -e '.production | strings | length > 0' "$ops/deploy/servers.json" >/dev/null; then
    log "embedding the server URLs"
  else
    [ "${REQUIRE_SERVERS:-0}" != 1 ] || die "the build needs the server URLs: TC_SERVERS_JSON, or TC_OPS_DIR with deploy/servers.json"
    log "no server URLs (TC_SERVERS_JSON or an ops checkout): this build asks for the server address at sign-in"
    printf '{}\n' >"$ops/deploy/servers.json"
  fi
  TC_OPS_DIR="$(native "$ops")"
  export TC_OPS_DIR
  packages=(-p desktop -p update-helper)
  if [ "$WITH_DAEMON" = 1 ]; then packages+=(-p daemon); fi
  log "building ${packages[*]} $VERSION (release)"
  cargo build --release --locked "${packages[@]}" --bins --manifest-path "$ROOT/Cargo.toml"
fi
BIN_DIR="${WINDOWS_BIN_DIR:-$CARGO_TARGET_DIR/release}"
APP_BIN="$BIN_DIR/$(bin_name desktop).exe"
HELPER_BIN="$BIN_DIR/$(bin_name update-helper).exe"
DAEMON_BIN="$BIN_DIR/$(bin_name daemon).exe"
[ -f "$APP_BIN" ] || die "no app at $APP_BIN"
[ -f "$HELPER_BIN" ] || die "no update helper at $HELPER_BIN"
if [ "$WITH_DAEMON" = 1 ]; then [ -f "$DAEMON_BIN" ] || die "no daemon at $DAEMON_BIN (DAEMON=0 leaves it out)"; fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/build-windows-installer.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/stage"
mkdir -p "$STAGE"

log "staging $NAME $VERSION"
cp "$APP_BIN" "$STAGE/$NAME.exe"
cp "$HELPER_BIN" "$STAGE/$SLUG-update.exe"
defines=()
if [ "$WITH_DAEMON" = 1 ]; then
  cp "$DAEMON_BIN" "$STAGE/$SLUG-daemon.exe"
  defines+=("-DHAS_DAEMON=1")
fi

# The WebView2 Evergreen bootstrapper: a small Microsoft program that downloads and installs
# the runtime. It is fetched at build time, not kept in the repository, and must carry a
# valid Microsoft signature.
if [ "$WITH_WEBVIEW2" = 1 ]; then
  BOOT="${WEBVIEW2_BOOTSTRAPPER:-}"
  if [ -z "$BOOT" ]; then
    BOOT="$CARGO_TARGET_DIR/webview2/MicrosoftEdgeWebview2Setup.exe"
    if [ ! -s "$BOOT" ]; then
      log "downloading the WebView2 bootstrapper"
      mkdir -p "$(dirname "$BOOT")"
      curl -fsSL --retry 3 -o "$BOOT.part" "https://go.microsoft.com/fwlink/p/?LinkId=2124703" ||
        die "cannot download the WebView2 bootstrapper (WEBVIEW2=0 leaves it out)"
      mv "$BOOT.part" "$BOOT"
    fi
  fi
  [ -s "$BOOT" ] || die "no WebView2 bootstrapper at $BOOT"
  if is_windows; then
    signer="$(FILE="$(native "$BOOT")" powershell.exe -NoProfile -NonInteractive -Command \
      '$s = Get-AuthenticodeSignature -LiteralPath $env:FILE; "$($s.Status)|$($s.SignerCertificate.Subject)"' | tr -d '\r')"
    case "$signer" in
      "Valid|"*"O=Microsoft Corporation"*) ;;
      *) die "the WebView2 bootstrapper is not signed by Microsoft ($signer)" ;;
    esac
  fi
  cp "$BOOT" "$WORK/MicrosoftEdgeWebview2Setup.exe"
  defines+=("-DWEBVIEW2=$(native "$WORK/MicrosoftEdgeWebview2Setup.exe")")
fi

SIGNED=0
if [ -n "${WINDOWS_SIGN_CMD:-}" ] || [ -n "${WINDOWS_SIGN_THUMBPRINT:-}" ]; then
  is_windows || die "code signing runs on Windows"
  SIGNED=1
  log "signing the programs"
  "$ROOT/scripts/windows/sign.sh" "$STAGE"/*.exe
  # The uninstaller is made inside makensis, which signs it through this command. The shell
  # goes by its short path: a command line that starts with a quote loses its outer quotes
  # in cmd.exe.
  shell="$(cygpath -ms "$BASH")"
  case "$shell" in *" "*) shell=bash ;; esac
  defines+=("-DSIGN_CMD=$shell \"$(cygpath -m "$ROOT/scripts/windows/sign.sh")\"")
else
  log "not signing (no WINDOWS_SIGN_THUMBPRINT or WINDOWS_SIGN_CMD): Windows will show the SmartScreen warning"
fi

mkdir -p "$DIST"
# Absolute: makensis reads a relative output path from the script's folder.
DIST="$(cd "$DIST" && pwd)"
OUT="$DIST/$NAME-$VERSION-setup.exe"
rm -f "$OUT" "$OUT.sha256"
log "packing $(basename "$OUT")"
# makensis reads the script as UTF-8; off Windows it also needs a UTF-8 locale.
if ! is_windows; then export LC_ALL=en_US.UTF-8; fi
if ! "$MAKENSIS" -V2 -INPUTCHARSET UTF8 \
  "-DNAME=$NAME" "-DSLUG=$SLUG" "-DAPP_ID=$BUNDLE_ID" "-DSCHEME=$SCHEME" \
  "-DVERSION=$VERSION" "-DVERSION4=$VERSION.0" "-DCACHE_DIR=$CACHE_DIR" \
  "-DSTAGE=$(native "$STAGE")" "-DICON=$(native "$ICON")" "-DOUT=$(native "$OUT")" \
  ${defines[@]+"${defines[@]}"} \
  "$(native "$ROOT/scripts/windows/installer.nsi")" >"$WORK/makensis.log" 2>&1; then
  tail -n 40 "$WORK/makensis.log" >&2
  die "makensis failed"
fi
[ -s "$OUT" ] || die "makensis wrote no installer"
if [ "$SIGNED" = 1 ]; then
  log "signing the installer"
  "$ROOT/scripts/windows/sign.sh" "$OUT"
fi

# sha256_file <file>: writes <file>.sha256 next to it. shasum is a Perl script that Git Bash
# may lack; sha256sum prints the same line.
sha256_file() {
  (
    cd "$(dirname "$1")"
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$(basename "$1")"; else shasum -a 256 "$(basename "$1")"; fi |
      sed 's/ \*/  /' >"$(basename "$1").sha256"
  )
}
sha256_file "$OUT"

# The daemon alone (signed when the programs are), named like the files of build-daemon.sh.
if [ "$WITH_DAEMON" = 1 ]; then
  DAEMON_OUT="$DIST/$SLUG-daemon-$VERSION-x86_64-pc-windows-msvc.exe"
  rm -f "$DAEMON_OUT" "$DAEMON_OUT.sha256"
  cp "$STAGE/$SLUG-daemon.exe" "$DAEMON_OUT"
  sha256_file "$DAEMON_OUT"
  log "wrote $DAEMON_OUT"
fi

log "wrote $OUT"
printf '%s\n' "$OUT"
