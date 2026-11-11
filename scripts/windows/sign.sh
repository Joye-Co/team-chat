#!/usr/bin/env bash
# Signs one Windows program (Authenticode, SHA-256) when a signing identity is configured;
# does nothing otherwise, so builds without the operator's certificate still work.
#
# Usage: scripts/windows/sign.sh <file>...
# Env (the certificate itself is never in this repository):
#   WINDOWS_SIGN_CMD          a command that signs the file given as its last argument (for a
#                             certificate kept in a cloud key store or on a token)
#   WINDOWS_SIGN_THUMBPRINT   SHA-1 thumbprint of a code-signing certificate in the current
#                             user's certificate store; signtool signs with it
#   WINDOWS_SIGN_TIMESTAMP_URL  RFC 3161 timestamp server (optional; releases set one so the
#                             signature outlives the certificate)
set -euo pipefail

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# signtool of the newest Windows SDK.
find_signtool() {
  if command -v signtool >/dev/null 2>&1; then command -v signtool; return; fi
  local kits
  kits="$(cygpath -u "${ProgramFiles_x86:-${PROGRAMFILES:-C:\\Program Files} (x86)}")/Windows Kits/10/bin"
  [ -d "$kits" ] || kits="/c/Program Files (x86)/Windows Kits/10/bin"
  find "$kits" -path '*/x64/signtool.exe' 2>/dev/null | sort -V | tail -n 1
}

[ $# -gt 0 ] || die "usage: sign.sh <file>..."
if [ -z "${WINDOWS_SIGN_CMD:-}" ] && [ -z "${WINDOWS_SIGN_THUMBPRINT:-}" ]; then
  exit 0
fi

for file in "$@"; do
  [ -f "$file" ] || die "no file at $file"
  native="$(cygpath -w "$file")"
  if [ -n "${WINDOWS_SIGN_CMD:-}" ]; then
    # shellcheck disable=SC2086
    $WINDOWS_SIGN_CMD "$native" >/dev/null || die "the signing command failed for $(basename "$file")"
  else
    tool="$(find_signtool)"
    [ -n "$tool" ] || die "no signtool.exe (Windows SDK)"
    args=(sign /fd sha256 /sha1 "$WINDOWS_SIGN_THUMBPRINT")
    if [ -n "${WINDOWS_SIGN_TIMESTAMP_URL:-}" ]; then args+=(/tr "$WINDOWS_SIGN_TIMESTAMP_URL" /td sha256); fi
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' "$tool" "${args[@]}" "$native" >/dev/null ||
      die "signtool failed for $(basename "$file")"
  fi
  status="$(FILE="$native" powershell.exe -NoProfile -NonInteractive -Command \
    '(Get-AuthenticodeSignature -LiteralPath $env:FILE).Status' | tr -d '\r')"
  [ "$status" = Valid ] || die "$(basename "$file") is signed but Windows says: $status"
done
