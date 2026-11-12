#!/usr/bin/env bash
# Runs a package.json script in each given directory that defines it; skips the others.
# CI uses it so a job keeps working while packages add their test and typecheck scripts.
#
# Usage: scripts/ci-run.sh <script> <dir>...
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need jq bun

[ $# -ge 2 ] || die "usage: scripts/ci-run.sh <script> <dir>..."
script="$1"; shift
ran=0
failed=0
for dir in "$@"; do
  [ -f "$dir/package.json" ] || continue
  if ! jq empty "$dir/package.json"; then
    log "$dir: invalid package.json"
    failed=$((failed + 1))
    continue
  fi
  if jq -e --arg s "$script" '.scripts[$s] // empty' "$dir/package.json" >/dev/null; then
    log "$dir: bun run $script"
    if [ "${GITHUB_ACTIONS:-}" = true ]; then printf '::group::%s: %s\n' "$dir" "$script"; fi
    # Finish the other packages too, so one failure does not hide the rest of the verdict.
    if (cd "$dir" && bun run "$script"); then
      result=passed
    else
      result=failed
      failed=$((failed + 1))
    fi
    if [ "${GITHUB_ACTIONS:-}" = true ]; then printf '::endgroup::\n'; fi
    log "$dir: $script $result"
    ran=$((ran + 1))
  else
    log "$dir: no '$script' script, skipped"
  fi
done
log "ran '$script' in $ran package(s)"
[ "$ran" -gt 0 ] || die "no package defines '$script'; check the script name and directories"
[ "$failed" -eq 0 ] || die "'$script' failed in $failed package(s)"
