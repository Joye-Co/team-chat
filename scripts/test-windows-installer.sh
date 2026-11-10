#!/usr/bin/env bash
# Checks an installer made by build-windows-installer.sh on this Windows machine: checksum,
# silent install for the current user, the installed files and their version resource, the
# registry entries (uninstall entry, URL scheme, app id), the Start menu shortcut and its
# AppUserModelID, a start of the installed app (its self-report, and one screen drawn by
# the release program), a reinstall over itself, the silent uninstall, and that nothing of
# the app is left. Like scripts/test-dmg.sh for the DMG, but it does install: run it on a
# CI runner or a test machine, not where the app is in use.
#
# Usage: scripts/test-windows-installer.sh [dist/<name>-<version>-setup.exe]
# Env: DAEMON=0            the installer was built without the machine daemon
#      EXPECT_SIGNED=1     the installer and the programs must carry a valid signature
#      SKIP_WINDOW=1       do not open a window (a session without a desktop)
#      Everything it writes besides the install is under <worktree>/.tmp/windows-installer/.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
# shellcheck source=scripts/windows/lib.sh
. "$(dirname "$0")/windows/lib.sh"
need jq

NAME="$(product name)"
SLUG="$(product slug)"
BUNDLE_ID="$(product bundleId)"
SCHEME="$(product urlScheme)"
VERSION="$(workspace_version)"
SETUP="${1:-$DIST/$NAME-$VERSION-setup.exe}"
[ -f "$SETUP" ] || die "no installer at $SETUP"
SETUP="$(cd "$(dirname "$SETUP")" && pwd)/$(basename "$SETUP")"
WORK="$ROOT/.tmp/windows-installer"
rm -rf "$WORK" && mkdir -p "$WORK"
DIR="$(default_install_dir)"
EXE="$DIR/$NAME.exe"
KEY="$(uninstall_key)"

if reg_key_exists "$KEY" || [ -e "$DIR" ]; then
  die "$NAME is installed on this machine already; this test installs and removes it"
fi

fail=0
check() {
  local what="$1"; shift
  if "$@" >"$WORK/check.log" 2>&1; then printf 'ok    %s\n' "$what"; else printf 'FAIL  %s\n' "$what"; sed 's/^/      /' "$WORK/check.log" | head -n 20; fail=1; fi
}
is() { [ "$1" = "$2" ] || { printf 'got "%s", want "%s"\n' "$1" "$2"; return 1; }; }
has() { case "$1" in *"$2"*) ;; *) printf 'got "%s", want it to hold "%s"\n' "$1" "$2"; return 1 ;; esac; }

check "sha256 file matches" bash -c "cd '$(dirname "$SETUP")' && sha256sum -c '$(basename "$SETUP").sha256'"
if [ "${EXPECT_SIGNED:-0}" = 1 ]; then
  check "installer signature" is "$(signature_status "$SETUP")" Valid
else
  printf 'note  the installer is %s\n' "$(signature_status "$SETUP")"
fi

