#!/usr/bin/env bash
# Makes a release: checks the release metadata (scripts/release-check.sh), sets the workspace
# version and the CHANGELOG.md date, builds and checks the DMG and the daemon,
# commits and tags v<version> on main, creates the GitHub Release, and uploads the DMG to
# the server so the app can update itself.
#
# Usage: scripts/release.sh <version> [--env staging|production] [--notes <file>]
#                           (--windows-installer <setup.exe> [--asset <file>]... | --without-windows)
#                           [--dry-run [--build]] [--same-as <dmg>]
#                           [--prebuilt <dir>] [--linux-daemon <file>]
#        scripts/release.sh <version> --build-only [--dry-run] [--same-as <dmg>] [--resume]
#        scripts/release.sh <version> ... --resume
#        scripts/release.sh <version> --reupload
#        scripts/release.sh <version> --upload-only [--platform windows|linux-x86_64]
#        scripts/release.sh <version> --add-windows <setup.exe> [--asset <file>]...
#   --env          server to upload to (default production)
#   --notes        Markdown release notes (default: notes that GitHub generates)
#   --windows-installer  the Windows installer of this version, built and tested before
#                  (scripts/build-windows-installer.sh on Windows; the Release workflow's
#                  Windows job). A release is one thing for both systems: the GitHub Release
#                  is created with the DMG and the installer together, and both go to the
#                  server. Without it a release stops before it changes anything
#   --asset        another file for the GitHub Release (the Windows daemon alone)
#   --without-windows  publish the Mac alone, on purpose: Windows apps are offered nothing
#                  for this version until --add-windows
#   --dry-run      run the checks and print the plan; change nothing
#   --build        with --dry-run: also set the version in the working tree, build and check the
#                  DMG and the daemon into dist/, then restore Cargo.toml, Cargo.lock, CHANGELOG.md
#   --same-as      also check that the new DMG has the same designated requirement as <dmg>
#                  (the previous release), as scripts/test-dmg.sh --same-as does
#   --build-only   the first half of a release, for a run that builds every system at the same
#                  time (the Release workflow): the checks, the version, the build and the
#                  checks of the DMG and the daemon into dist/, and the version files of the
#                  release commit into dist/release-files/. Commits, tags and uploads nothing,
#                  and needs no Windows installer. With --dry-run a failed check is reported
#                  and the build goes on
#   --prebuilt     the second half: take the DMG, the Mac daemon and release-files/ from <dir>
#                  (what --build-only wrote on a Mac, at this same commit) instead of
#                  building; then the commit, the tag, the GitHub Release and the uploads as
#                  ever. Runs on any system, without cargo
#   --linux-daemon the Linux daemon of this version (scripts/build-daemon.sh on Linux): goes to
#                  the GitHub Release with the other files and to the server as the
#                  linux-x86_64 file of the release
#   --resume       finish a release that stopped after its tag was pushed and before the GitHub
#                  Release was made: the tag v<version> exists on main, the release does not.
#                  Builds the tag's source again (check it out first), makes no commit and no
#                  tag, creates the GitHub Release and uploads. Takes the options of a release
#   --reupload     the repair after a release whose uploads to the server did not all arrive:
#                  downloads the DMG and the Windows installer of the GitHub Release
#                  v<version>, checks them against GitHub's digests, and uploads each to the
#                  server. A file the server already has with the same content counts as
#                  done; both are tried, and one that is missing or refused fails the run
#   --allow-missing-route  a server that answers 404 to an upload is skipped instead of
#                  failing (a server that is meant to have no release routes)
#   --upload-only  only upload the existing dist/<name>-<version>.dmg to the server
#                  (retry after the endpoint was missing or the upload failed)
#   --platform     with --upload-only: macos (default, the DMG) or windows, which uploads
#                  dist/<name>-<version>-setup.exe as the Windows file of the release;
#                  linux-x86_64 uploads the daemon alone,
#                  dist/<slug>-daemon-<version>-x86_64-unknown-linux-gnu, which Linux hosts
#                  install and update themselves from (built on Linux by scripts/build-daemon.sh,
#                  the Release workflow's Linux job)
#   --add-windows  give a release that exists its Windows installer (it was published
#                  --without-windows, or its Windows build failed and was fixed since): the
#                  installer goes to the server first, then to the GitHub Release. A server
#                  that already has an installer for this version refuses: a published file is
#                  never replaced, release a higher version instead
# Env: CHAT_API_BASE overrides the server URL. The admin secret is read from
#      ~/.config/team-chat/<env>/admin-secret (TEAM_CHAT_CONFIG_DIR overrides
#      ~/.config/team-chat) and is never printed. The Release workflow
#      (.github/workflows/release.yml) runs --build-only on a macOS runner while the Windows
#      and Linux jobs build, then --prebuilt in the job that publishes.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

