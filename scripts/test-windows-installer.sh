#!/usr/bin/env bash
# Checks an installer made by build-windows-installer.sh on this Windows machine: checksum,
# silent install for the current user, the installed files and their version resource, the
# registry entries (uninstall entry, URL scheme, app id), the Start menu shortcut and its
# AppUserModelID, a start of the installed app (its self-report, and one screen drawn by
# the release program), a reinstall over itself, the silent uninstall, and that nothing of
# the app is left. Like scripts/test-dmg.sh for the DMG, but it does install: run it on a
# CI runner or a test machine, not where the app is in use.
#
# Usage: scripts/test-windows-installer.sh [dist/<name>-<version>-setup.exe] [--same-as <setup.exe>]
#   --same-as   the installer of the previous release: an app installed from it must accept
#               the new one as an update, by the update helper's own rule (`--accepts`), as
#               scripts/test-dmg.sh --same-as proves for the DMG's designated requirement
# Env: DAEMON=0            the installer was built without the machine daemon
#      TC_SERVERS_JSON     the server URLs the build was given (build-windows-installer.sh):
#                          the installed app must carry exactly them. Unset: it must carry none
#      SKIP_SERVERS=1      do not check the server URLs (an installer built elsewhere)
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
SETUP=""
SAME_AS=""
while [ $# -gt 0 ]; do
  case "$1" in
    --same-as) SAME_AS="${2:-}"; shift 2 ;;
    --same-as=*) SAME_AS="${1#*=}"; shift ;;
    -*) die "unknown option: $1" ;;
    *) [ -z "$SETUP" ] || die "more than one installer given"; SETUP="$1"; shift ;;
  esac
done
SETUP="${SETUP:-$DIST/$NAME-$VERSION-setup.exe}"
[ -z "$SAME_AS" ] || [ -f "$SAME_AS" ] || die "no installer at $SAME_AS"
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

# Whether this user has app data from before the test (it then stays).
DATA_BEFORE=0
[ ! -e "$(app_data_dir)" ] || DATA_BEFORE=1

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
# The installed update helper says whether an app of the previous release would take this
# installer: the rule of a real update, asked before the release goes out.
if [ -n "$SAME_AS" ]; then
  check "an app of $(basename "$SAME_AS") accepts this installer" same_publisher "$DIR/$SLUG-update.exe" "$SAME_AS" "$SETUP"
fi
if [ "${DAEMON:-1}" = 1 ]; then check "machine daemon installed" test -f "$DIR/$SLUG-daemon.exe"; fi
printf 'install log:\n'; sed 's/^/      /' "$DIR/install.log" 2>/dev/null || true
# The installer refuses a Windows older than the one the docs promise (10 version 22H2).
check "the installer asks for Windows 10 22H2" grep -q '^!define MIN_BUILD 19045$' "$ROOT/scripts/windows/installer.nsi"
check "install log: version" grep -qx "version $VERSION" <(tr -d '\r' <"$DIR/install.log")
# The runtime is there already (found) or the bootstrapper put it there (exit 0).
check "install log: WebView2 runtime" grep -Eq '^webview2 (found [0-9.]+|bootstrapper exit 0)$' <(tr -d '\r' <"$DIR/install.log")
check "install log: shortcut carries the app id" grep -qx "shortcut with app id $BUNDLE_ID" <(tr -d '\r' <"$DIR/install.log")

# The program: version and name resources, and no console window (GUI subsystem = 2).
check "version resource" is "$(product_version "$EXE")" "$VERSION"
check "product name resource" is "$(ARG1="$(cygpath -w "$EXE")" ps_out '(Get-Item -LiteralPath $env:ARG1).VersionInfo.ProductName')" "$NAME"
check "window program (no console)" is "$(ARG1="$(cygpath -w "$EXE")" ps_out \
  '$b = [IO.File]::ReadAllBytes($env:ARG1); $pe = [BitConverter]::ToInt32($b, 0x3C); [BitConverter]::ToUInt16($b, $pe + 0x5C)')" 2
# 64-bit x86 programs, as the DMG test asks for the arm64 slice. Windows on ARM runs them
# under its x64 emulation; there is no ARM64 build.
for f in "$EXE" "$DIR/$SLUG-update.exe"; do
  check "x64 program: $(basename "$f")" is "$(pe_machine "$f")" 8664
done
if [ "${DAEMON:-1}" = 1 ]; then check "x64 program: $SLUG-daemon.exe" is "$(pe_machine "$DIR/$SLUG-daemon.exe")" 8664; fi
check "icon resource" is "$(ARG1="$(cygpath -w "$EXE")" \
  ARG2='[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern uint ExtractIconExW(string file, int index, IntPtr[] large, IntPtr[] small, uint count);' \
  ps_out 'Add-Type -Namespace W -Name Shell -MemberDefinition $env:ARG2; [W.Shell]::ExtractIconExW($env:ARG1, -1, $null, $null, 0) -ge 1')" True
if [ "${EXPECT_SIGNED:-0}" = 1 ]; then
  signed=("$EXE" "$DIR/$SLUG-update.exe" "$DIR/uninstall.exe")
  if [ "${DAEMON:-1}" = 1 ]; then signed+=("$DIR/$SLUG-daemon.exe"); fi
  for f in "${signed[@]}"; do
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

