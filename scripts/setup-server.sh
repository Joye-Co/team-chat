#!/usr/bin/env bash
# Sets up and deploys your own server on your Cloudflare account, from a checkout of this
# repository: the walk-through is docs/SELF-HOSTING.md (docs/SELF-HOSTING.zh.md).
#
# Usage: scripts/setup-server.sh [production|staging|e2e] [--owner-name <name>] [--sign-in-code]
#                                [--url <base>] [--worker <name>] [--d1 <name>] [--bucket <name>]
#   production       the env to set up (default). Names: Worker chat-api-prod, D1 chat-prod,
#                    R2 bucket chat-files-prod (staging and e2e: chat-api-<env>, chat-<env>,
#                    chat-files-<env>); --worker, --d1 and --bucket choose others on the first run.
#   --owner-name     the first owner's name (asked for when missing and a terminal is attached)
#   --sign-in-code   make a new single-use sign-in code for the owner (a first setup makes one)
#   --url            the server's base URL, when it is not the workers.dev address of the Worker
#
# In order, each step checked first, so running it again repairs what is missing and changes
# nothing else:
#   1. prerequisites: git, curl, jq, openssl, bun, Node.js 22+, the repository's packages, a
#      signed-in wrangler; a message for each one that is missing
#   2. the ops folder (TC_OPS_DIR, else ~/.config/team-chat/ops) and its deploy/ folder
#   3. the D1 database (location hint oc) and the R2 bucket
#   4. deploy/server.wrangler.jsonc with an env block for this env (an existing file is kept)
#   5. secret files in ~/.config/team-chat/<env>/ (folder 700, files 600): admin-secret and
#      hook-secret-key, made when missing; never printed
#   6. scripts/deploy-server.sh <env>: D1 migrations, the Worker, /v1/health (a first deploy
#      writes the URL into deploy/servers.json)
#   7. Worker secrets ADMIN_SECRET and HOOK_SECRET_KEY, put from the files when the Worker
#      lacks them (a HOOK_SECRET_KEY the Worker has is never replaced: it encrypts stored keys)
#   8. the first owner (POST /v1/admin/bootstrap) when there is no owner-token file, and a
#      sign-in code for the owner
#   9. the next step
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

ENV_NAME=""
OWNER_NAME=""
SIGN_IN_CODE=0
URL=""
WORKER=""
D1_NAME=""
BUCKET=""
while [ $# -gt 0 ]; do
  case "$1" in
    --owner-name) OWNER_NAME="${2:-}"; [ -n "$OWNER_NAME" ] || die "--owner-name needs a name"; shift 2 ;;
    --sign-in-code) SIGN_IN_CODE=1; shift ;;
    --url) URL="${2:-}"; [[ "$URL" =~ ^https?://[^[:space:]]+$ ]] || die "--url needs an http(s) address"; shift 2 ;;
    --worker) WORKER="${2:-}"; [ -n "$WORKER" ] || die "--worker needs a name"; shift 2 ;;
    --d1) D1_NAME="${2:-}"; [ -n "$D1_NAME" ] || die "--d1 needs a name"; shift 2 ;;
    --bucket) BUCKET="${2:-}"; [ -n "$BUCKET" ] || die "--bucket needs a name"; shift 2 ;;
    -h | --help) sed -n '2,29p' "$0"; exit 0 ;;
    -*) die "unknown option $1 (see scripts/setup-server.sh --help)" ;;
    *) [ -z "$ENV_NAME" ] || die "more than one env given"; ENV_NAME="$1"; shift ;;
  esac
done
ENV_NAME="${ENV_NAME:-production}"
case "$ENV_NAME" in
  production) suffix=prod ;;
  staging | e2e) suffix="$ENV_NAME" ;;
  *) die "the env must be production, staging or e2e (default production)" ;;
esac
WORKER="${WORKER:-chat-api-$suffix}"
D1_NAME="${D1_NAME:-chat-$suffix}"
BUCKET="${BUCKET:-chat-files-$suffix}"
# admin.ts reads its files here.
SECRETS="$HOME/.config/team-chat/$ENV_NAME"
OPS="$(ops_dir_path)"
CONFIG_FILE="$OPS/deploy/server.wrangler.jsonc"
BOOTSTRAP_WAIT="${BOOTSTRAP_WAIT_SECONDS:-60}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

