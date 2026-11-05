#!/usr/bin/env bash
# The desktop app's `pnpm dev`: build it, open it, and every time a Rust file
# under apps/desktop or crates/ is saved, build again and reopen the window.
# GPUI has no hot reload, so a change shows up in a fresh window a few seconds
# after the save (a failed build keeps the old window and prints the error).
#
#   scripts/dev-desktop.sh            fake data (no server, no sign-in)
#   scripts/dev-desktop.sh --real     the real app (sign in to a server)
#
# Ctrl-C stops the watcher and closes the window.
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

args=(--fake)
[[ "${1:-}" == "--real" ]] && args=()

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
need cargo
command -v sccache >/dev/null 2>&1 && export RUSTC_WRAPPER=sccache
# Each worktree keeps its own target folder (AGENTS.md); incremental builds
# stay on so a one-line change rebuilds in seconds.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.tmp/target}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
mkdir -p "$ROOT/.tmp"
LOG="$ROOT/.tmp/dev-desktop.log"
APP="$CARGO_TARGET_DIR/debug/desktop"
pid=""

stop_app() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  pid=""
}
trap 'stop_app; exit 0' INT TERM

# A fingerprint of every watched file's path and modification time.
fingerprint() {
  find "$ROOT/apps/desktop/src" "$ROOT/apps/desktop/Cargo.toml" "$ROOT/crates" \
    \( -name target -prune \) -o \( -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.json' \) -print0 \) \
    | xargs -0 stat -f '%m %N' | sort | shasum | cut -d' ' -f1
}

build_and_open() {
  log "building…"
  local started=$SECONDS
  if (cd "$ROOT/apps/desktop" && cargo build -q); then
    stop_app
    "$APP" "${args[@]}" >"$LOG" 2>&1 &
    pid=$!
    log "opened in $((SECONDS - started)) s (pid $pid, log $LOG)"
  else
    log "build failed; the old window stays. Fix and save again."
  fi
}

last="$(fingerprint)"
build_and_open
log "watching apps/desktop and crates/ (Ctrl-C to stop)"
while true; do
  sleep 1
  now="$(fingerprint)"
  if [[ "$now" != "$last" ]]; then
    last="$now"
    build_and_open
  fi
done