VERSION=""
ENV_NAME=production
NOTES=""
DRY_RUN=0
UPLOAD_ONLY=0
BUILD=0
SAME_AS=""
PLATFORM=macos
WINDOWS_INSTALLER=""
WITHOUT_WINDOWS=0
ADD_WINDOWS=""
ASSETS=()
RESUME=0
REUPLOAD=0
BUILD_ONLY=0
PREBUILT=""
LINUX_DAEMON=""
ALLOW_MISSING_ROUTE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --env) ENV_NAME="${2:-}"; shift 2 ;;
    --env=*) ENV_NAME="${1#*=}"; shift ;;
    --notes) NOTES="${2:-}"; shift 2 ;;
    --notes=*) NOTES="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --upload-only) UPLOAD_ONLY=1; shift ;;
    --platform) PLATFORM="${2:-}"; shift 2 ;;
    --platform=*) PLATFORM="${1#*=}"; shift ;;
    --build) BUILD=1; shift ;;
    --windows-installer) WINDOWS_INSTALLER="${2:-}"; shift 2 ;;
    --windows-installer=*) WINDOWS_INSTALLER="${1#*=}"; shift ;;
    --without-windows) WITHOUT_WINDOWS=1; shift ;;
    --resume) RESUME=1; shift ;;
    --build-only) BUILD_ONLY=1; shift ;;
    --prebuilt) PREBUILT="${2:-}"; shift 2 ;;
    --prebuilt=*) PREBUILT="${1#*=}"; shift ;;
    --linux-daemon) LINUX_DAEMON="${2:-}"; shift 2 ;;
    --linux-daemon=*) LINUX_DAEMON="${1#*=}"; shift ;;
    --reupload) REUPLOAD=1; shift ;;
    --allow-missing-route) ALLOW_MISSING_ROUTE=1; shift ;;
    --add-windows) ADD_WINDOWS="${2:-}"; shift 2 ;;
    --add-windows=*) ADD_WINDOWS="${1#*=}"; shift ;;
    --asset) ASSETS+=("${2:-}"); shift 2 ;;
    --asset=*) ASSETS+=("${1#*=}"); shift ;;
    --same-as) SAME_AS="${2:-}"; shift 2 ;;
    --same-as=*) SAME_AS="${1#*=}"; shift ;;
    -h | --help) sed -n '2,71p' "$0"; exit 0 ;;
    -*) die "unknown option: $1" ;;
    *) [ -z "$VERSION" ] || die "more than one version given"; VERSION="$1"; shift ;;
  esac
done

[ -n "$VERSION" ] || die "usage: scripts/release.sh <version> [--env staging|production] [--notes <file>] (--windows-installer <setup.exe> | --without-windows) [--dry-run [--build]] [--same-as <dmg>] | --upload-only [--platform windows|linux-x86_64] | --add-windows <setup.exe>"
case "$PLATFORM" in macos | windows | linux-x86_64) ;; *) die "--platform must be macos, windows or linux-x86_64" ;; esac
[ "$PLATFORM" = macos ] || [ "$UPLOAD_ONLY" = 1 ] || die "--platform $PLATFORM goes with --upload-only: that file is built on its own system (the Release workflow's Windows and Linux jobs)"
[ -z "$WINDOWS_INSTALLER" ] || [ "$WITHOUT_WINDOWS" = 0 ] || die "--windows-installer and --without-windows contradict each other"
if [ -n "$ADD_WINDOWS" ]; then
  [ "$UPLOAD_ONLY" = 0 ] && [ "$BUILD" = 0 ] && [ -z "$WINDOWS_INSTALLER" ] && [ "$WITHOUT_WINDOWS" = 0 ] && [ "$RESUME" = 0 ] && [ "$REUPLOAD" = 0 ] ||
    die "--add-windows stands alone (with --env, --notes, --asset, --dry-run)"
fi
if [ "$REUPLOAD" = 1 ]; then
  [ "$UPLOAD_ONLY" = 0 ] && [ "$BUILD" = 0 ] && [ -z "$WINDOWS_INSTALLER" ] && [ "$WITHOUT_WINDOWS" = 0 ] && [ "$RESUME" = 0 ] ||
    die "--reupload stands alone (with --env, --notes, --dry-run)"