step() { printf '\n== %s\n' "$*" >&2; }
# json_part: the JSON of a wrangler command's output, without a banner line printed before it.
json_part() { sed -n '/^[[{]/,$p'; }

# ---------------------------------------------------------------------------------------------
step "1. prerequisites"
missing=0
miss() { printf 'missing: %s\n' "$*" >&2; missing=1; }
for t in git curl jq openssl; do
  command -v "$t" >/dev/null 2>&1 || miss "$t (Ubuntu: sudo apt-get install -y git curl jq openssl unzip)"
done
if ! command -v bun >/dev/null 2>&1 || ! bun -e 'process.exit(typeof Bun.JSONC?.parse === "function" ? 0 : 1)' >/dev/null 2>&1; then
  miss "bun 1.3 or later (Ubuntu: sudo apt-get install -y unzip; curl -fsSL https://bun.sh/install | bash -s bun-v1.3.10; then open a new shell)"
fi
if [ -z "${WRANGLER:-}" ]; then
  node_major="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)"
  if ! [[ "$node_major" =~ ^[0-9]+$ ]] || [ "$node_major" -lt 22 ]; then
    miss "Node.js 22 or later, which wrangler needs (Ubuntu: see docs/SELF-HOSTING.md step 1)"
  fi
  if command -v bun >/dev/null 2>&1 && [ ! -x "$ROOT/server/node_modules/.bin/wrangler" ]; then
    miss "the repository's packages (run: bun install --frozen-lockfile, at the repository root)"
  fi
fi
[ "$missing" = 0 ] || die "install what is missing above, then run scripts/setup-server.sh again"

cd "$ROOT/server"
whoami_json="$(wrangler whoami --json 2>/dev/null | json_part || true)"
if [ "$(printf '%s' "$whoami_json" | jq -r '.loggedIn // false' 2>/dev/null)" != true ]; then
  die "wrangler is not signed in to Cloudflare: run 'cd server && bunx wrangler login --device' (or set CLOUDFLARE_API_TOKEN, see docs/SELF-HOSTING.md step 2), then run this again"
