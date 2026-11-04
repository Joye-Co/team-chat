#!/usr/bin/env bash
# Checks that the release metadata of <version> is final and consistent:
#   - Cargo.toml [workspace.package] and every workspace package in Cargo.lock have <version>;
#   - CHANGELOG.md has a "## <version> (YYYY-MM-DD)" or "## <version> (unreleased)" section with
#     no draft note (scripts/release.sh writes the date in the release commit);
#   - docs/releases/<version>.md exists and has no draft note;
#   - with --tag: the tag v<version> points at HEAD;
#   - with --env <env>: GET /v1/health of that env reports <version> (CHAT_API_BASE overrides).
# Prints one line per problem and exits 1 when there is any.
#
# Usage: scripts/release-check.sh <version> [--tag] [--env e2e|staging|production]
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

VERSION=""
TAG_CHECK=0
ENV_NAME=""
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) TAG_CHECK=1; shift ;;
    --env) ENV_NAME="${2:-}"; [ -n "$ENV_NAME" ] || die "--env needs an env"; shift 2 ;;
    -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
    -*) die "unknown option: $1" ;;
    *) [ -z "$VERSION" ] || die "more than one version given"; VERSION="${1#v}"; shift ;;
  esac
done
[ -n "$VERSION" ] || die "usage: scripts/release-check.sh <version> [--tag] [--env <env>]"
valid_semver "$VERSION" || die "version '$VERSION' is not semver"
need awk grep

problems=0
problem() { printf 'release-check: %s\n' "$*" >&2; problems=$((problems + 1)); }

ws="$(workspace_version)"
[ "$ws" = "$VERSION" ] || problem "Cargo.toml [workspace.package] version is $ws, not $VERSION"

# Workspace packages take version.workspace = true; their Cargo.lock entries must match.
while IFS= read -r manifest; do
  name="$(awk '/^\[package\]/ { p = 1; next } /^\[/ { p = 0 } p && /^name[[:space:]]*=/ { gsub(/.*=[[:space:]]*"|".*/, ""); print; exit }' "$ROOT/$manifest")"
  [ -n "$name" ] || continue
  locked="$(awk -v n="$name" '$0 == "name = \"" n "\"" { getline; gsub(/.*= "|"/, ""); print; exit }' "$ROOT/Cargo.lock")"
  [ "$locked" = "$VERSION" ] || problem "Cargo.lock has $name ${locked:-missing}, not $VERSION (run cargo update --workspace --offline)"
done < <(awk '/^members[[:space:]]*=/ { m = 1; next } m && /\]/ { exit } m { gsub(/[",[:space:]]/, ""); if ($0 != "") print $0 "/Cargo.toml" }' "$ROOT/Cargo.toml")

esc="${VERSION//./\\.}"
heading="$(grep -nE "^## $esc \(([0-9]{4}-[0-9]{2}-[0-9]{2}|unreleased)\)$" "$ROOT/CHANGELOG.md" | head -n 1 || true)"
if [ -z "$heading" ]; then
  problem "CHANGELOG.md has no '## $VERSION (YYYY-MM-DD)' section"
else
  start="${heading%%:*}"
  section="$(awk -v s="$start" 'NR > s && /^## / { exit } NR > s { print }' "$ROOT/CHANGELOG.md")"
  if grep -qE 'Draft:|草稿：' <<<"$section"; then problem "CHANGELOG.md $VERSION section still has a draft note"; fi
  if [ "$TAG_CHECK" = 1 ] && [[ "$heading" == *"(unreleased)" ]]; then problem "CHANGELOG.md $VERSION is still (unreleased) at the tag"; fi
fi

notes="$ROOT/docs/releases/$VERSION.md"
if [ ! -s "$notes" ]; then
  problem "no release notes at docs/releases/$VERSION.md"
elif grep -qE 'Draft:|草稿：' "$notes"; then
  problem "docs/releases/$VERSION.md still has a draft note"
fi

if [ "$TAG_CHECK" = 1 ]; then
  need git
  git -C "$ROOT" tag --points-at HEAD --list "v$VERSION" | grep -qx "v$VERSION" || problem "tag v$VERSION does not point at HEAD"
fi

if [ -n "$ENV_NAME" ]; then
  need curl jq
  base="$(env_base "$ENV_NAME")"
  got="$(health_version "$base")"
  if [ -z "$got" ]; then
    problem "$base/v1/health did not answer"
  elif placeholder_version "$got"; then
    problem "$base/v1/health reports placeholder version $got"
  elif [ "$got" != "$VERSION" ]; then
    problem "$base/v1/health reports $got, not $VERSION"
  fi
fi

if [ "$problems" -gt 0 ]; then
  log "release check of $VERSION: $problems problem(s)"
  exit 1
fi
log "release check of $VERSION: ok"