fi
[ "$RESUME" = 0 ] || [ "$UPLOAD_ONLY" = 0 ] || die "--resume is for a release, not for --upload-only"
if [ "$BUILD_ONLY" = 1 ]; then
  [ "$UPLOAD_ONLY" = 0 ] && [ "$REUPLOAD" = 0 ] && [ -z "$ADD_WINDOWS" ] && [ -z "$WINDOWS_INSTALLER" ] && [ "$WITHOUT_WINDOWS" = 0 ] && [ -z "$PREBUILT" ] && [ -z "$LINUX_DAEMON" ] ||
    die "--build-only builds the Mac's files and publishes nothing (with --env, --notes, --same-as, --dry-run, --resume)"
  BUILD=1
fi
[ "$BUILD" = 0 ] || [ "$DRY_RUN" = 1 ] || [ "$BUILD_ONLY" = 1 ] || die "--build goes with --dry-run (a real run always builds)"
if [ -n "$PREBUILT" ] || [ -n "$LINUX_DAEMON" ]; then
  [ "$UPLOAD_ONLY" = 0 ] && [ "$REUPLOAD" = 0 ] && [ -z "$ADD_WINDOWS" ] && [ "$BUILD" = 0 ] || die "--prebuilt and --linux-daemon go with a release"
fi
VERSION="${VERSION#v}"
valid_semver "$VERSION" || die "version '$VERSION' is not semver (for example 0.2.0 or 0.2.0-rc.1)"
# The server takes only MAJOR.MINOR.PATCH for app updates. A prerelease goes to GitHub only.
SERVER_UPLOAD=1
if ! valid_release_version "$VERSION"; then
  [[ "$VERSION" == *-* ]] || die "version '$VERSION' has a leading zero; the server would refuse it"
  [ "$UPLOAD_ONLY" = 0 ] && [ "$REUPLOAD" = 0 ] || die "the server takes no prerelease versions ($VERSION); nothing to upload"
  SERVER_UPLOAD=0
fi
case "$ENV_NAME" in staging | production) ;; *) die "--env must be staging or production" ;; esac
# abs <file>: its absolute path (the script changes folder later).
abs() { printf '%s/%s' "$(cd "$(dirname "$1")" && pwd)" "$(basename "$1")"; }
if [ -n "$SAME_AS" ]; then [ -r "$SAME_AS" ] || die "cannot read $SAME_AS"; SAME_AS="$(cd "$(dirname "$SAME_AS")" && pwd)/$(basename "$SAME_AS")"; fi
if [ -n "$NOTES" ]; then [ -r "$NOTES" ] || die "cannot read notes file $NOTES"; NOTES="$(cd "$(dirname "$NOTES")" && pwd)/$(basename "$NOTES")"; fi

need git jq curl shasum base64
NAME="$(product name)"
TAG="v$VERSION"
DMG="$DIST/$NAME-$VERSION.dmg"
SETUP_NAME="$NAME-$VERSION-setup.exe"
# windows_installer_problem <file>: why <file> cannot be this version's Windows installer;
# prints nothing when it can.
windows_installer_problem() {
  if [ ! -s "$1" ]; then printf 'no Windows installer at %s' "$1"
  elif [ "$(basename "$1")" != "$SETUP_NAME" ]; then printf 'the Windows installer must be %s, not %s' "$SETUP_NAME" "$(basename "$1")"
  fi
}
# (Counted loops and ${ASSETS[@]+...}: an empty array is "unbound" to the bash of macOS.)
i=0
while [ "$i" -lt "${#ASSETS[@]}" ]; do
  [ -s "${ASSETS[$i]}" ] || die "no file at ${ASSETS[$i]} (--asset)"
  ASSETS[$i]="$(abs "${ASSETS[$i]}")"
  i=$((i + 1))
done
if [ -n "$WINDOWS_INSTALLER" ] && [ -e "$WINDOWS_INSTALLER" ]; then WINDOWS_INSTALLER="$(abs "$WINDOWS_INSTALLER")"; fi
if [ -n "$ADD_WINDOWS" ] && [ -e "$ADD_WINDOWS" ]; then ADD_WINDOWS="$(abs "$ADD_WINDOWS")"; fi
LINUX_NAME="$(product slug)-daemon-$VERSION-x86_64-unknown-linux-gnu"
if [ -n "$LINUX_DAEMON" ]; then
  [ -s "$LINUX_DAEMON" ] || die "no Linux daemon at $LINUX_DAEMON"
  [ "$(basename "$LINUX_DAEMON")" = "$LINUX_NAME" ] || die "the Linux daemon must be $LINUX_NAME, not $(basename "$LINUX_DAEMON")"
  LINUX_DAEMON="$(abs "$LINUX_DAEMON")"
