#!/usr/bin/env bash
# Downloads the Windows installer of the newest published release other than <version>, for
# scripts/test-windows-installer.sh --same-as: the new installer must be one that apps
# installed from the last release accept as an update.
#
# Usage: scripts/previous-windows-installer.sh <version> <dir>
# Prints the path of the downloaded installer, or nothing when no earlier release has one (the
# first release for Windows). Any other failure (GitHub not reachable, a download that breaks)
# is an error: a release must not go out with this check skipped by accident.
# Needs gh, logged in (GH_TOKEN in a workflow).
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need gh jq

VERSION="${1:?usage: previous-windows-installer.sh <version> <dir>}"
OUT="${2:?usage: previous-windows-installer.sh <version> <dir>}"
VERSION="${VERSION#v}"

# Newest first. The release of <version> itself may exist already: the macOS job creates it
# before the Windows job gets here.
tags="$(gh release list --exclude-drafts --exclude-pre-releases --limit 50 --json tagName -q '.[].tagName')" ||
  die "cannot list the releases"
for tag in $tags; do
  [ "$tag" != "v$VERSION" ] || continue
  assets="$(gh release view "$tag" --json assets -q '.assets[].name')" || die "cannot read release $tag"
  name="$(printf '%s\n' "$assets" | grep -E -- '-setup\.exe$' | head -n 1 || true)"
  [ -n "$name" ] || continue
  mkdir -p "$OUT"
  gh release download "$tag" --pattern "$name" --dir "$OUT" --clobber >&2 ||
    die "cannot download $name of release $tag"
  [ -s "$OUT/$name" ] || die "the download of $name of release $tag is empty"
  log "the previous Windows installer is $name of $tag"
  printf '%s\n' "$OUT/$name"
  exit 0
done
log "no earlier release has a Windows installer: nothing to compare with"
