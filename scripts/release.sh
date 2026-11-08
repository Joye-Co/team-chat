#!/usr/bin/env bash
# Makes a release: checks the release metadata (scripts/release-check.sh), sets the workspace
# version and the CHANGELOG.md date, builds and checks the DMG and the daemon,
# commits and tags v<version> on main, creates the GitHub Release, and uploads the DMG to
# the server so the app can update itself.
#
# Usage: scripts/release.sh <version> [--env staging|production] [--notes <file>]
#                           [--dry-run] [--upload-only]
#   --env          server to upload to (default production)
#   --notes        Markdown release notes (default: notes that GitHub generates)
#   --dry-run      run the checks and print the plan; change nothing
#   --upload-only  only upload the existing dist/<name>-<version>.dmg to the server
#                  (retry after the endpoint was missing or the upload failed)
# Env: CHAT_API_BASE overrides the server URL. The admin secret is read from
#      ~/.config/team-chat/<env>/admin-secret (TEAM_CHAT_CONFIG_DIR overrides
#      ~/.config/team-chat) and is never printed.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

VERSION=""
ENV_NAME=production
NOTES=""
DRY_RUN=0
UPLOAD_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --env) ENV_NAME="${2:-}"; shift 2 ;;
    --env=*) ENV_NAME="${1#*=}"; shift ;;
    --notes) NOTES="${2:-}"; shift 2 ;;
    --notes=*) NOTES="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --upload-only) UPLOAD_ONLY=1; shift ;;
    -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
    -*) die "unknown option: $1" ;;
    *) [ -z "$VERSION" ] || die "more than one version given"; VERSION="$1"; shift ;;
  esac
done

[ -n "$VERSION" ] || die "usage: scripts/release.sh <version> [--env staging|production] [--notes <file>] [--dry-run] [--upload-only]"
VERSION="${VERSION#v}"
valid_semver "$VERSION" || die "version '$VERSION' is not semver (for example 0.2.0 or 0.2.0-rc.1)"
# The server takes only MAJOR.MINOR.PATCH for app updates. A prerelease goes to GitHub only.
SERVER_UPLOAD=1
if ! valid_release_version "$VERSION"; then
  [[ "$VERSION" == *-* ]] || die "version '$VERSION' has a leading zero; the server would refuse it"
  [ "$UPLOAD_ONLY" = 0 ] || die "the server takes no prerelease versions ($VERSION); nothing to upload"
  SERVER_UPLOAD=0
fi
case "$ENV_NAME" in staging | production) ;; *) die "--env must be staging or production" ;; esac
if [ -n "$NOTES" ]; then [ -r "$NOTES" ] || die "cannot read notes file $NOTES"; NOTES="$(cd "$(dirname "$NOTES")" && pwd)/$(basename "$NOTES")"; fi

need git jq curl shasum base64
NAME="$(product name)"
TAG="v$VERSION"
DMG="$DIST/$NAME-$VERSION.dmg"
SECRET_FILE="${TEAM_CHAT_CONFIG_DIR:-$HOME/.config/team-chat}/$ENV_NAME/admin-secret"

BASE="$(env_base "$ENV_NAME")"

# release_notes_b64: notes for the server header, base64 on one line, at most ~6 KB of text.
release_notes_b64() {
  if [ -n "$NOTES" ]; then
    head -c 6000 "$NOTES" | iconv -f UTF-8 -t UTF-8 -c
  else
    printf '%s %s\n' "$NAME" "$VERSION"
  fi | base64 | tr -d '\n'
}

