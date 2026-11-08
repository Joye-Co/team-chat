# Shared helpers for the packaging scripts. Source it; do not run it.
# shellcheck shell=bash

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${DIST:-$ROOT/dist}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

need() {
  local c
  for c in "$@"; do command -v "$c" >/dev/null 2>&1 || die "missing tool: $c"; done
}

# product <key>: a string field of product.json.
product() {
  need jq
  local v
  v="$(jq -er --arg k "$1" '.[$k] | strings' "$ROOT/product.json")" || die "product.json has no string field '$1'"
  printf '%s' "$v"
}

# workspace_version [file]: the version in the [workspace.package] table of Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") }
    in_pkg && /^version[[:space:]]*=/ { gsub(/.*=[[:space:]]*"|".*/, ""); print; exit }
  ' "${1:-$ROOT/Cargo.toml}"
}

# set_workspace_version <version> [file]: rewrite only that line.
set_workspace_version() {
  local file="${2:-$ROOT/Cargo.toml}" tmp
  tmp="$(mktemp)"
  awk -v ver="$1" '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") }
    in_pkg && !done && /^version[[:space:]]*=/ { print "version = \"" ver "\""; done = 1; next }
    { print }
    END { if (!done) exit 3 }
  ' "$file" >"$tmp" || { rm -f "$tmp"; die "no version line in [workspace.package] of $file"; }
  cat "$tmp" >"$file"
  rm -f "$tmp"
}

valid_semver() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]
}

# The versions the server accepts for an app update: MAJOR.MINOR.PATCH without leading zeros,
# no prerelease. Keep in step with ReleaseVersion in packages/protocol/src/updates.ts.
valid_release_version() {
  [[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

cargo_env() {
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  need cargo
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
  if [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then export RUSTC_WRAPPER=sccache; fi
}

# bin_name <package>: the first binary target of a workspace package.
bin_name() {
  cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/Cargo.toml" |
    jq -er --arg p "$1" '[.packages[] | select(.name == $p) | .targets[] | select(.kind | index("bin")) | .name][0] // empty' ||
    die "package '$1' has no binary target"
}

write_sha256() {
  (cd "$(dirname "$1")" && shasum -a 256 "$(basename "$1")" >"$(basename "$1").sha256")
}

# Code signing (build-dmg.sh, test-dmg.sh). The key and the certificate are plain files; no
# keychain is used.
signing_dir() { printf '%s' "${SIGNING_DIR:-$HOME/.config/team-chat/signing}"; }

# signing_mode: "cert" when the key and certificate files exist (and SIGNING is not adhoc),
# else "adhoc".
signing_mode() {
  local d
  d="$(signing_dir)"
  if [ "${SIGNING:-}" != adhoc ] && [ -s "$d/key.pem" ] && [ -s "$d/cert.pem" ]; then echo cert; else echo adhoc; fi
}

# signing_cert_sha1: lowercase SHA-1 of the certificate (DER), as a requirement names it.
signing_cert_sha1() {
  openssl x509 -in "$(signing_dir)/cert.pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g' | tr 'A-F' 'a-f'
}

# rcodesign_sign <log> <args...>: signs with the key files; the log stays quiet unless it fails.
rcodesign_sign() {
  local logf="$1" d
  shift
  d="$(signing_dir)"
  if ! rcodesign sign --pem-file "$d/key.pem" --pem-file "$d/cert.pem" --timestamp-url none "$@" >"$logf" 2>&1; then
    tail -n 20 "$logf" >&2
    die "rcodesign sign failed"
  fi
}

# designated_requirement <path>: the designated requirement text of a signed file.
designated_requirement() {
  codesign -d -r- "$1" 2>/dev/null | sed -n 's/^\(# \)\{0,1\}designated => //p' | head -n 1
}

# ops_dir: the private ops checkout of the operator (deploy config, server URLs, character
# pack). TC_OPS_DIR, else ~/.config/team-chat/ops. Empty when neither exists.
ops_dir() {
  local d="${TC_OPS_DIR:-${TEAM_CHAT_CONFIG_DIR:-$HOME/.config/team-chat}/ops}"
  if [ -d "$d" ]; then printf '%s' "$d"; fi
}

# env_base <env>: the API base URL of a deployed env, from <ops>/deploy/servers.json.
# CHAT_API_BASE overrides it.
env_base() {
  local base="${CHAT_API_BASE:-}" ops
  if [ -z "$base" ]; then
    ops="$(ops_dir)"
    if [ -n "$ops" ] && [ -f "$ops/deploy/servers.json" ]; then
      base="$(jq -r --arg e "$1" '.[$e] // empty' "$ops/deploy/servers.json")"
    fi
  fi
  [ -n "$base" ] || die "no API base URL for env '$1': set CHAT_API_BASE, or TC_OPS_DIR with deploy/servers.json"
  printf '%s' "${base%/}"
}

# deploy_wrangler_config <server|monitor>: copies <ops>/deploy/<name>.wrangler.jsonc next to the
# Worker as wrangler.deploy.jsonc (git ignores it) and prints that file name. The committed
# wrangler.jsonc has no deployed env.
deploy_wrangler_config() {
  local ops dir
  ops="$(ops_dir)"
  [ -n "$ops" ] && [ -f "$ops/deploy/$1.wrangler.jsonc" ] || die "no $1 deploy config: set TC_OPS_DIR (needs deploy/$1.wrangler.jsonc)"
  case "$1" in server) dir="$ROOT/server" ;; monitor) dir="$ROOT/ops/monitor" ;; *) die "unknown worker '$1'" ;; esac
  cp "$ops/deploy/$1.wrangler.jsonc" "$dir/wrangler.deploy.jsonc"
  printf 'wrangler.deploy.jsonc'
}

# health_version <base>: the version that GET <base>/v1/health reports (empty when unreachable).
health_version() {
  curl -sS -m 15 "$1/v1/health" 2>/dev/null | jq -r '.version // empty' 2>/dev/null || true
}

# placeholder_version <v>: true for the static VERSION values in server/wrangler.jsonc
# (0.0.0, 0.0.0-dev, 0.0.0-staging...): a Worker that reports one was not deployed by
# scripts/deploy-server.sh.
placeholder_version() {
  [[ "$1" == 0.0.0 || "$1" == 0.0.0-* ]]
}