fi
if [ -n "$PREBUILT" ]; then
  [ -d "$PREBUILT" ] || die "no folder at $PREBUILT (--prebuilt)"
  PREBUILT="$(cd "$PREBUILT" && pwd)"
fi
# use_platform <macos|windows> [<file>]: what upload_to_server sends next: the DMG to the release
# route every server has, or the Windows installer to the route for the files of other
# platforms (docs/protocol.md).
use_platform() {
  PLATFORM="$1"
  case "$1" in
    macos) ASSET="${2:-$DMG}"; ASSET_ROUTE="/v1/admin/releases/$VERSION"; ASSET_TYPE="application/x-apple-diskimage" ;;
    windows) ASSET="${2:-$DIST/$SETUP_NAME}"; ASSET_ROUTE="/v1/admin/releases/$VERSION/assets/windows"; ASSET_TYPE="application/vnd.microsoft.portable-executable" ;;
    linux-x86_64) ASSET="${2:-$DIST/$(product slug)-daemon-$VERSION-x86_64-unknown-linux-gnu}"; ASSET_ROUTE="/v1/admin/releases/$VERSION/assets/linux-x86_64"; ASSET_TYPE="application/octet-stream" ;;
  esac
}
use_platform "$PLATFORM"
# 1 when the last upload_to_server was refused because the server has that file already.
UPLOAD_CONFLICT=0
SECRET_FILE="${TEAM_CHAT_CONFIG_DIR:-$HOME/.config/team-chat}/$ENV_NAME/admin-secret"

# Empty when this machine has no ops checkout; the upload and the preflight report it.
BASE="$(env_base "$ENV_NAME" 2>/dev/null || true)"

# release_notes_b64: notes for the server header, base64 on one line, at most ~6 KB of text.
release_notes_b64() {
  if [ -n "$NOTES" ]; then
    head -c 6000 "$NOTES" | iconv -f UTF-8 -t UTF-8 -c
  else
    printf '%s %s\n' "$NAME" "$VERSION"
  fi | base64 | tr -d '\n'
}

upload_to_server() {
  UPLOAD_CONFLICT=0
  if [ "$SERVER_UPLOAD" = 0 ]; then
    log "skip server upload: $VERSION is a prerelease; the app updates only to MAJOR.MINOR.PATCH releases"
    return 0
  fi
  [ -f "$ASSET" ] || die "no file at $ASSET"
  # The GitHub Release holds both files once it exists: --reupload sends what is missing.
  local again="scripts/release.sh $VERSION --env $ENV_NAME --reupload"
  if [ ! -s "$SECRET_FILE" ]; then
    # A run that was asked to upload (--reupload, --upload-only) and cannot must not look done.
    [ "$REUPLOAD" = 0 ] && [ "$UPLOAD_ONLY" = 0 ] || die "no admin secret at $SECRET_FILE: nothing can be uploaded to $ENV_NAME"
    log "skip server upload: no admin secret at $SECRET_FILE"
    log "after you add it, run: $again"
    return 0
  fi
  if [ -n "$(find "$SECRET_FILE" -perm -004 2>/dev/null)" ]; then log "warning: $SECRET_FILE is world-readable; run chmod 600"; fi

  local sha hdr resp code
  [ -n "$BASE" ] || die "no server URL for $ENV_NAME: set TC_OPS_DIR (deploy/servers.json) or CHAT_API_BASE"
  sha="$(shasum -a 256 "$ASSET" | awk '{ print $1 }')"
  hdr="$(mktemp)"
  resp="$(mktemp)"
  chmod 600 "$hdr"
  # The secret goes into a mode-600 header file so it never shows in argv or output.
  { printf 'X-Admin-Secret: '; tr -d '\r\n' <"$SECRET_FILE"; printf '\n'; } >"$hdr"
  log "uploading $(basename "$ASSET") to $BASE ($ENV_NAME)"
  code="$(curl -sS -o "$resp" -w '%{http_code}' --upload-file "$ASSET" \
    --max-time 900 --retry 2 \
    -H @"$hdr" \
    -H "Content-Type: $ASSET_TYPE" \
    -H "X-Sha256: $sha" \
    -H "X-Release-Notes: $(release_notes_b64)" \
    "$BASE$ASSET_ROUTE")" || code="000"
  rm -f "$hdr"
  case "$code" in
    2??) log "server upload done ($code)" ;;
    404)
      rm -f "$resp"
      if [ "$ALLOW_MISSING_ROUTE" = 1 ]; then
        log "skip server upload: $BASE has no route for the $PLATFORM file (404), and --allow-missing-route says that is meant"
        return 0
      fi
      die "$BASE has no route for the $PLATFORM file of a release (404): nothing was uploaded. Deploy the server, then run: $again" ;;
    401 | 403) rm -f "$resp"; die "the server rejected the admin secret ($code); check $SECRET_FILE" ;;
    409)
      rm -f "$resp"
      UPLOAD_CONFLICT=1
      die "the server has another $PLATFORM file for $VERSION already (409; a server from before it accepted the same file twice answers this for the same file too: deploy it). A published file is never replaced: withdraw the version if it is bad and release a higher one" ;;
    *)
      log "server answered $code: $(head -c 300 "$resp" | tr -d '\r')"
      rm -f "$resp"
      die "server upload failed; retry with: $again" ;;
  esac
  rm -f "$resp"
}