upload_to_server() {
  if [ "$SERVER_UPLOAD" = 0 ]; then
    log "skip server upload: $VERSION is a prerelease; the app updates only to MAJOR.MINOR.PATCH releases"
    return 0
  fi
  [ -f "$DMG" ] || die "no DMG at $DMG"
  if [ ! -s "$SECRET_FILE" ]; then
    log "skip server upload: no admin secret at $SECRET_FILE"
    log "after you add it, run: scripts/release.sh $VERSION --env $ENV_NAME --upload-only"
    return 0
  fi
  if [ -n "$(find "$SECRET_FILE" -perm -004 2>/dev/null)" ]; then log "warning: $SECRET_FILE is world-readable; run chmod 600"; fi

  local sha hdr resp code
  sha="$(shasum -a 256 "$DMG" | awk '{ print $1 }')"
  hdr="$(mktemp)"
  resp="$(mktemp)"
  chmod 600 "$hdr"
  # The secret goes into a mode-600 header file so it never shows in argv or output.
  { printf 'X-Admin-Secret: '; tr -d '\r\n' <"$SECRET_FILE"; printf '\n'; } >"$hdr"
  log "uploading $(basename "$DMG") to $BASE ($ENV_NAME)"
  code="$(curl -sS -o "$resp" -w '%{http_code}' --upload-file "$DMG" \
    --max-time 900 --retry 2 \
    -H @"$hdr" \
    -H "Content-Type: application/x-apple-diskimage" \
    -H "X-Sha256: $sha" \
    -H "X-Release-Notes: $(release_notes_b64)" \
    "$BASE/v1/admin/releases/$VERSION")" || code="000"
  rm -f "$hdr"
  case "$code" in
    2??) log "server upload done ($code)" ;;
    404)
      log "skip server upload: $BASE has no release endpoint yet (404)"
      log "when it exists, run: scripts/release.sh $VERSION --env $ENV_NAME --upload-only" ;;
    401 | 403) rm -f "$resp"; die "the server rejected the admin secret ($code); check $SECRET_FILE" ;;
    *)
      log "server answered $code: $(head -c 300 "$resp" | tr -d '\r')"
      rm -f "$resp"
      die "server upload failed; retry with: scripts/release.sh $VERSION --env $ENV_NAME --upload-only" ;;
  esac
  rm -f "$resp"
}

if [ "$UPLOAD_ONLY" = 1 ]; then
  [ "$DRY_RUN" = 0 ] || { log "dry run: would upload $DMG to $BASE/v1/admin/releases/$VERSION"; exit 0; }
  upload_to_server
  exit 0
fi

# Preflight. A real run stops on the first problem; a dry run lists them all.
problems=0
problem() {
  if [ "$DRY_RUN" = 1 ]; then printf 'would stop: %s\n' "$*" >&2; problems=$((problems + 1)); else die "$*"; fi
}

cd "$ROOT"
need gh
[ -z "$(git status --porcelain)" ] || problem "the working tree is not clean"
# Any worktree works (main may be checked out elsewhere) as long as HEAD is origin/main.
git fetch --quiet origin main --tags || problem "git fetch origin failed"
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main 2>/dev/null)" ] || problem "HEAD is not origin/main; pull or push first"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || [ -n "$(git ls-remote --tags origin "refs/tags/$TAG")" ]; then
  problem "tag $TAG exists already"
fi
gh auth status >/dev/null 2>&1 || problem "gh is not logged in"
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
[ -n "$REPO" ] || problem "gh cannot find the GitHub repository of origin"
CURRENT="$(workspace_version)"
# The notes must be final now; the version and the date are set below (release-check.sh).
[ -s "$ROOT/docs/releases/$VERSION.md" ] || problem "no release notes at docs/releases/$VERSION.md"
grep -qE "^## ${VERSION//./\\.} \((unreleased|[0-9]{4}-[0-9]{2}-[0-9]{2})\)$" CHANGELOG.md || problem "CHANGELOG.md has no $VERSION section"
if grep -qE 'Draft:|草稿：' "$ROOT/docs/releases/$VERSION.md" 2>/dev/null; then problem "docs/releases/$VERSION.md still has a draft note"; fi
[ -s "$SECRET_FILE" ] || log "note: no admin secret at $SECRET_FILE; the server upload will be skipped"

if [ "$DRY_RUN" = 1 ]; then
  cat >&2 <<PLAN