cleanup() { silent_uninstall "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

check "silent install" silent_install "$SETUP"
check "app installed" test -f "$EXE"
check "update helper installed" test -f "$DIR/$SLUG-update.exe"
check "uninstaller installed" test -f "$DIR/uninstall.exe"
if [ "${DAEMON:-1}" = 1 ]; then check "machine daemon installed" test -f "$DIR/$SLUG-daemon.exe"; fi
printf 'install log:\n'; sed 's/^/      /' "$DIR/install.log" 2>/dev/null || true
check "install log: version" grep -qx "version $VERSION" <(tr -d '\r' <"$DIR/install.log")
# The runtime is there already (found) or the bootstrapper put it there (exit 0).
check "install log: WebView2 runtime" grep -Eq '^webview2 (found [0-9.]+|bootstrapper exit 0)$' <(tr -d '\r' <"$DIR/install.log")
check "install log: shortcut carries the app id" grep -qx "shortcut with app id $BUNDLE_ID" <(tr -d '\r' <"$DIR/install.log")

# The program: version and name resources, and no console window (GUI subsystem = 2).
check "version resource" is "$(product_version "$EXE")" "$VERSION"
check "product name resource" is "$(ARG1="$(cygpath -w "$EXE")" ps_out '(Get-Item -LiteralPath $env:ARG1).VersionInfo.ProductName')" "$NAME"
check "window program (no console)" is "$(ARG1="$(cygpath -w "$EXE")" ps_out \
  '$b = [IO.File]::ReadAllBytes($env:ARG1); $pe = [BitConverter]::ToInt32($b, 0x3C); [BitConverter]::ToUInt16($b, $pe + 0x5C)')" 2
check "icon resource" is "$(ARG1="$(cygpath -w "$EXE")" \
  ARG2='[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern uint ExtractIconExW(string file, int index, IntPtr[] large, IntPtr[] small, uint count);' \
  ps_out 'Add-Type -Namespace W -Name Shell -MemberDefinition $env:ARG2; [W.Shell]::ExtractIconExW($env:ARG1, -1, $null, $null, 0) -ge 1')" True
if [ "${EXPECT_SIGNED:-0}" = 1 ]; then
  for f in "$EXE" "$DIR/$SLUG-update.exe" "$DIR/uninstall.exe"; do
    check "signature of $(basename "$f")" is "$(signature_status "$f")" Valid
  done
fi

# Registry (all under HKEY_CURRENT_USER: the install needs no administrator).
WIN_DIR="$(cygpath -w "$DIR")"
check "uninstall entry: name" is "$(reg_value "$KEY" DisplayName)" "$NAME"
check "uninstall entry: version" is "$(reg_value "$KEY" DisplayVersion)" "$VERSION"
check "uninstall entry: location" is "$(reg_value "$KEY" InstallLocation)" "$WIN_DIR"
check "uninstall entry: uninstaller" has "$(reg_value "$KEY" UninstallString)" "$WIN_DIR\\uninstall.exe"
check "uninstall entry: quiet uninstaller" has "$(reg_value "$KEY" QuietUninstallString)" "/S"
check "URL scheme: protocol" is "$(reg_value "Software\\Classes\\$SCHEME" "")" "URL:$NAME"
check "URL scheme: opens the app with the link" is "$(reg_value "Software\\Classes\\$SCHEME\\shell\\open\\command" "")" "\"$WIN_DIR\\$NAME.exe\" \"%1\""
check "app id: display name" is "$(reg_value "Software\\Classes\\AppUserModelId\\$BUNDLE_ID" DisplayName)" "$NAME"

# Start menu shortcut: target and AppUserModelID (notifications need the id on it).
LNK="$(start_menu_shortcut)"
check "Start menu shortcut" test -f "$LNK"
check "shortcut target" is "$(ARG1="$(cygpath -w "$LNK")" ps_out '(New-Object -ComObject WScript.Shell).CreateShortcut($env:ARG1).TargetPath')" "$WIN_DIR\\$NAME.exe"
check "shortcut AppUserModelID" is "$(ARG1="$(cygpath -w "$(dirname "$LNK")")" ARG2="$(basename "$LNK")" ps_out \
  '(New-Object -ComObject Shell.Application).Namespace($env:ARG1).ParseName($env:ARG2).ExtendedProperty("System.AppUserModel.ID")')" "$BUNDLE_ID"

# The installed app starts: it reports itself, takes a link as its argument, and the
# release program draws a screen.
"$EXE" --selftest-installed --report "$(cygpath -m "$WORK/installed.json")" && RC=0 || RC=$?
check "app self-report" is "$RC" 0
check "app reports its version" is "$(jq -r .version "$WORK/installed.json" 2>/dev/null)" "$VERSION"
check "app finds its install folder" is "$(jq -r .install_dir "$WORK/installed.json" 2>/dev/null)" "$WIN_DIR"
if [ "${SKIP_WINDOW:-0}" != 1 ]; then
  mkdir -p "$WORK/shots"
  "$EXE" --screenshot "$(cygpath -m "$WORK/shots")" --scenes login --theme light >"$WORK/screenshot.log" 2>&1 && RC=0 || RC=$?
  check "app draws a window (login screen)" bash -c "[ '$RC' = 0 ] && ls '$WORK/shots'/*.png"
  [ "$RC" = 0 ] || tail -n 20 "$WORK/screenshot.log"
fi

# A second install over the first works and keeps the folder.
check "reinstall over itself" silent_install "$SETUP"
check "still installed" test -f "$EXE"

check "silent uninstall" silent_uninstall "$DIR"
trap - EXIT
check "nothing left" assert_uninstalled "$DIR"

if [ "$fail" = 0 ]; then log "installer checks passed: $SETUP"; else die "installer checks failed: $SETUP"; fi
