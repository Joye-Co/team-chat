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
for dir in "$@"; do
  [ -f "$dir/package.json" ] || continue
  if jq -e --arg s "$script" '.scripts[$s] // empty' "$dir/package.json" >/dev/null; then
    log "$dir: bun run $script"
    (cd "$dir" && bun run "$script")
    ran=$((ran + 1))
  else
    log "$dir: no '$script' script, skipped"
  fi
done
log "ran '$script' in $ran package(s)"