# A notification that is clicked after the app has exited: the app's toasts are protocol
# activated (their link is <scheme>://…), so Windows needs the URL scheme above and the
# AppUserModelID on the shortcut, and no COM activator. Opening a link of the scheme the way
# Windows does must start the installed app.
if [ "${SKIP_WINDOW:-0}" != 1 ]; then
  app_pids() { ARG1="$WIN_DIR\\$NAME.exe" ps_out '(Get-Process | Where-Object { $_.Path -ieq $env:ARG1 } | ForEach-Object { $_.Id }) -join " "'; }
  check "the app is not running before the link" is "$(app_pids)" ""
  ARG1="$SCHEME://group/installer-test/topic/installer-test" ps_out 'Start-Process $env:ARG1'
  started=""
  for _ in $(seq 1 60); do
    started="$(app_pids)"
    [ -z "$started" ] || break
    sleep 0.5
  done
  check "a $SCHEME:// link starts the installed app" test -n "$started"
  # Only the processes this step started, by the path of the install.
  ARG1="$WIN_DIR\\$NAME.exe" ps_out 'Get-Process | Where-Object { $_.Path -ieq $env:ARG1 } | Stop-Process -Force' >/dev/null 2>&1 || true
  for _ in $(seq 1 40); do [ -z "$(app_pids)" ] && break; sleep 0.5; done
  check "the app has stopped again" is "$(app_pids)" ""
fi

# The installed app starts: it reports itself, takes a link as its argument, and the
# release program draws a screen.
"$EXE" --selftest-installed --report "$(cygpath -m "$WORK/installed.json")" && RC=0 || RC=$?
check "app self-report" is "$RC" 0
check "app reports its version" is "$(jq -r .version "$WORK/installed.json" 2>/dev/null)" "$VERSION"
check "app finds its install folder" is "$(jq -r .install_dir "$WORK/installed.json" 2>/dev/null)" "$WIN_DIR"
# The server URLs of the sign-in screen are the ones the build was given, as in the DMG.
if [ "${SKIP_SERVERS:-0}" != 1 ]; then
  digest() { if [ -n "$1" ]; then printf '%s' "$1" | sha256sum | cut -d' ' -f1; else printf 'null'; fi; }
  for env_name in production staging; do
    want="$(digest "$(printf '%s' "${TC_SERVERS_JSON:-{\}}" | jq -r --arg e "$env_name" '.[$e] // empty')")"
    check "server URL of $env_name ($([ "$want" = null ] && echo none || echo embedded))" \
      is "$(jq -r --arg e "$env_name" '.servers[$e] // "null"' "$WORK/installed.json" 2>/dev/null)" "$want"
  done
fi
if [ "${SKIP_WINDOW:-0}" != 1 ]; then
  mkdir -p "$WORK/shots"
  "$EXE" --screenshot "$(cygpath -m "$WORK/shots")" --scenes login --theme light >"$WORK/screenshot.log" 2>&1 && RC=0 || RC=$?
  check "app draws a window (login screen)" bash -c "[ '$RC' = 0 ] && ls '$WORK/shots'/*.png"
  [ "$RC" = 0 ] || tail -n 20 "$WORK/screenshot.log"
fi

# A second install over the first works and keeps the folder.
check "reinstall over itself" silent_install "$SETUP"
check "still installed" test -f "$EXE"

# What a person who used the app has besides the install: 登录时打开 turned on (the Run value
# and Task Manager's record of it), the caches, and their data. The uninstall removes the
# first three and keeps the data, as a Mac keeps it when the app goes to the Trash.
ARG1="HKCU:\\$RUN_KEY" ARG2="$BUNDLE_ID" ARG3="\"$WIN_DIR\\$NAME.exe\"" ps_out \
  'New-ItemProperty -LiteralPath $env:ARG1 -Name $env:ARG2 -Value $env:ARG3 -PropertyType String -Force | Out-Null'
ARG1="HKCU:\\$STARTUP_APPROVED_KEY" ARG2="$BUNDLE_ID" ps_out \
  'if (-not (Test-Path -LiteralPath $env:ARG1)) { New-Item -Path $env:ARG1 -Force | Out-Null }; New-ItemProperty -LiteralPath $env:ARG1 -Name $env:ARG2 -Value ([byte[]](2,0,0,0,0,0,0,0,0,0,0,0)) -PropertyType Binary -Force | Out-Null'
mkdir -p "$(app_cache_dir)/webview2/strict/EBWebView" "$(app_cache_dir)/diagrams" "$(app_data_dir)/logs"
echo cache >"$(app_cache_dir)/webview2/strict/EBWebView/file"
echo cache >"$(app_cache_dir)/diagrams/one.png"
echo mine >"$(app_data_dir)/logs/uninstall-test.log"
check "seeded: the sign-in start entry" test -n "$(reg_value "$RUN_KEY" "$BUNDLE_ID")"
check "seeded: the startup-approved record" test -n "$(reg_value "$STARTUP_APPROVED_KEY" "$BUNDLE_ID")"

check "silent uninstall" silent_uninstall "$DIR"
trap - EXIT
check "nothing left" assert_uninstalled "$DIR"
check "the cache folder is gone" test ! -e "$(app_cache_dir)"
check "the user's data is kept" test -f "$(app_data_dir)/logs/uninstall-test.log"
# Only what this test made: the whole folder when it was not there before.
if [ "$DATA_BEFORE" = 1 ]; then rm -f "$(app_data_dir)/logs/uninstall-test.log"; else rm -rf "$(app_data_dir)"; fi

if [ "$fail" = 0 ]; then log "installer checks passed: $SETUP"; else die "installer checks failed: $SETUP"; fi
