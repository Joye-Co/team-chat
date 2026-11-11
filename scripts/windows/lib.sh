# Helpers of the Windows installer tests. Source it after scripts/lib.sh; Git Bash only.
# shellcheck shell=bash

case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) ;; *) die "this script runs on Windows (Git Bash)" ;; esac

# ps_out <PowerShell command>: its output without carriage returns. Values travel in the
# environment (ARG1, ARG2, ...), never inside the command text.
ps_out() {
  powershell.exe -NoProfile -NonInteractive -Command "$1" | tr -d '\r'
}

# reg_value <key under HKCU> <value name, "" for the default>: the value, empty when missing.
reg_value() {
  ARG1="HKCU:\\$1" ARG2="${2:-(default)}" ps_out \
    'try { (Get-ItemProperty -LiteralPath $env:ARG1 -ErrorAction Stop).($env:ARG2) } catch { "" }'
}

# reg_key_exists <key under HKCU>
reg_key_exists() {
  [ "$(ARG1="HKCU:\\$1" ps_out 'Test-Path -LiteralPath $env:ARG1')" = True ]
}

# product_version <exe>: the ProductVersion of its version resource.
product_version() {
  ARG1="$(cygpath -w "$1")" ps_out '(Get-Item -LiteralPath $env:ARG1).VersionInfo.ProductVersion'
}

# signature_status <file>: Valid, NotSigned, HashMismatch, ...
signature_status() {
  ARG1="$(cygpath -w "$1")" ps_out '(Get-AuthenticodeSignature -LiteralPath $env:ARG1).Status'
}

# signer <file>: "<status>|<certificate subject>|<certificate thumbprint>" of its signature;
# the last two are empty for a file that is not signed.
signer() {
  ARG1="$(cygpath -w "$1")" ps_out \
    '$s = Get-AuthenticodeSignature -LiteralPath $env:ARG1; "$($s.Status)|$($s.SignerCertificate.Subject)|$($s.SignerCertificate.Thumbprint)"'
}

# same_publisher <previous signer> <new signer>: whether an app installed from the previous
# installer takes the new one as an update. The rule of apps/update-helper (signer_allows):
# a validly signed app takes only a valid signature of the same subject; an app whose
# certificate this machine does not trust takes only the same certificate; an unsigned app
# takes an unsigned or a validly signed installer.
same_publisher() {
  local old_status old_subject old_thumb new_status new_subject new_thumb
  IFS='|' read -r old_status old_subject old_thumb <<<"$1"
  IFS='|' read -r new_status new_subject new_thumb <<<"$2"
  case "$old_status" in
    NotSigned)
      case "$new_status" in NotSigned | Valid) return 0 ;; esac
      printf 'the new installer has a signature that does not verify (%s)\n' "$new_status"; return 1 ;;
    Valid)
      [ "$new_status" = Valid ] || { printf 'the previous release is signed (%s); the new installer is %s, so installed apps would refuse it\n' "$old_subject" "$new_status"; return 1; }
      [ "$new_subject" = "$old_subject" ] || { printf 'another publisher: "%s", before "%s"; installed apps would refuse the update\n' "$new_subject" "$old_subject"; return 1; } ;;
    *)
      [ -n "$old_thumb" ] && [ "$new_thumb" = "$old_thumb" ] || { printf 'the previous release is signed by a certificate this machine does not trust (%s); only the same certificate updates it\n' "$old_status"; return 1; } ;;
  esac
}

# pe_machine <exe>: the machine field of its PE header in hex (8664 = x64, aa64 = ARM64).
pe_machine() {
  ARG1="$(cygpath -w "$1")" ps_out \
    '$b = [IO.File]::ReadAllBytes($env:ARG1); $pe = [BitConverter]::ToInt32($b, 0x3C); "{0:x4}" -f [BitConverter]::ToUInt16($b, $pe + 4)'
}

# run_native <program> <args...>: runs a Windows program with its arguments untouched (Git
# Bash would turn /S into a path).
run_native() {
  MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' "$@"
}

# default_install_dir: where the installer puts the app (a Unix-style path).
default_install_dir() {
  printf '%s/Programs/%s' "$(cygpath -u "$LOCALAPPDATA")" "$(product name)"
}

start_menu_shortcut() {
  printf '%s/Microsoft/Windows/Start Menu/Programs/%s.lnk' "$(cygpath -u "$APPDATA")" "$(product name)"
}

uninstall_key() {
  printf 'Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\%s' "$(product bundleId)"
}

RUN_KEY='Software\Microsoft\Windows\CurrentVersion\Run'
STARTUP_APPROVED_KEY='Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'

# app_cache_dir: where the app keeps what it can make again (the WebView2 user data, the drawn
# diagrams); the name of scripts/build-windows-installer.sh (CACHE_DIR).
app_cache_dir() {
  printf '%s/team-chat' "$(cygpath -u "$LOCALAPPDATA")"
}

# app_data_dir: the user's messages, settings and logs, which an uninstall keeps.
app_data_dir() {
  printf '%s/%s' "$(cygpath -u "$APPDATA")" "$(product bundleId)"
}

# silent_install <setup.exe>: installs for this user into the default folder.
silent_install() {
  run_native "$1" /S
}

# silent_uninstall <install dir>: runs the uninstaller and waits until the folder is gone
# (the uninstaller continues from a copy of itself, so its first process returns early).
silent_uninstall() {
  local dir="$1" _
  [ -f "$dir/uninstall.exe" ] || return 0
  run_native "$dir/uninstall.exe" /S
  for _ in $(seq 1 120); do
    [ -e "$dir" ] || return 0
    sleep 0.5
  done
  return 1
}

# assert_uninstalled <install dir>: nothing of the app is left (the user's data aside).
assert_uninstalled() {
  local dir="$1" scheme id left=0
  scheme="$(product urlScheme)"
  id="$(product bundleId)"
  if [ -e "$dir" ]; then printf 'left: %s\n' "$dir" >&2; ls -la "$dir" >&2 || true; left=1; fi
  if [ -e "$(start_menu_shortcut)" ]; then printf 'left: the Start menu shortcut\n' >&2; left=1; fi
  if reg_key_exists "$(uninstall_key)"; then printf 'left: the uninstall entry\n' >&2; left=1; fi
  if reg_key_exists "Software\\Classes\\$scheme"; then printf 'left: the URL scheme\n' >&2; left=1; fi
  if reg_key_exists "Software\\Classes\\AppUserModelId\\$id"; then printf 'left: the app id entry\n' >&2; left=1; fi
  if [ -n "$(toast_activator_clsid)" ] && reg_key_exists "Software\\Classes\\CLSID\\{$(toast_activator_clsid)}"; then printf 'left: the notification activator\n' >&2; left=1; fi
  if compgen -G "$(dirname "$dir")/.$(product name).old-*" >/dev/null; then printf 'left: an update backup\n' >&2; left=1; fi
  if [ -n "$(reg_value "$RUN_KEY" "$id")" ]; then printf 'left: the sign-in start entry\n' >&2; left=1; fi
  if [ -n "$(reg_value "$STARTUP_APPROVED_KEY" "$id")" ]; then printf 'left: the startup-approved record\n' >&2; left=1; fi
  if [ -n "$(reg_value "${RUN_KEY}Once" "$(product name).update-recovery")" ]; then printf 'left: the update recovery\n' >&2; left=1; fi
  for cache in webview2 diagrams; do
    if [ -e "$(app_cache_dir)/$cache" ]; then printf 'left: the %s cache\n' "$cache" >&2; left=1; fi
  done
  return "$left"
}