# --add-windows: the release exists; give it its Windows installer. The server first (it
# refuses a second installer for a version), then the GitHub Release.
if [ -n "$ADD_WINDOWS" ]; then
  need gh
  why="$(windows_installer_problem "$ADD_WINDOWS")"
  [ -z "$why" ] || die "$why"
  gh release view "$TAG" --json tagName >/dev/null 2>&1 || die "there is no GitHub Release $TAG to add the installer to; make the release first"
  if [ "$SERVER_UPLOAD" = 1 ] && [ ! -s "$SECRET_FILE" ]; then die "no admin secret at $SECRET_FILE: the installer cannot go to the server"; fi
  if [ "$DRY_RUN" = 1 ]; then
    log "dry run: would upload $(basename "$ADD_WINDOWS") to $BASE/v1/admin/releases/$VERSION/assets/windows and add it to the GitHub Release $TAG"
    exit 0
  fi
  use_platform windows "$ADD_WINDOWS"
  upload_to_server
  log "adding $(basename "$ADD_WINDOWS") to the GitHub Release $TAG"
  gh release upload "$TAG" "$ADD_WINDOWS" ${ASSETS[@]+"${ASSETS[@]}"} --clobber || die "the installer is on the server but not on the GitHub Release $TAG; add it there by hand: gh release upload $TAG <file>"
  log "release $TAG has its Windows installer"
  exit 0
fi

# --reupload: both files of the GitHub Release go to the server. Each is tried, whatever
# happened to the other; the server answers 200 for a file it already has with the same
# content, so a release whose DMG arrived and whose installer did not is repaired by this.
if [ "$REUPLOAD" = 1 ]; then
  need gh
  if [ "$DRY_RUN" = 1 ]; then
    log "dry run: would download the DMG and the Windows installer of $TAG and upload them to $BASE"
    exit 0
  fi
  [ -s "$SECRET_FILE" ] || die "no admin secret at $SECRET_FILE: nothing can be uploaded to $ENV_NAME"
  WORK="$(mktemp -d "${TMPDIR:-/tmp}/reupload.XXXXXX")"
  trap 'rm -rf "$WORK"' EXIT
  failed=""
  for platform in macos windows; do
    case "$platform" in macos) pattern='*.dmg' ;; windows) pattern='*-setup.exe' ;; esac
    mkdir -p "$WORK/$platform"
    if ! gh release download "$TAG" --pattern "$pattern" --dir "$WORK/$platform" >/dev/null 2>&1; then
      log "error: the GitHub Release $TAG has no $platform file ($pattern)$([ "$platform" = windows ] && printf ': add one with --add-windows (the workflow: mode=windows-only)')"
      failed="$failed $platform"
      continue
    fi
    file="$(find "$WORK/$platform" -type f | head -n 1)"
    if ! "$ROOT/scripts/check-release-digest.sh" "$TAG" "$file" >&2; then
      failed="$failed $platform"
      continue
    fi
    # A subshell: a refused upload ends it, not this loop.
    (use_platform "$platform" "$file"; upload_to_server) || failed="$failed $platform"
  done
  [ -z "$failed" ] || die "not on the server after --reupload:$failed (see above)"
  log "both files of $TAG are on the server"
  exit 0
