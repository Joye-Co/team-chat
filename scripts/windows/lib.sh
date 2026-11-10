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
  if compgen -G "$(dirname "$dir")/.$(product name).old-*" >/dev/null; then printf 'left: an update backup\n' >&2; left=1; fi
  return "$left"
}
