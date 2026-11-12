#!/usr/bin/env bash
# The daemon's tests against the real Claude Code and Codex CLIs of this machine, against a
# fake model (no login, no model call). They test programs we do not make, so CI does not run
# them: each one is `#[ignore = "real CLI: scripts/test-real-cli.sh"]`. Run this after a CLI
# update, or after a change to how the daemon starts a CLI, its approval hook or its sandbox.
#
#   scripts/test-real-cli.sh                  # every case whose CLI is on PATH
#   scripts/test-real-cli.sh --managed-hooks  # also the managed-settings case (needs sudo)
#
# --managed-hooks writes Claude Code's managed settings that allow only managed hooks
# (macOS: /Library/Application Support/ClaudeCode, Linux: /etc/claude-code) around the one
# test that needs them, and removes the file afterwards. Do not use it on a machine whose
# managed settings you need: an existing file is refused.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

managed=0
for a in "$@"; do
  case "$a" in
    --managed-hooks) managed=1 ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $a" >&2; exit 2 ;;
  esac
done

# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
if command -v sccache >/dev/null; then export RUSTC_WRAPPER=sccache; fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0

t() { cargo test --locked -p daemon "$@"; }
ran=0

if command -v claude >/dev/null; then
  echo "== Claude Code $(claude --version 2>/dev/null | head -1)"
  t --test real_cli -- --ignored
  t --test integration -- --ignored --exact the_real_cli_waits_for_a_person_at_a_gated_command
  ran=1
else
  echo "-- claude is not on PATH: its cases are left out" >&2
fi

if command -v codex >/dev/null; then
  echo "== Codex $(codex --version 2>/dev/null | head -1)"
  t --test runtimes -- --ignored
  ran=1
else
  echo "-- codex is not on PATH: its cases are left out" >&2
fi

if [ "$managed" = 1 ]; then
  command -v claude >/dev/null || { echo "--managed-hooks needs claude on PATH" >&2; exit 1; }
  case "$(uname -s)" in
    Darwin) dir="/Library/Application Support/ClaudeCode" ;;
    *) dir="/etc/claude-code" ;;
  esac
  file="$dir/managed-settings.json"
  [ -e "$file" ] && { echo "$file exists already; not touching it" >&2; exit 1; }
  sudo mkdir -p "$dir"
  trap 'sudo rm -f "$file"' EXIT
  echo '{ "allowManagedHooksOnly": true }' | sudo tee "$file" >/dev/null
  TC_MANAGED_HOOKS_ONLY=1 t --test integration -- --ignored --exact \
    a_cli_whose_hooks_are_managed_only_never_starts_a_gated_run --nocapture
  ran=1
fi

[ "$ran" = 1 ] || { echo "no real CLI on PATH: nothing ran" >&2; exit 1; }
echo "real CLI tests passed"
