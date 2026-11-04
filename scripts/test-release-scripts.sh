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

jq -n --argjson fails "$fails" --arg version "$(workspace_version)" '{ version: $version, failures: $fails }' >"$WORK/report.json"
[ "$fails" = 0 ] || die "$fails release script test(s) failed"
log "release script tests passed; report $WORK/report.json"
