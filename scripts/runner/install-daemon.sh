#!/usr/bin/env bash
# Installs the machine daemon of a Linux host from a GitHub release: the asset
# <slug>-daemon-<version>-x86_64-unknown-linux-gnu, checked against the SHA-256 digest GitHub
# keeps for it. Only when the release has no Linux daemon (or this is not x86_64) it builds the
# daemon from this checkout with scripts/build-daemon.sh, which needs cargo.
#
# The new file is written next to the old one and renamed over it, so a running daemon keeps
# its runs: it sees the new file and restarts on it once no run is active. Afterwards the
# daemon keeps itself up to date from the server (self_update in daemon.toml).
#
# Usage: scripts/runner/install-daemon.sh [--version latest|<x.y.z>] [--dest <file>] [--owner <user>]
#   --version  the release to install (default latest, the newest that is not a prerelease)
#   --dest     where the daemon goes (default ~<owner>/.local/bin/<slug>-daemon); its folder
#              must stay writable by the owner, or the daemon cannot update itself
#   --owner    the user that runs the daemon (default the current user); root chowns to it
# Env: DAEMON_REPO (owner/name; default: the GitHub repository of this checkout's origin),
#      GITHUB_API (default https://api.github.com), DAEMON_ARCH (default uname -m; for tests).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

version=latest
dest=""
owner="$(id -un)"
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --dest) dest="${2:-}"; shift 2 ;;
    --owner) owner="${2:-}"; shift 2 ;;
    -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
for c in curl jq sha256sum; do command -v "$c" >/dev/null 2>&1 || die "missing tool: $c"; done
version="${version#v}"
[ "$version" = latest ] || [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "--version must be latest or x.y.z"
slug="$(jq -er '.slug | strings' "$root/product.json")" || die "no slug in $root/product.json"
own_dirs=()
if [ -z "$dest" ]; then
  home="$(getent passwd "$owner" | cut -d: -f6)"
  [ -n "$home" ] || die "no home folder for user $owner"
  dest="$home/.local/bin/$slug-daemon"
  own_dirs=("$home/.local" "$home/.local/bin")
fi
api="${GITHUB_API:-https://api.github.com}"
repo="${DAEMON_REPO:-}"
if [ -z "$repo" ]; then
  origin="$(git -C "$root" remote get-url origin 2>/dev/null || true)"
  repo="$(sed -nE 's#^(https://github\.com/|git@github\.com:)([^/]+/[^/]+)$#\2#p' <<<"$origin")"
  repo="${repo%.git}"
fi

# put <file>: installs <file> (checked already) at $dest for $owner, by a rename in its folder.
put() {
  local dir new
  dir="$(dirname "$dest")"
  local d
  for d in ${own_dirs[@]+"${own_dirs[@]}"}; do
    [ -d "$d" ] || install -d -m 755 "$d"
    if [ "$(id -u)" = 0 ]; then chown "$owner:" "$d"; fi
  done
  mkdir -p "$dir"
  new="$dest.new"
  cp "$1" "$new"
  chmod 755 "$new"
  if [ "$(id -u)" = 0 ] && [ "$owner" != root ]; then chown "$owner:" "$new"; fi
  mv -f "$new" "$dest"
  if [ "$(id -u)" = 0 ] && [ "$owner" != root ] && ! su -s /bin/sh -c "test -w '$dir'" "$owner"; then
    log "warning: $owner cannot write $dir, so the daemon cannot update itself there"
  fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"; rm -f "$dest.new"' EXIT

url="" digest=""
if [ "${DAEMON_ARCH:-$(uname -m)}" = x86_64 ] && [ -n "$repo" ]; then
  if [ "$version" = latest ]; then what="releases/latest"; else what="releases/tags/v$version"; fi
  if curl -fsSL --max-time 60 -H 'Accept: application/vnd.github+json' -o "$tmp/release.json" "$api/repos/$repo/$what"; then
    pattern="^${slug//./\\.}-daemon-[0-9]+\\.[0-9]+\\.[0-9]+-x86_64-unknown-linux-gnu\$"
    url="$(jq -r --arg p "$pattern" 'first(.assets[]? | select(.name | test($p))) | .browser_download_url // empty' "$tmp/release.json")"
    digest="$(jq -r --arg p "$pattern" 'first(.assets[]? | select(.name | test($p))) | .digest // empty' "$tmp/release.json")"
    tag="$(jq -r '.tag_name // empty' "$tmp/release.json")"
  else
    log "no release $version in $repo"
  fi
fi

if [ -n "$url" ]; then
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || die "release $tag has no SHA-256 digest for its Linux daemon"
  want="${digest#sha256:}"
  if [ -f "$dest" ] && [ "$(sha256sum "$dest" | cut -d' ' -f1)" = "$want" ]; then
    log "the daemon of $tag is installed already at $dest"
    exit 0
  fi
  case "$url" in https://* | http://127.0.0.1:* | http://localhost:*) ;; *) die "refusing a download that is not https: $url" ;; esac
  log "downloading the daemon of $tag"
  curl -fsSL --max-time 900 -o "$tmp/daemon" "$url"
  have="$(sha256sum "$tmp/daemon" | cut -d' ' -f1)"
  [ "$have" = "$want" ] || die "the download does not match the digest of release $tag"
  put "$tmp/daemon"
  log "installed the daemon of $tag at $dest"
  exit 0
fi

# No released Linux daemon: build it from this checkout.
command -v cargo >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/cargo" ] ||
  die "no released Linux daemon${repo:+ in $repo} and no cargo to build one; install Rust (rustup) or name a release with --version"
log "no released Linux daemon; building it from $root"
built="$(DIST="$tmp/dist" "$root/scripts/build-daemon.sh" | tail -n 1)"
(cd "$(dirname "$built")" && shasum -a 256 -c "$(basename "$built").sha256" >/dev/null) || die "the built daemon does not match its .sha256"
put "$built"
log "installed the daemon built from this checkout at $dest"