Release plan (dry run, nothing changed):
  version        $CURRENT -> $VERSION (Cargo.toml [workspace.package])
  build          scripts/build-dmg.sh, scripts/test-dmg.sh, scripts/build-daemon.sh
  commit         chore(release): $TAG  (Cargo.toml, Cargo.lock, CHANGELOG.md date)
  push           origin HEAD:main, then tag $TAG
  GitHub         ${REPO:-?}: release $TAG$([[ "$VERSION" == *-* ]] && printf ' (prerelease)')
                 assets: $NAME-$VERSION.dmg, daemon, .sha256 files
  notes          ${NOTES:-generated by GitHub}
  server         $([ "$SERVER_UPLOAD" = 1 ] && printf 'PUT %s/v1/admin/releases/%s (%s)' "$BASE" "$VERSION" "$ENV_NAME" || printf 'none: a prerelease is not offered as an app update')
  admin secret   $SECRET_FILE ($([ -s "$SECRET_FILE" ] && echo present || echo missing))
PLAN
  if [ "$problems" -gt 0 ]; then log "dry run found $problems problem(s); a real run would stop"; else log "dry run: all checks passed"; fi
  exit 0
fi

need cargo
# The operator's build: the character pack and the server URLs come from the ops checkout
# (apps/desktop/build.rs). A release without them would ship monograms and no server menu.
OPS="$(ops_dir)"
[ -n "$OPS" ] && [ -f "$OPS/characters/boxes.json" ] && [ -f "$OPS/deploy/servers.json" ] \
  || die "release needs the ops checkout (TC_OPS_DIR) with characters/ and deploy/servers.json"
export TC_OPS_DIR="$OPS"
log "setting version $VERSION"
restore() { git checkout --quiet -- Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true; }
trap 'restore' ERR
set_workspace_version "$VERSION"
cargo_env
cargo update --workspace --offline --manifest-path "$ROOT/Cargo.toml"
# "## <version> (unreleased)" gets today's date (UTC).
sed -i '' "s/^## ${VERSION//./\\.} (unreleased)\$/## $VERSION ($(date -u +%Y-%m-%d))/" CHANGELOG.md
"$ROOT/scripts/release-check.sh" "$VERSION" || die "release metadata of $VERSION is not final"

"$ROOT/scripts/build-dmg.sh"
"$ROOT/scripts/test-dmg.sh" "$DMG"
DAEMON="$("$ROOT/scripts/build-daemon.sh" | tail -n 1)"
[ -f "$DMG" ] && [ -f "$DAEMON" ] || die "build outputs are missing"

if ! git diff --quiet -- Cargo.toml Cargo.lock CHANGELOG.md; then
  git add Cargo.toml Cargo.lock CHANGELOG.md
  git commit --quiet -m "chore(release): $TAG"
fi
trap - ERR
git tag -a "$TAG" -m "$NAME $VERSION"
git fetch --quiet origin main || true
if git merge-base --is-ancestor HEAD origin/main; then
  # The version was set on main before; HEAD is on main already (main may have moved on).
  log "HEAD is on origin/main already; tagging it without a release commit"
elif ! git push --quiet origin HEAD:main; then
  git tag -d "$TAG" >/dev/null
  die "push to main was rejected (main moved). Your release commit is still local: run 'git reset --keep origin/main' if you want to drop it, then run this script again"
fi
git push --quiet origin "refs/tags/$TAG"

log "creating GitHub release $TAG"
gh_args=(release create "$TAG" "$DMG" "$DMG.sha256" "$DAEMON" "$DAEMON.sha256" --verify-tag --title "$NAME $VERSION")
if [ -n "$NOTES" ]; then gh_args+=(--notes-file "$NOTES"); else gh_args+=(--generate-notes); fi
if [[ "$VERSION" == *-* ]]; then gh_args+=(--prerelease); fi
gh "${gh_args[@]}"

upload_to_server
log "release $TAG done"
