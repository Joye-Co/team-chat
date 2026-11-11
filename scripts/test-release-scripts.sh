#!/usr/bin/env bash
# Tests the release guards of scripts/deploy-server.sh and scripts/release-check.sh against a
# local stub of GET /v1/health. Deploys nothing and needs no secret.
#
# Usage: scripts/test-release-scripts.sh
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need bun curl jq

WORK="$ROOT/.tmp/test-release-scripts"
mkdir -p "$WORK"
PORT="${STUB_PORT:-18977}"
STUB_PID=""
cleanup() { if [ -n "$STUB_PID" ] && kill -0 "$STUB_PID" 2>/dev/null; then kill "$STUB_PID"; fi; }
trap cleanup EXIT

# stub <version>: serve {"ok":true,"version":<version>} on 127.0.0.1:$PORT.
stub() {
  cleanup
  bun -e "Bun.serve({ hostname: '127.0.0.1', port: $PORT, fetch: () => Response.json({ ok: true, version: process.argv[1] }) })" "$1" \
    >"$WORK/stub.log" 2>&1 &
  STUB_PID=$!
  local _
  for _ in $(seq 1 50); do
    [ "$(curl -s "http://127.0.0.1:$PORT/v1/health" | jq -r .version 2>/dev/null)" = "$1" ] && return 0
    sleep 0.1
  done
  die "stub did not start"
}

fails=0
expect() { # expect <ok|fail> <name> <command...>
  local want="$1" name="$2" got
  shift 2
  if "$@" >"$WORK/out.log" 2>&1; then got=ok; else got=fail; fi
  if [ "$got" = "$want" ]; then
    printf 'pass  %s\n' "$name"
  else
    printf 'FAIL  %s (wanted %s, got %s)\n' "$name" "$want" "$got"
    sed 's/^/      /' "$WORK/out.log"
    fails=$((fails + 1))
  fi
}

export CHAT_API_BASE="http://127.0.0.1:$PORT" HEALTH_WAIT_SECONDS=0
D="$ROOT/scripts/deploy-server.sh"

expect fail "deploy refuses a 0.0.0 version" env DEPLOY_VERSION=0.0.0-staging "$D" staging --print-version
expect fail "deploy refuses version 0.0.0" env DEPLOY_VERSION=0.0.0 "$D" production --print-version
expect ok "deploy takes a release version" env DEPLOY_VERSION=1.1.0 "$D" staging --print-version

stub 0.0.0-staging
expect fail "health check fails on a placeholder version" "$D" staging --check-health
expect fail "release-check --env fails on a placeholder version" "$ROOT/scripts/release-check.sh" "$(workspace_version)" --env staging
stub 1.0.0
expect fail "health check fails on another version" "$D" staging --check-health --expect 1.1.0
expect ok "health check takes a real version" "$D" staging --check-health
stub "$(workspace_version)"
expect ok "health check takes the expected version" "$D" staging --check-health --expect "$(workspace_version)"
expect ok "release-check passes for the workspace version" "$ROOT/scripts/release-check.sh" "$(workspace_version)" --env staging
expect fail "release-check fails for another version" "$ROOT/scripts/release-check.sh" 9.9.9

is() { [ "$1" = "$2" ] || { printf 'got "%s", want "%s"\n' "$1" "$2"; return 1; }; }

# --- the previous Windows installer, for the publisher comparison of a release -----------------
# A stand-in for gh: releases newest first; v9.9.9 is the release being made (the macOS job has
# created it already), v9.9.8 has no installer, v9.9.7 has one. GH_FAIL makes one call fail.
FAKE_BIN="$WORK/fake-bin"
rm -rf "$FAKE_BIN" "$WORK/previous" && mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/gh" <<'GH'
#!/usr/bin/env bash
case "$1 $2" in
  "release list")
    [ "${GH_FAIL:-}" != list ] || exit 1
    printf '%s\n' ${GH_TAGS:-v9.9.9 v9.9.8 v9.9.7 v9.9.6} ;;
  "release view")
    [ "${GH_FAIL:-}" != view ] || exit 1
    case "$3" in
      v9.9.9) printf 'X-9.9.9.dmg\nX-9.9.9-setup.exe\n' ;;
      v9.9.8) printf 'X-9.9.8.dmg\n' ;;
      v9.9.7) printf 'X-9.9.7.dmg\nX-9.9.7-setup.exe\nx-daemon-9.9.7-x86_64-pc-windows-msvc.exe\n' ;;
      v9.9.6) printf 'X-9.9.6-setup.exe\n' ;;
    esac ;;
  "release download")
    [ "${GH_FAIL:-}" != download ] || exit 1
    dir=""; name=""; tag="$3"; shift 3
    while [ $# -gt 0 ]; do case "$1" in --dir) dir="$2"; shift 2 ;; --pattern) name="$2"; shift 2 ;; *) shift ;; esac; done
    printf 'installer of %s' "$tag" >"$dir/$name" ;;
  *) exit 2 ;;
esac
GH
chmod +x "$FAKE_BIN/gh"
P="$ROOT/scripts/previous-windows-installer.sh"
prev() { PATH="$FAKE_BIN:$PATH" "$P" "$@" 2>"$WORK/prev.err"; }
expect ok "the release being made is not its own previous one" is "$(prev 9.9.9 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "a release without an installer is passed over" is "$(cat "$WORK/previous/X-9.9.7-setup.exe")" "installer of v9.9.7"
expect ok "a version given with v works" is "$(prev v9.9.9 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "the newest other release with an installer wins" is "$(prev 9.9.10 "$WORK/previous")" "$WORK/previous/X-9.9.9-setup.exe"
expect ok "no earlier installer is not an error" env PATH="$FAKE_BIN:$PATH" GH_TAGS="v9.9.9 v9.9.8" "$P" 9.9.9 "$WORK/previous"
expect ok "and names nothing" is "$(GH_TAGS="v9.9.9 v9.9.8" prev 9.9.9 "$WORK/previous")" ""
for step in list view download; do
  expect fail "a failed 'gh release $step' fails the check" env PATH="$FAKE_BIN:$PATH" GH_FAIL="$step" "$P" 9.9.9 "$WORK/previous"
done

# --- the logs of a run must not show a server address -----------------------------------------
L="$ROOT/scripts/check-logs-for-servers.sh"
rm -rf "$WORK/logs" && mkdir -p "$WORK/logs"
printf 'build\nuploading to ***\n' >"$WORK/logs/1.log"
SERVERS='{"staging":"https://staging.chat.example.org","production":"https://chat.example.org:8443/api"}'
expect ok "masked logs pass" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
printf 'GET https://chat.example.org:8443/api/v1/health\n' >"$WORK/logs/2.log"
expect fail "a log with the production host fails" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
expect ok "the failure does not print the host" bash -c "! grep -q 'example.org' '$WORK/out.log'"
printf 'TC_SERVERS_JSON: {"staging":"https://staging.chat.example.org"}\n' >"$WORK/logs/2.log"
expect fail "a log with the staging host fails" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
expect ok "a run without server URLs has nothing to check" env TC_SERVERS_JSON= LOG_DIR="$WORK/logs" "$L"

jq -n --argjson fails "$fails" --arg version "$(workspace_version)" '{ version: $version, failures: $fails }' >"$WORK/report.json"
[ "$fails" = 0 ] || die "$fails release script test(s) failed"
log "release script tests passed; report $WORK/report.json"