fi
accounts="$(printf '%s' "$whoami_json" | jq -r '.accounts | length' 2>/dev/null || echo 0)"
if [ "${accounts:-0}" -gt 1 ] && [ -z "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
  die "your Cloudflare login has $accounts accounts: set CLOUDFLARE_ACCOUNT_ID to the one to use ('bunx wrangler whoami' lists them), then run this again"
fi
log "tools found, wrangler signed in"

# ---------------------------------------------------------------------------------------------
step "2. ops folder"
if [ -d "$OPS/deploy" ]; then
  log "ops folder: $OPS"
else
  mkdir -p "$OPS/deploy"
  chmod 700 "$OPS"
  log "made the ops folder $OPS (keep it private; it is your deployment, not part of the repository)"
fi

# ---------------------------------------------------------------------------------------------
step "3. D1 database and R2 bucket"
d1_id() { wrangler d1 list --json 2>/dev/null | json_part | jq -r --arg n "$1" '.[] | select(.name == $n) | .uuid' | head -n 1; }
config_has_env() {
  [ -f "$CONFIG_FILE" ] &&
    bun -e 'const c = Bun.JSONC.parse(await Bun.file(process.argv[1]).text()); process.exit(c.env?.[process.argv[2]] ? 0 : 1)' "$CONFIG_FILE" "$ENV_NAME"
}
HEADER="made by scripts/setup-server.sh"
if config_has_env; then
  log "$CONFIG_FILE already has env.$ENV_NAME: its D1 database and R2 bucket are used"
elif [ -f "$CONFIG_FILE" ] && ! head -n 1 "$CONFIG_FILE" | grep -q "$HEADER"; then
  die "$CONFIG_FILE has no env.$ENV_NAME and was not made by this script, so it is left as it is: add the env block by hand (docs/ENVIRONMENTS.md, Ops checkout), or move the file away and run this again"
else
  DB_ID="$(d1_id "$D1_NAME")"
  if [ -n "$DB_ID" ]; then
    log "D1 database $D1_NAME exists"
  else
    log "creating D1 database $D1_NAME (location hint oc)"
    wrangler d1 create "$D1_NAME" --location oc >/dev/null
    DB_ID="$(d1_id "$D1_NAME")"
    [ -n "$DB_ID" ] || die "created D1 database $D1_NAME, but 'wrangler d1 list' does not show it"
  fi
  if wrangler r2 bucket info "$BUCKET" >/dev/null 2>&1; then
    log "R2 bucket $BUCKET exists"
  else
    log "creating R2 bucket $BUCKET (location hint oc)"
    wrangler r2 bucket create "$BUCKET" --location oc >/dev/null ||
      die "could not create R2 bucket $BUCKET: R2 must be turned on for the account once (Cloudflare dashboard, R2 Object Storage)"
  fi

  # -------------------------------------------------------------------------------------------
  step "4. Worker config"
  # shellcheck disable=SC2016 # JavaScript, not shell
  bun -e '
    const [src, out, env, worker, d1, id, bucket] = process.argv.slice(1);
    const top = Bun.JSONC.parse(await Bun.file(src).text());
    const exists = await Bun.file(out).exists();
    const cfg = exists ? Bun.JSONC.parse(await Bun.file(out).text()) : top;
    // Wrangler does not inherit bindings, vars or these settings into an env: each env lists them.
    cfg.env = { ...(cfg.env ?? {}), [env]: {
      name: worker,
      vars: { ENVIRONMENT: env, VERSION: `0.0.0-${env}` },
      d1_databases: [{ ...top.d1_databases[0], database_name: d1, database_id: id }],
      r2_buckets: [{ ...top.r2_buckets[0], bucket_name: bucket }],
      durable_objects: top.durable_objects,
      observability: top.observability,
      triggers: top.triggers,
    } };
    const head = "// API Worker config of this deployment, made by scripts/setup-server.sh from server/wrangler.jsonc.\n" +
      "// scripts/deploy-server.sh copies it to server/wrangler.deploy.jsonc. Keep it private.\n";
    await Bun.write(out, head + JSON.stringify(cfg, null, 2) + "\n");
  ' "$ROOT/server/wrangler.jsonc" "$CONFIG_FILE" "$ENV_NAME" "$WORKER" "$D1_NAME" "$DB_ID" "$BUCKET"
  log "wrote env.$ENV_NAME into $CONFIG_FILE (Worker $WORKER, D1 $D1_NAME, R2 $BUCKET)"
fi
if [ -n "$URL" ]; then
  record_server_url "$ENV_NAME" "$URL"
  log "server URL for $ENV_NAME: $URL (in $OPS/deploy/servers.json)"
fi

# ---------------------------------------------------------------------------------------------
step "5. secret files"
mkdir -p "$SECRETS"
chmod 700 "$SECRETS"
new_secret() { (umask 077; openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n' >"$1"); chmod 600 "$1"; }
had_admin_file=1
had_hook_file=1
[ -s "$SECRETS/admin-secret" ] || { had_admin_file=0; new_secret "$SECRETS/admin-secret"; log "made $SECRETS/admin-secret"; }
[ -s "$SECRETS/hook-secret-key" ] || { had_hook_file=0; }
log "secret files in $SECRETS (never printed)"

# ---------------------------------------------------------------------------------------------
step "6. deploy"
"$ROOT/scripts/deploy-server.sh" "$ENV_NAME"
BASE="$(env_base "$ENV_NAME")"

# ---------------------------------------------------------------------------------------------
step "7. Worker secrets"
cd "$ROOT/server"
secret_names="$(wrangler secret list -c wrangler.deploy.jsonc --env "$ENV_NAME" --format json 2>/dev/null | json_part | jq -r '.[].name' 2>/dev/null)" ||
  die "could not list the secrets of Worker $WORKER ('bunx wrangler -c wrangler.deploy.jsonc secret list --env $ENV_NAME' inside server/ says why)"
has_secret() { printf '%s\n' "$secret_names" | grep -qx "$1"; }
put_secret() { wrangler secret put "$1" -c wrangler.deploy.jsonc --env "$ENV_NAME" <"$2" >/dev/null; log "put Worker secret $1"; }
secrets_changed=0
if ! has_secret ADMIN_SECRET; then
  put_secret ADMIN_SECRET "$SECRETS/admin-secret"
  secrets_changed=1
elif [ "$had_admin_file" = 0 ]; then
  # The Worker has one this machine does not know: replace it (it guards only admin routes).
  put_secret ADMIN_SECRET "$SECRETS/admin-secret"
  secrets_changed=1
else
  log "Worker secret ADMIN_SECRET is set"
fi
if has_secret HOOK_SECRET_KEY; then
  if [ "$had_hook_file" = 0 ]; then
    log "warning: Worker secret HOOK_SECRET_KEY is set, but $SECRETS/hook-secret-key is missing. It was not replaced:"
    log "  it encrypts the AI 服务 key and hook secrets stored on the server. Copy the file from where you made it."
  else
    log "Worker secret HOOK_SECRET_KEY is set"
  fi
else
  if [ "$had_hook_file" = 0 ]; then new_secret "$SECRETS/hook-secret-key"; log "made $SECRETS/hook-secret-key"; fi
  put_secret HOOK_SECRET_KEY "$SECRETS/hook-secret-key"
  secrets_changed=1
fi

# ---------------------------------------------------------------------------------------------
step "8. first owner"
cd "$ROOT"
admin() { bun "$ROOT/server/scripts/admin.ts" "$@" --env "$ENV_NAME" --url "$BASE"; }
made_owner=0
if [ -s "$SECRETS/owner-token" ]; then
  log "owner token exists: $SECRETS/owner-token"
else
  if [ -z "$OWNER_NAME" ] && [ -t 0 ]; then
    read -r -p "Your name, as the team's first owner: " OWNER_NAME
  fi
  [ -n "$OWNER_NAME" ] || die "no owner yet: run again with --owner-name <your name>"
  [ "$secrets_changed" = 0 ] || sleep "${SECRET_SETTLE_SECONDS:-5}"
  waited=0
  out="$WORK/bootstrap.log"
  while :; do
    if admin bootstrap --name "$OWNER_NAME" >"$out" 2>&1; then
      cat "$out" >&2
      made_owner=1
      break
    fi
    if grep -q "409\|conflict" "$out"; then
      cat "$out" >&2
      die "this server has an owner already, and this computer has no $SECRETS/owner-token. Copy that file from the computer that made the owner, or sign in from the app with a sign-in code from an owner."
    fi
    if [ "$waited" -ge "$BOOTSTRAP_WAIT" ]; then
      cat "$out" >&2
      die "could not make the first owner (above); run scripts/setup-server.sh $ENV_NAME again in a minute"
    fi
    # A secret that was just put takes a few seconds to reach every location.
    sleep 5
    waited=$((waited + 5))
  done
fi
CODE_FILE=""
if [ "$made_owner" = 1 ] || [ "$SIGN_IN_CODE" = 1 ]; then
  out="$WORK/invite.log"
  admin invite --self >"$out" 2>&1 || { cat "$out" >&2; die "could not make a sign-in code (above)"; }
  cat "$out" >&2
  CODE_FILE="$(sed -n 's/^sign-in code (XXXX-XXXX, single use): //p' "$out")"
fi

# ---------------------------------------------------------------------------------------------
step "9. next step"
{
  printf 'The server of %s is running: %s\n\n' "$ENV_NAME" "$BASE"
  if [ -n "$CODE_FILE" ]; then
    printf 'Your sign-in code (single use, valid 7 days) is in %s. Show it with:\n  cat %s\n\n' "$CODE_FILE" "$CODE_FILE"
  else
    printf 'For a new sign-in code for the owner, run: scripts/setup-server.sh %s --sign-in-code\n\n' "$ENV_NAME"
  fi
  printf 'Next, on your Mac or Windows computer (docs/SELF-HOSTING.md step 7):\n'
  printf '  1. install the app from the GitHub releases page\n'
  printf '  2. at sign-in, choose 使用其他服务器… and enter %s\n' "$BASE"
  printf '  3. enter the sign-in code\n'
  printf '  4. 设置 › 机器 › 用这台电脑\n'
} >&2
