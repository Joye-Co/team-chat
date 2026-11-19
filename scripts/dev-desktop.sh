#!/usr/bin/env bash
# The desktop app's `pnpm dev`: build it, open it, and every time a Rust file
# under apps/desktop or crates/ is saved, build again and reopen the window.
# GPUI has no hot reload, so a change shows up in a fresh window a few seconds
# after the save (a failed build keeps the old window and prints the error).
#
#   scripts/dev-desktop.sh            the real app on this worktree's local sandbox: the
#                                     real server with the sample team (scripts/sandbox/),
#                                     signed in, nothing touches your own accounts
#   scripts/dev-desktop.sh --reset    the same, with the sample data made again
#   scripts/dev-desktop.sh --real     the real app (sign in to a server yourself)
#   scripts/dev-desktop.sh --fake     the in-process samples of the screenshot self-tests
#
# Ctrl-C stops the watcher, closes the window and stops the sandbox server (its data stays in
# .tmp/sandbox/ for the next start). Runs on macOS and Linux.
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mode=""
reset=()
for arg in "$@"; do
  case "$arg" in
    --real | --fake)
      [[ -z "$mode" || "$mode" == "${arg#--}" ]] || die "--real and --fake are two different apps: give one"
      mode="${arg#--}"
      ;;
    --reset) reset=(--reset) ;;
    -h | --help) usage; exit 0 ;;
    *) die "unknown argument: $arg (use --reset, --real or --fake)" ;;
  esac
done
[[ -z "$mode" || "${#reset[@]}" == 0 ]] || die "--reset makes the sandbox's sample data again: it does not go with --$mode"
mode="${mode:-sandbox}"
case "$mode" in
  sandbox) args=(--sandbox "$ROOT/.tmp/sandbox") ;;
  fake) args=(--fake) ;;
  real) args=() ;;
esac

# Each worktree keeps its own target folder (AGENTS.md); incremental builds
# stay on so a one-line change rebuilds in seconds.
own_target="${CARGO_TARGET_DIR:-}"
cargo_env
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
if [[ -z "$own_target" && -d "$ROOT/.tmp/target" ]]; then
  log "builds go to $CARGO_TARGET_DIR now; .tmp/target is from an older dev:desktop: delete it when no build uses it (rm -rf '$ROOT/.tmp/target')"
fi
warn_low_disk
if [[ "$mode" == sandbox ]]; then need bun; fi
mkdir -p "$ROOT/.tmp"
LOG="$ROOT/.tmp/dev-desktop.log"
APP="$CARGO_TARGET_DIR/debug/desktop"
pid=""

# stat prints "<modification time> <path>": BSD (macOS) and GNU (Linux) spell that differently.
if stat -f '%m %N' "$ROOT" >/dev/null 2>&1; then stat_args=(-f '%m %N'); else stat_args=(-c '%Y %n'); fi

stop_app() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  pid=""
}
stop_all() {
  stop_app
  [[ "$mode" == sandbox ]] && bun "$ROOT/scripts/sandbox/up.ts" --stop || true
}
# Every way out closes the window and stops the sandbox, not only Ctrl-C.
trap stop_all EXIT
trap 'exit 0' INT TERM

if [[ "$mode" == sandbox ]]; then
  bun "$ROOT/scripts/sandbox/up.ts" ${reset[@]+"${reset[@]}"} || die "the sandbox did not start"
fi

# A fingerprint of every watched file's path and modification time. A file that goes away
# between find and stat (a branch switch, a rename) is left out: the next tick sees the change.
fingerprint() {
  { find "$ROOT/apps/desktop/src" "$ROOT/apps/desktop/Cargo.toml" "$ROOT/crates" \
    \( -name target -prune \) -o \( -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.json' \) -print0 \) 2>/dev/null || true; } \
    | { xargs -0 stat "${stat_args[@]}" 2>/dev/null || true; } | sort | shasum | cut -d' ' -f1
}

show_log() { tail -n 20 "$LOG" >&2 || true; }

build_and_open() {
  log "building…"
  local started=$SECONDS
  if (cd "$ROOT/apps/desktop" && cargo build -q); then
    stop_app
    "$APP" ${args[@]+"${args[@]}"} >"$LOG" 2>&1 &
    pid=$!
    log "opened in $((SECONDS - started)) s (pid $pid, log $LOG)"
    sleep 1
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
      pid=""
      log "the app exited at start; the end of its log:"
      show_log
      log "no window is open. Fix and save again."
    fi
  elif [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    log "build failed; the old window stays. Fix and save again."
  else
    log "build failed; no window is open. Fix and save again."
  fi
}

# The app ended while the watcher ran. A window the developer closed is a clean end and says
# nothing; a panic or a signal is reported once, with the end of the log.
note_app_end() {
  [[ -n "$pid" ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then return 0; fi
  local status=0
  wait "$pid" 2>/dev/null || status=$?
  pid=""
  if [[ "$status" != 0 ]]; then
    log "the app stopped (status $status); the end of its log:"
    show_log
  fi
}

last="$(fingerprint)"
build_and_open
log "watching apps/desktop and crates/ (Ctrl-C to stop)"
while true; do
  sleep 1
  note_app_end
  now="$(fingerprint)"
  if [[ "$now" != "$last" ]]; then
    last="$now"
    build_and_open
  fi
done