fi

if [ "$UPLOAD_ONLY" = 1 ]; then
  [ "$DRY_RUN" = 0 ] || { log "dry run: would upload $ASSET to $BASE$ASSET_ROUTE"; exit 0; }
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
# A release is for both systems. This comes first: nothing below matters without it.
if [ "$BUILD_ONLY" = 1 ]; then
  : # The half that builds the Mac's files: the installer is the publishing half's business.
elif [ -n "$WINDOWS_INSTALLER" ]; then
  why="$(windows_installer_problem "$WINDOWS_INSTALLER")"
  [ -z "$why" ] || problem "$why"
elif [ "$WITHOUT_WINDOWS" = 0 ]; then
  problem "a release needs the Windows installer of $VERSION: --windows-installer <$SETUP_NAME> (the Release workflow builds and tests it first), or --without-windows to publish the Mac alone on purpose"
else
  log "note: --without-windows: this release has no Windows installer; add it later with --add-windows"
fi
git fetch --quiet origin main --tags || problem "git fetch origin failed"
gh auth status >/dev/null 2>&1 || problem "gh is not logged in"
tag_exists=0
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || [ -n "$(git ls-remote --tags origin "refs/tags/$TAG" 2>/dev/null)" ]; then tag_exists=1; fi
release_exists=0
if gh release view "$TAG" --json tagName >/dev/null 2>&1; then release_exists=1; fi
if [ "$RESUME" = 1 ]; then
  # The release stopped between the tag and the GitHub Release: the commit and the tag are
  # on main already, nothing is public. Build that source again and go on from there.
  if [ "$tag_exists" = 0 ]; then
    problem "--resume: there is no tag $TAG; make the release without --resume"
  elif [ "$release_exists" = 1 ]; then
    problem "--resume: the GitHub Release $TAG exists already; --reupload sends its files to the server again, --add-windows adds a missing installer"
  else
    git merge-base --is-ancestor "$TAG" origin/main 2>/dev/null || problem "--resume: tag $TAG is not on main"
    [ "$(git show "$TAG:Cargo.toml" 2>/dev/null | workspace_version /dev/stdin)" = "$VERSION" ] || problem "--resume: tag $TAG does not hold version $VERSION"
    # The workflow keeps its own scripts and workflows and takes everything else from the tag.
    git diff --quiet "$TAG" -- . ':(exclude)scripts' ':(exclude).github' 2>/dev/null ||
      problem "--resume: the working tree is not the source of $TAG; check it out first (git checkout $TAG)"
  fi
else
  [ -z "$(git status --porcelain)" ] || problem "the working tree is not clean"
  # Any worktree works (main may be checked out elsewhere) as long as HEAD is origin/main.
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main 2>/dev/null)" ] || problem "HEAD is not origin/main; pull or push first (a commit that reached main while the release was being prepared stops it here: start the release again)"
  if [ "$tag_exists" = 1 ] && [ "$release_exists" = 0 ]; then
    problem "tag $TAG exists but the GitHub Release does not: an earlier run stopped in between. Finish it with --resume"
  elif [ "$tag_exists" = 1 ]; then
    problem "tag $TAG exists already"
  fi
fi
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
[ -n "$REPO" ] || problem "gh cannot find the GitHub repository of origin"
CURRENT="$(workspace_version)"
# The notes must be final now; the version and the date are set below (release-check.sh).
[ -s "$ROOT/docs/releases/$VERSION.md" ] || problem "no release notes at docs/releases/$VERSION.md"
grep -qE "^## ${VERSION//./\\.} \((unreleased|[0-9]{4}-[0-9]{2}-[0-9]{2})\)$" CHANGELOG.md || problem "CHANGELOG.md has no $VERSION section"
if grep -qE 'Draft:|草稿：' "$ROOT/docs/releases/$VERSION.md" 2>/dev/null; then problem "docs/releases/$VERSION.md still has a draft note"; fi
[ -s "$SECRET_FILE" ] || [ "$BUILD_ONLY" = 1 ] || log "note: no admin secret at $SECRET_FILE; the server upload will be skipped"

# release_ops: a release embeds only the server URLs (the 正式服务器 / 测试服务器 menu), never
# the local roster (the character art is in the repository). Builds against an ops dir that holds servers.json alone,
# from TC_SERVERS_JSON (the Release workflow) or the ops checkout's deploy/servers.json.
release_ops() {
  local dir="$ROOT/.tmp/release-ops"
  rm -rf "$dir" && mkdir -p "$dir/deploy"
  servers_json >"$dir/deploy/servers.json" \
    || die "release needs the server URLs: TC_SERVERS_JSON, or TC_OPS_DIR with deploy/servers.json"
  export TC_OPS_DIR="$dir"
}

