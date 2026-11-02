#!/usr/bin/env bash
# Deploys the API Worker of one env: applies the D1 migrations, then `wrangler deploy` with the
# VERSION var that GET /v1/health reports. The version is, in this order:
#   1. $DEPLOY_VERSION, when set (for example by a release job);
#   2. the release version when HEAD carries a release tag v<semver> (scripts/release.sh makes it);
#   3. otherwise the short commit, with "-dirty" when server/, packages/ or protocol/ have
#      uncommitted changes (what is deployed then is not exactly that commit).
#
# Usage: scripts/deploy-server.sh <e2e|staging|production> [--dry-run] [--print-version]
#   --dry-run        wrangler deploy --dry-run (builds, uploads nothing, no D1 migrations)
#   --print-version  print the version that would be deployed and exit
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need git

ENV_NAME=""
DRY_RUN=0
PRINT=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --print-version) PRINT=1; shift ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$ENV_NAME" ] || die "more than one env given"; ENV_NAME="$1"; shift ;;
  esac
done
case "$ENV_NAME" in
  e2e | staging | production) ;;
  *) die "usage: scripts/deploy-server.sh <e2e|staging|production> [--dry-run] [--print-version]" ;;
esac

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
if [ "$PRINT" = 1 ]; then
  printf '%s\n' "$VERSION"
  exit 0
fi

cd "$ROOT/server"
if [ "$DRY_RUN" = 1 ]; then
  log "dry run: $ENV_NAME version $VERSION (no D1 migrations, nothing uploaded)"
  exec bunx wrangler deploy --env "$ENV_NAME" --var "VERSION:$VERSION" --dry-run
fi
log "applying D1 migrations ($ENV_NAME)"
bunx wrangler d1 migrations apply DB --env "$ENV_NAME" --remote
log "deploying chat API ($ENV_NAME) version $VERSION"
bunx wrangler deploy --env "$ENV_NAME" --var "VERSION:$VERSION"
