#!/usr/bin/env bash
# Runs a package.json script in each given directory that defines it; skips the others.
# CI uses it so a job keeps working while packages add their test and typecheck scripts.
# Fails when no directory ran the script: a wrong path or a renamed script is not a pass.
#
# Usage: scripts/ci-run.sh <script> <dir>...
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
case "${1:-}" in -h | --help) usage; exit 0 ;; esac
need jq bun

[ $# -ge 2 ] || die "usage: scripts/ci-run.sh <script> <dir>..."
script="$1"; shift
dirs="$*"
ran=0
for dir in "$@"; do
  if [ ! -f "$dir/package.json" ]; then
    log "$dir: no package.json, skipped"
    continue
  fi
  if jq -e --arg s "$script" '.scripts[$s] // empty' "$dir/package.json" >/dev/null; then
    log "$dir: bun run $script"
    (cd "$dir" && bun run "$script")
    ran=$((ran + 1))
  else
    log "$dir: no '$script' script, skipped"
  fi
done
[ "$ran" -gt 0 ] || die "no package in '$dirs' has a '$script' script: nothing ran"
log "ran '$script' in $ran package(s)"
