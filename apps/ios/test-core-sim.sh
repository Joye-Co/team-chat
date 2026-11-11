#!/usr/bin/env bash
# Runs chat-core's tests inside the iOS simulator: the same tests as `cargo test -p chat-core`,
# built for the simulator and started in it one test binary at a time.
# Uses the booted simulator, or boots the first available iPhone (SIM_DEVICE picks one by UDID).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TRIPLE=aarch64-apple-ios-sim

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
command -v sccache >/dev/null 2>&1 && export RUSTC_WRAPPER=sccache
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.tmp/target-ios}"
export IPHONEOS_DEPLOYMENT_TARGET=17.0

DEVICE="${SIM_DEVICE:-}"
if [[ -z "$DEVICE" ]]; then
  DEVICE="$(xcrun simctl list devices booted -j | jq -r '[.devices[][] | select(.state == "Booted")][0].udid // empty')"
fi
if [[ -z "$DEVICE" ]]; then
  DEVICE="$(xcrun simctl list devices available -j | jq -r '[.devices | to_entries[] | select(.key | contains("iOS")) | .value[] | select(.name | startswith("iPhone"))][0].udid // empty')"
  [[ -n "$DEVICE" ]] || { echo "no iPhone simulator is installed" >&2; exit 1; }
  xcrun simctl boot "$DEVICE"
fi
xcrun simctl bootstatus "$DEVICE" -b >/dev/null

cd "$ROOT"
BINS="$(cargo test --locked -p chat-core --target "$TRIPLE" --no-run --message-format=json \
  | jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable')"
[[ -n "$BINS" ]] || { echo "no test binaries were built" >&2; exit 1; }

# cargo runs a crate's tests from the crate's folder.
cd "$ROOT/crates/chat-core"
while IFS= read -r bin; do
  echo "== $(basename "$bin")"
  # The perf_ tests assert host timings (a 10 ms query); a simulator on a shared CI runner
  # misses them by chance. They run on the host in the Rust jobs.
  xcrun simctl spawn "$DEVICE" "$bin" --skip perf_
done <<<"$BINS"
