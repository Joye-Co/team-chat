#!/usr/bin/env bash
# Deploys the API Worker of one env: applies the D1 migrations, then `wrangler deploy` with the
# VERSION var that GET /v1/health reports. The version is, in this order:
#   1. $DEPLOY_VERSION, when set (for example by a release job);
#   2. the release version when HEAD carries a release tag v<semver> (scripts/release.sh makes it);
#   3. otherwise the short commit, with "-dirty" when server/, packages/ or protocol/ have
#      uncommitted changes (what is deployed then is not exactly that commit).
#
# The env config comes from the private ops checkout (TC_OPS_DIR, deploy/server.wrangler.jsonc;
# scripts/setup-server.sh makes it for a new server). When the ops checkout has no URL for the
# env yet (deploy/servers.json), the first deploy takes the workers.dev address that wrangler
# prints and writes it there.
# A version that starts with 0.0.0 (the static VERSION values in the wrangler config) is refused.
# After the deploy it reads GET /v1/health until it reports that version (up to
# HEALTH_WAIT_SECONDS, default 60) and fails when it does not, or when it reports 0.0.0*.
#
# Usage: scripts/deploy-server.sh <e2e|staging|production> [--dry-run] [--print-version]
#                                 [--check-health [--expect <version>]]
#   --dry-run        wrangler deploy --dry-run (builds, uploads nothing, no D1 migrations)
#   --print-version  print the version that would be deployed and exit
#   --check-health   deploy nothing: check that /v1/health reports a real version (and
#                    <version> when --expect is given). CHAT_API_BASE overrides the URL.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need git

ENV_NAME=""
DRY_RUN=0
PRINT=0
CHECK=0
EXPECT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --print-version) PRINT=1; shift ;;
    --check-health) CHECK=1; shift ;;
    --expect) EXPECT="${2:-}"; [ -n "$EXPECT" ] || die "--expect needs a version"; shift 2 ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$ENV_NAME" ] || die "more than one env given"; ENV_NAME="$1"; shift ;;
  esac
done
case "$ENV_NAME" in
  e2e | staging | production) ;;
  *) die "usage: scripts/deploy-server.sh <e2e|staging|production> [--dry-run] [--print-version] [--check-health [--expect <version>]]" ;;
esac

# check_health <expected or empty>: polls /v1/health; fails on a placeholder or another version.
check_health() {
  need curl jq
  local want="${1#v}" base got="" waited=0 limit="${HEALTH_WAIT_SECONDS:-60}"
  base="$(env_base "$ENV_NAME")"
  while :; do
    got="$(health_version "$base")"
    if [ -n "$got" ] && ! placeholder_version "$got" && { [ -z "$want" ] || [ "$got" = "$want" ]; }; then
      log "$base/v1/health reports $got"
      return 0
    fi
    [ "$waited" -lt "$limit" ] || break
    sleep 5
    waited=$((waited + 5))
  done
  if [ -z "$got" ]; then die "$base/v1/health did not answer"; fi
  if placeholder_version "$got"; then die "$base/v1/health reports placeholder version $got; deploy with scripts/deploy-server.sh"; fi
  die "$base/v1/health reports $got, expected $want"
}

if [ "$CHECK" = 1 ]; then
  check_health "$EXPECT"
  exit 0
fi

deploy_version() {
  if [ -n "${DEPLOY_VERSION:-}" ]; then
    printf '%s' "${DEPLOY_VERSION#v}"
    return
  fi
  local tag v
  while IFS= read -r tag; do
    v="${tag#v}"
    if valid_semver "$v"; then
      printf '%s' "$v"
      return
    fi
  done < <(git -C "$ROOT" tag --points-at HEAD --list 'v*' --sort=-v:refname)
  v="$(git -C "$ROOT" rev-parse --short=12 HEAD)" || die "not a git checkout; set DEPLOY_VERSION"
  if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no -- server packages protocol)" ]; then v="$v-dirty"; fi
  printf '%s' "$v"
}

VERSION="$(deploy_version)"
[[ "$VERSION" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]{0,63}$ ]] || die "version '$VERSION' must be 1-64 letters, digits, '.', '+' or '-'"
! placeholder_version "$VERSION" || die "version '$VERSION' is a placeholder; /v1/health must report a real version"
if [ "$PRINT" = 1 ]; then
  printf '%s\n' "$VERSION"
  exit 0
fi

[ -n "${WRANGLER:-}" ] || need bun
[ -n "${WRANGLER:-}" ] || [ -x "$ROOT/server/node_modules/.bin/wrangler" ] ||
  die "wrangler is not installed in server/: run 'bun install --frozen-lockfile' at the repository root"
CONFIG="$(deploy_wrangler_config server)"
cd "$ROOT/server"
if [ "$DRY_RUN" = 1 ]; then
  log "dry run: $ENV_NAME version $VERSION (no D1 migrations, nothing uploaded)"
  wrangler deploy -c "$CONFIG" --env "$ENV_NAME" --var "VERSION:$VERSION" --dry-run
  exit
fi
need curl jq
# Known before the deploy, or learned from what wrangler prints (a first deploy).
KNOWN_BASE="$( (env_base "$ENV_NAME") 2>/dev/null || true)"
log "applying D1 migrations ($ENV_NAME)"
wrangler d1 migrations apply DB -c "$CONFIG" --env "$ENV_NAME" --remote
log "deploying chat API ($ENV_NAME) version $VERSION"
DEPLOY_LOG="$(mktemp)"
trap 'rm -f "$DEPLOY_LOG"' EXIT
wrangler deploy -c "$CONFIG" --env "$ENV_NAME" --var "VERSION:$VERSION" | tee "$DEPLOY_LOG"
if [ -z "$KNOWN_BASE" ]; then
  url="$(workers_dev_url <"$DEPLOY_LOG")"
  [ -n "$url" ] || die "deployed, but wrangler printed no workers.dev address: put the server's URL into $(ops_dir_path)/deploy/servers.json as \"$ENV_NAME\", then run scripts/deploy-server.sh $ENV_NAME --check-health"
  record_server_url "$ENV_NAME" "$url"
  log "first deploy of $ENV_NAME: wrote $url into $(ops_dir_path)/deploy/servers.json"
fi
check_health "$VERSION"
