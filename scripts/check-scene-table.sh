#!/usr/bin/env bash
# Fails when docs/plans/windows-parity/interface-scenes.zh.md is not what
# scripts/windows-scene-table.ts writes from the two runs kept beside it
# (docs/plans/windows-parity/runs/) and the notes. The run id is the one the file names.
set -euo pipefail
cd "$(dirname "$0")/.."
dir=docs/plans/windows-parity
run="$(sed -n 's/.*\*\*run \([0-9][0-9]*\)\*\*.*/\1/p' "$dir/interface-scenes.zh.md" | head -1)"
[ -n "$run" ] || { echo "no run id in $dir/interface-scenes.zh.md" >&2; exit 1; }
bun scripts/windows-scene-table.ts "$dir/runs/mac.screenshots.json" "$dir/runs/windows.screenshots.json" "$run"
if ! git diff --exit-code -- "$dir/interface-scenes.zh.md"; then
  echo "interface-scenes.zh.md differs from what the runs and notes give: run scripts/check-scene-table.sh and commit the file" >&2
  exit 1
fi