restore() { git checkout --quiet -- Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true; }

# The files of the release commit, as --build-only keeps them next to its build.
RELEASE_FILES=(Cargo.toml Cargo.lock CHANGELOG.md)

# take_prebuilt: what --build-only built at this commit instead of a build here: the version
# files into the working tree (not for --resume: the tag holds them), the DMG and the daemon
# into dist/. Every file is checked against the .sha256 written next to it. Sets DAEMON.
take_prebuilt() {
  local f found
  found="$(find "$PREBUILT" -maxdepth 1 -type f -name "*-daemon-$VERSION-*-apple-darwin" | head -n 1)"
  [ -s "$PREBUILT/$(basename "$DMG")" ] || die "--prebuilt: no $(basename "$DMG") in $PREBUILT"
  [ -n "$found" ] || die "--prebuilt: no Mac daemon of $VERSION in $PREBUILT"
  for f in "$PREBUILT"/*.sha256; do
    [ -e "$f" ] || continue
    (cd "$PREBUILT" && shasum -a 256 -c "$(basename "$f")" >/dev/null) || die "--prebuilt: $(basename "${f%.sha256}") does not match its .sha256"
  done
  if [ "$RESUME" = 0 ]; then
    for f in "${RELEASE_FILES[@]}"; do
      [ -e "$PREBUILT/release-files/$f" ] || die "--prebuilt: no release-files/$f in $PREBUILT"
      cp "$PREBUILT/release-files/$f" "$ROOT/$f"
    done
  fi
  [ "$(workspace_version)" = "$VERSION" ] || die "--prebuilt: the version files hold $(workspace_version), not $VERSION"
  "$ROOT/scripts/release-check.sh" "$VERSION" || die "release metadata of $VERSION is not final"
  mkdir -p "$DIST"
  [ "$PREBUILT" = "$(cd "$DIST" && pwd)" ] || cp "$PREBUILT/$(basename "$DMG")" "$found" "$DIST/"
  DAEMON="$DIST/$(basename "$found")"
  log "using the build of $VERSION in $PREBUILT: $(basename "$DMG"), $(basename "$DAEMON")"
}

# keep_release_files: the version files of the release commit into dist/release-files/, for
# the run that publishes this build (--prebuilt).
keep_release_files() {
  local f
  rm -rf "$DIST/release-files" && mkdir -p "$DIST/release-files"
  for f in "${RELEASE_FILES[@]}"; do cp "$ROOT/$f" "$DIST/release-files/$f"; done
}

# set_version_and_build: sets the version and the CHANGELOG date in the working tree, checks the
# release metadata, builds and checks the DMG and the daemon. Sets DAEMON.
set_version_and_build() {
  if [ -n "$PREBUILT" ]; then take_prebuilt; return 0; fi
  need cargo
  release_ops
  log "setting version $VERSION"
  set_workspace_version "$VERSION"
  cargo_env
  cargo update --workspace --offline --manifest-path "$ROOT/Cargo.toml"
  # "## <version> (unreleased)" gets today's date (UTC).
  # (Through a temporary file: sed -i differs between macOS and Linux.)
  local dated
  dated="$(mktemp)"
  sed "s/^## ${VERSION//./\\.} (unreleased)\$/## $VERSION ($(date -u +%Y-%m-%d))/" CHANGELOG.md >"$dated"
  cat "$dated" >CHANGELOG.md
  rm -f "$dated"
  "$ROOT/scripts/release-check.sh" "$VERSION" || die "release metadata of $VERSION is not final"

  "$ROOT/scripts/build-dmg.sh"
  local same_as=()
  if [ -n "$SAME_AS" ]; then same_as=(--same-as "$SAME_AS"); fi
  "$ROOT/scripts/test-dmg.sh" "$DMG" ${same_as[@]+"${same_as[@]}"}
  DAEMON="$("$ROOT/scripts/build-daemon.sh" | tail -n 1)"
  [ -f "$DMG" ] && [ -f "$DAEMON" ] || die "build outputs are missing"
}

if [ "$DRY_RUN" = 1 ]; then
  cat >&2 <<PLAN
Release plan (dry run, nothing changed):
  version        $CURRENT -> $VERSION (Cargo.toml [workspace.package])
  build          scripts/build-dmg.sh, scripts/test-dmg.sh, scripts/build-daemon.sh
  commit         chore(release): $TAG  (Cargo.toml, Cargo.lock, CHANGELOG.md date)
  push           origin HEAD:main, then tag $TAG
  GitHub         ${REPO:-?}: release $TAG$([[ "$VERSION" == *-* ]] && printf ' (prerelease)')
                 assets: $NAME-$VERSION.dmg, daemon (GitHub shows their SHA-256); the
                 Linux daemon: ${LINUX_DAEMON:-from its own job}
  build          $(if [ -n "$PREBUILT" ]; then printf 'none: the files of %s' "$PREBUILT"; elif [ "$BUILD_ONLY" = 1 ]; then printf 'only (--build-only): nothing is published by this run'; else printf 'here'; fi)
  Windows        $(if [ -n "$WINDOWS_INSTALLER" ]; then printf '%s and %s more file(s), on the GitHub Release and the server with the DMG' "$(basename "$WINDOWS_INSTALLER")" "${#ASSETS[@]}"; elif [ "$WITHOUT_WINDOWS" = 1 ]; then printf 'none, on purpose (--without-windows)'; else printf 'missing: a real run would stop'; fi)
  notes          ${NOTES:-generated by GitHub}
  server         $([ "$SERVER_UPLOAD" = 1 ] && printf 'PUT %s/v1/admin/releases/%s (%s)' "$BASE" "$VERSION" "$ENV_NAME" || printf 'none: a prerelease is not offered as an app update')
  admin secret   $SECRET_FILE ($([ -s "$SECRET_FILE" ] && echo present || echo missing))
PLAN
  if [ "$problems" -gt 0 ]; then log "dry run found $problems problem(s); a real run would stop"; else log "dry run: all checks passed"; fi
  if [ "$BUILD" = 1 ]; then
    trap 'restore' EXIT
    set_version_and_build
    [ "$BUILD_ONLY" = 0 ] || keep_release_files
    log "dry run: built and checked $(basename "$DMG") and $(basename "$DAEMON") in $DIST; nothing committed, tagged or uploaded"
  elif [ -n "$PREBUILT" ]; then
    # The files a real run would publish must be there and whole.
    trap 'restore' EXIT
    if (take_prebuilt); then log "dry run: the build in $PREBUILT is complete"; else log "dry run: the build in $PREBUILT cannot be published"; exit 1; fi
  fi
  exit 0
fi

if [ "$BUILD_ONLY" = 1 ]; then
  trap 'restore' EXIT
  set_version_and_build
  keep_release_files
  log "built and checked $(basename "$DMG") and $(basename "$DAEMON") in $DIST; nothing committed, tagged or uploaded (publish with --prebuilt $DIST)"
  exit 0
fi

# Restore the version files on any failure before the release commit.
trap 'restore' EXIT
set_version_and_build

if [ "$RESUME" = 1 ]; then
  # The commit and the tag are on main already.
  trap - EXIT
  log "resuming the release of $TAG: no commit, no tag; creating the GitHub Release from the tag"
else
  if ! git diff --quiet -- Cargo.toml Cargo.lock CHANGELOG.md; then
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit --quiet -m "chore(release): $TAG"
  fi
  trap - EXIT
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
fi

log "creating GitHub release $TAG"
gh_args=(release create "$TAG" "$DMG" "$DAEMON" --verify-tag --title "$NAME $VERSION")
# Both systems in one step: the release never exists with the DMG alone.
if [ -n "$WINDOWS_INSTALLER" ]; then gh_args+=("$WINDOWS_INSTALLER" ${ASSETS[@]+"${ASSETS[@]}"}); fi
if [ -n "$LINUX_DAEMON" ]; then gh_args+=("$LINUX_DAEMON"); fi
if [ -n "$NOTES" ]; then gh_args+=(--notes-file "$NOTES"); else gh_args+=(--generate-notes); fi
if [[ "$VERSION" == *-* ]]; then gh_args+=(--prerelease); fi
gh "${gh_args[@]}"

upload_to_server
if [ -n "$WINDOWS_INSTALLER" ]; then
  use_platform windows "$WINDOWS_INSTALLER"
  upload_to_server
fi
if [ -n "$LINUX_DAEMON" ]; then
  use_platform linux-x86_64 "$LINUX_DAEMON"
  upload_to_server
fi
log "release $TAG done"
