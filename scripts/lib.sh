# Shared helpers for the packaging scripts. Source it; do not run it.
# shellcheck shell=bash

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${DIST:-$ROOT/dist}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

# tool_hint <tool>: where a tool that does not ship with the system comes from, or nothing.
tool_hint() {
  case "$1" in
    cargo | rustc) printf 'rustup, https://rustup.rs; rust-toolchain.toml names the toolchain' ;;
    bun) printf 'https://bun.sh' ;;
    rcodesign) printf 'cargo install apple-codesign' ;;
    jq | gh | shellcheck) printf 'for example: brew install %s' "$1" ;;
  esac
}

# need <tool>...: stops when one is missing, after naming every missing one of the call and
# where to get it.
need() {
  local c hint missing=() lines=""
  for c in "$@"; do command -v "$c" >/dev/null 2>&1 || missing+=("$c"); done
  [ "${#missing[@]}" -gt 0 ] || return 0
  if [ "${#missing[@]}" = 1 ]; then
    hint="$(tool_hint "${missing[0]}")"
    die "missing tool: ${missing[0]}${hint:+ ($hint)}"
  fi
  for c in "${missing[@]}"; do
    hint="$(tool_hint "$c")"
    lines+=$'\n'"  $c${hint:+ ($hint)}"
  done
  die "missing tools:$lines"
}

# usage: prints the comment block at the top of the running script (from line 2 to the first
# line that is not a comment) without the "# ". Every script's --help is this, so no script
# keeps line numbers of itself.
usage() {
  awk 'NR == 1 { next } !/^#/ { exit } { sub(/^# ?/, ""); print }' "$0"
}

# opt_value <option> [value]: the value of an option; stops when the option came without one.
# In an option loop: X="$(opt_value "$@")"; shift 2
opt_value() {
  [ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value"
  printf '%s' "$2"
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

# default_target_dir: where cargo builds when CARGO_TARGET_DIR is not set: one folder per
# worktree for every script, so a merged branch has one folder to delete.
default_target_dir() { printf '%s' "$ROOT/target"; }

cargo_env() {
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  need cargo
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(default_target_dir)}"
  if [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then export RUSTC_WRAPPER=sccache; fi
}

# warn_low_disk: one warning when the home volume has less than 60 GB free (a desktop debug
# target takes 10-30 GB). It never stops a build, and says nothing in CI.
warn_low_disk() {
  [ -z "${CI:-}" ] || return 0
  local free_kb
  free_kb="$(df -Pk "$HOME" 2>/dev/null | awk 'NR == 2 { print $4 }')" || return 0
  [[ "$free_kb" =~ ^[0-9]+$ ]] || return 0
  if [ "$free_kb" -lt $((60 * 1024 * 1024)) ]; then
    log "warning: $((free_kb / 1024 / 1024)) GB free on the disk of $HOME, less than the 60 GB a build should start with; delete the target folders of merged branches"
  fi
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

# ops_dir_path: where the ops checkout is or would be: TC_OPS_DIR, else ~/.config/team-chat/ops.
ops_dir_path() {
  printf '%s' "${TC_OPS_DIR:-${TEAM_CHAT_CONFIG_DIR:-$HOME/.config/team-chat}/ops}"
}

# ops_dir: the private ops checkout of the operator (deploy config, server URLs, character
# pack). TC_OPS_DIR, else ~/.config/team-chat/ops. Empty when neither exists.
ops_dir() {
  local d
  d="$(ops_dir_path)"
  if [ -d "$d" ]; then printf '%s' "$d"; fi
}

# wrangler <args...>: the repository's wrangler (bunx, from server/node_modules). WRANGLER names
# another program to run instead (the tests use a fake one).
wrangler() {
  if [ -n "${WRANGLER:-}" ]; then "$WRANGLER" "$@"; else bunx wrangler "$@"; fi
}

# workers_dev_url: the first https://<name>.<subdomain>.workers.dev address in its input (what
# `wrangler deploy` prints), or nothing.
workers_dev_url() {
  grep -Eo 'https://[A-Za-z0-9.-]+\.workers\.dev' | head -n 1 || true
}

# record_server_url <env> <url>: puts the env's base URL into <ops>/deploy/servers.json (made
# when missing), keeping the other envs.
record_server_url() {
  local ops file tmp
  ops="$(ops_dir_path)"
  file="$ops/deploy/servers.json"
  mkdir -p "$ops/deploy"
  tmp="$(mktemp)"
  if [ -s "$file" ]; then
    jq --arg e "$1" --arg u "${2%/}" '.[$e] = $u' "$file" >"$tmp" || { rm -f "$tmp"; die "cannot read $file"; }
  else
    jq -n --arg e "$1" --arg u "${2%/}" '{($e): $u}' >"$tmp"
  fi
  cat "$tmp" >"$file"
  rm -f "$tmp"
}

# servers_json: the server URLs as JSON ({ "e2e", "staging", "production" }): TC_SERVERS_JSON
# when set (CI), else <ops>/deploy/servers.json. Fails when there are none.
servers_json() {
  local ops
  if [ -n "${TC_SERVERS_JSON:-}" ]; then
    printf '%s' "$TC_SERVERS_JSON" | jq -e 'if type == "object" then . else error("not an object") end'
    return
  fi
  ops="$(ops_dir)"
  [ -n "$ops" ] && [ -f "$ops/deploy/servers.json" ] && jq -e 'if type == "object" then . else error("not an object") end' "$ops/deploy/servers.json"
}

# env_base <env>: the API base URL of a deployed env, from servers_json. CHAT_API_BASE overrides it.
env_base() {
  local base="${CHAT_API_BASE:-}"
  if [ -z "$base" ]; then
    base="$(servers_json 2>/dev/null | jq -r --arg e "$1" '.[$e] // empty')" || base=""
  fi
  [ -n "$base" ] || die "no API base URL for env '$1': set CHAT_API_BASE, TC_SERVERS_JSON, or TC_OPS_DIR with deploy/servers.json"
  printf '%s' "${base%/}"
}

# deploy_wrangler_config <server|monitor>: copies <ops>/deploy/<name>.wrangler.jsonc next to the
# Worker as wrangler.deploy.jsonc (git ignores it) and prints that file name. The committed
# wrangler.jsonc has no deployed env.
deploy_wrangler_config() {
  local ops dir
  ops="$(ops_dir)"
  if [ -z "$ops" ] || [ ! -f "$ops/deploy/$1.wrangler.jsonc" ]; then
    if [ "$1" = server ]; then
      die "no server deploy config at $(ops_dir_path)/deploy/server.wrangler.jsonc: run scripts/setup-server.sh first (docs/SELF-HOSTING.md), or set TC_OPS_DIR to your ops checkout"
    fi
    die "no $1 deploy config: set TC_OPS_DIR (needs deploy/$1.wrangler.jsonc)"
  fi
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
