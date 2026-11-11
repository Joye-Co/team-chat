#!/usr/bin/env bash
# Tests scripts/setup-server.sh and the first-deploy part of scripts/deploy-server.sh without a
# Cloudflare account: a fake wrangler (D1, R2, secrets and deploy kept in files) and a local stub
# of the server routes the setup uses (health, bootstrap, me, invites). Checks that a first run
# makes everything, that a second run changes nothing, that no secret reaches the output, and
# the messages for what is missing.
#
# Usage: scripts/test-setup-server.sh
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need bun curl jq openssl

WORK="$ROOT/.tmp/test-setup-server"
rm -rf "$WORK"
mkdir -p "$WORK/state/secrets" "$WORK/state/r2" "$WORK/home"
PORT="${STUB_PORT:-18978}"
STUB_PID=""
cleanup() { if [ -n "$STUB_PID" ] && kill -0 "$STUB_PID" 2>/dev/null; then kill "$STUB_PID"; fi; }
trap cleanup EXIT

# The fake wrangler: what each command would do on Cloudflare, kept under $FAKE_STATE.
cat >"$WORK/wrangler" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
S="$FAKE_STATE"
echo "$*" >>"$S/calls"
pos=()
while [ $# -gt 0 ]; do
  case "$1" in
    -c | --env | --location | --format | --var) shift 2 ;;
    --*) shift ;;
    *) pos+=("$1"); shift ;;
  esac
done
case "${pos[*]}" in
  whoami)
    [ "${FAKE_LOGGED_OUT:-0}" = 0 ] || { echo '{"loggedIn":false}'; exit 1; }
    echo '{"loggedIn":true,"accounts":[{"id":"fake-account"}]}' ;;
  "d1 list") if [ -f "$S/d1.json" ]; then cat "$S/d1.json"; else echo '[]'; fi ;;
  "d1 create "*)
    list="$(if [ -f "$S/d1.json" ]; then cat "$S/d1.json"; else echo '[]'; fi)"
    printf '%s' "$list" | jq --arg n "${pos[2]}" '. + [{name: $n, uuid: "11111111-2222-3333-4444-555555555555"}]' >"$S/d1.json"
    echo "created ${pos[2]}" ;;
  "r2 bucket info "*) [ -e "$S/r2/${pos[3]}" ] ;;
  "r2 bucket create "*) touch "$S/r2/${pos[3]}"; echo "created ${pos[3]}" ;;
  "d1 migrations apply DB") echo "migrations applied" ;;
  deploy) echo "Uploaded chat-api-prod"; echo "  ${FAKE_DEPLOY_URL:-https://chat-api-prod.fake-subdomain.workers.dev}" ;;
  "secret list") (cd "$S/secrets" && ls) | jq -R '{name: ., type: "secret_text"}' | jq -s . ;;
  "secret put "*) cat >"$S/secrets/${pos[2]}"; echo "Uploaded secret ${pos[2]}" ;;
  *) echo "fake wrangler: unexpected command: ${pos[*]}" >&2; exit 2 ;;
esac
EOF
chmod 755 "$WORK/wrangler"

# The stub server: the admin secret it accepts is the one the fake wrangler stored.
cat >"$WORK/stub.ts" <<'EOF'
import { existsSync, readFileSync, writeFileSync } from "node:fs";
const S = process.env.FAKE_STATE!;
const json = (b: unknown, status = 200) => Response.json(b, { status });
const err = (status: number, code: string) => json({ error: { code, message: code } }, status);
const read = (f: string) => (existsSync(`${S}/${f}`) ? readFileSync(`${S}/${f}`, "utf8").trim() : "");
Bun.serve({
  hostname: "127.0.0.1",
  port: Number(process.env.PORT),
  async fetch(req) {
    const path = new URL(req.url).pathname;
    const bearer = (req.headers.get("authorization") ?? "").replace(/^Bearer /, "");
    const owner = { id: "01OWNERFAKE", name: read("owner-name"), kind: "human", level: "owner" };
    if (path === "/v1/health") return json({ ok: true, version: process.env.STUB_VERSION });
    if (path === "/v1/admin/bootstrap") {
      const secret = read("secrets/ADMIN_SECRET");
      if (!secret || req.headers.get("x-admin-secret") !== secret) return err(401, "unauthorized");
      if (read("owner-token")) return err(409, "conflict");
      const token = `tc_${crypto.randomUUID().replace(/-/g, "")}`;
      writeFileSync(`${S}/owner-token`, token);
      const { ownerName } = (await req.json()) as { ownerName: string };
      writeFileSync(`${S}/owner-name`, ownerName);
      return json({ token, member: { ...owner, name: ownerName }, group: { id: "01GROUPFAKE" } });
    }
    if (!bearer || bearer !== read("owner-token")) return err(401, "unauthorized");
    if (path === "/v1/me") return json(owner);
    if (path === "/v1/invites") {
      const body = (await req.json()) as { memberId?: string };
      if (body.memberId !== owner.id) return err(400, "invalid");
      const code = `${crypto.randomUUID().slice(0, 4)}-${crypto.randomUUID().slice(0, 4)}`.toUpperCase();
      writeFileSync(`${S}/codes`, `${read("codes")}\n${code}`);
      return json({ code, expiresAt: Date.now() + 7 * 86_400_000 });
    }
    return err(404, "not_found");
  },
});
EOF

export FAKE_STATE="$WORK/state" WRANGLER="$WORK/wrangler" HOME="$WORK/home" TC_OPS_DIR="$WORK/ops"
export DEPLOY_VERSION=1.0.0 STUB_VERSION=1.0.0 HEALTH_WAIT_SECONDS=0 SECRET_SETTLE_SECONDS=0 BOOTSTRAP_WAIT_SECONDS=0
unset CHAT_API_BASE TC_SERVERS_JSON TEAM_CHAT_CONFIG_DIR CLOUDFLARE_ACCOUNT_ID
BASE="http://127.0.0.1:$PORT"
PORT="$PORT" bun "$WORK/stub.ts" >"$WORK/stub.log" 2>&1 &
STUB_PID=$!
for _ in $(seq 1 50); do
  curl -s "$BASE/v1/health" >/dev/null 2>&1 && break
  sleep 0.1
done
curl -s "$BASE/v1/health" >/dev/null || die "stub did not start"

fails=0
pass() { printf 'pass  %s\n' "$1"; }
fail() { printf 'FAIL  %s\n' "$1"; fails=$((fails + 1)); }
check() { # check <name> <command...>
  local name="$1"
  shift
  if "$@"; then pass "$name"; else fail "$name"; fi
}
run() { # run <ok|fail> <name> <args...>: setup-server.sh with <args>, output in $WORK/out.log
  local want="$1" name="$2" got
  shift 2
  if "$ROOT/scripts/setup-server.sh" "$@" >"$WORK/out.log" 2>&1 </dev/null; then got=ok; else got=fail; fi
  if [ "$got" = "$want" ]; then
    pass "$name"
  else
    fail "$name (wanted $want, got $got)"
    sed 's/^/      /' "$WORK/out.log"
  fi
}
count() { grep -c -- "$1" "$FAKE_STATE/calls" || true; }
mode() { if [ "$(uname -s)" = Darwin ]; then stat -f '%Lp' "$1"; else stat -c '%a' "$1"; fi; }
no_secret_in_output() {
  local f v
  for f in "$HOME/.config/team-chat/production/admin-secret" "$HOME/.config/team-chat/production/hook-secret-key" \
    "$HOME/.config/team-chat/production/owner-token"; do
    v="$(cat "$f")"
    if grep -qF -- "$v" "$WORK/out.log"; then return 1; fi
  done
  while IFS= read -r v; do
    if [ -n "$v" ] && grep -qF -- "$v" "$WORK/out.log"; then return 1; fi
  done <"$FAKE_STATE/codes"
}

# --- what is missing is named -------------------------------------------------------------------
FAKE_LOGGED_OUT=1 run fail "stops when wrangler is not signed in" production --owner-name Owner
check "  and says how to sign in" grep -q "wrangler login --device" "$WORK/out.log"
check "  and changes nothing" test ! -e "$TC_OPS_DIR"
run fail "refuses an unknown env" local
mkdir -p "$WORK/foreign/deploy"
printf '// my own config\n{ "name": "chat-api", "env": { "staging": {} } }\n' >"$WORK/foreign/deploy/server.wrangler.jsonc"
TC_OPS_DIR="$WORK/foreign" run fail "leaves a config it did not make alone" production --owner-name Owner
check "  and says so" grep -q "was not made by this script" "$WORK/out.log"
: >"$FAKE_STATE/calls"

# --- first run ------------------------------------------------------------------------------------
run ok "first run sets everything up" production --owner-name Owner --url "$BASE"
cp "$WORK/out.log" "$WORK/first-run.log"
S="$HOME/.config/team-chat/production"
check "  ops folder and deploy config" test -s "$TC_OPS_DIR/deploy/server.wrangler.jsonc"
check "  config has env.production with the D1 id, bucket and Worker name" bun -e '
  const c = Bun.JSONC.parse(await Bun.file(process.argv[1]).text());
  const e = c.env.production;
  process.exit(e.name === "chat-api-prod" && e.d1_databases[0].database_id === "11111111-2222-3333-4444-555555555555" &&
    e.d1_databases[0].binding === "DB" && e.r2_buckets[0].bucket_name === "chat-files-prod" &&
    e.vars.ENVIRONMENT === "production" && e.durable_objects.bindings.length === 4 &&
    e.observability.logs.invocation_logs === false && e.triggers.crons.includes("0 3 * * *") ? 0 : 1)' "$TC_OPS_DIR/deploy/server.wrangler.jsonc"
check "  servers.json has the URL" test "$(jq -r .production "$TC_OPS_DIR/deploy/servers.json")" = "$BASE"
check "  D1 made with location oc" test "$(count 'd1 create chat-prod --location oc')" = 1
check "  R2 bucket made" test -e "$FAKE_STATE/r2/chat-files-prod"
check "  secret folder is 700" test "$(mode "$S")" = 700
check "  admin-secret is 600" test "$(mode "$S/admin-secret")" = 600
check "  hook-secret-key is 600" test "$(mode "$S/hook-secret-key")" = 600
check "  Worker ADMIN_SECRET is the file" cmp -s "$S/admin-secret" "$FAKE_STATE/secrets/ADMIN_SECRET"
check "  Worker HOOK_SECRET_KEY is the file" cmp -s "$S/hook-secret-key" "$FAKE_STATE/secrets/HOOK_SECRET_KEY"
check "  owner token saved, mode 600" test "$(mode "$S/owner-token")" = 600
check "  one sign-in code made" test "$(find "$S" -name 'signin-*.txt' | wc -l | tr -d ' ')" = 1
check "  no secret in the output" no_secret_in_output
check "  ends with the next step" grep -q "使用其他服务器" "$WORK/out.log"
hook_before="$(cat "$FAKE_STATE/secrets/HOOK_SECRET_KEY")"
: >"$FAKE_STATE/calls"

# --- second run ---------------------------------------------------------------------------------
run ok "second run succeeds" production
check "  creates no D1 database" test "$(count 'd1 create')" = 0
check "  creates no bucket" test "$(count 'r2 bucket create')" = 0
check "  puts no secret" test "$(count 'secret put')" = 0
check "  makes no new owner or code" test "$(find "$S" -name 'signin-*.txt' | wc -l | tr -d ' ')" = 1
check "  deploys again" test "$(count '^deploy')" = 1
check "  no secret in the output" no_secret_in_output

run ok "--sign-in-code makes a new code" production --sign-in-code
check "  two codes now" test "$(find "$S" -name 'signin-*.txt' | wc -l | tr -d ' ')" = 2
check "  no secret in the output" no_secret_in_output

mv "$S/hook-secret-key" "$WORK/hook-secret-key.saved"
run ok "a lost hook-secret-key file" production
check "  is not replaced on the Worker" test "$(cat "$FAKE_STATE/secrets/HOOK_SECRET_KEY")" = "$hook_before"
check "  and the output says so" grep -q "HOOK_SECRET_KEY is set, but" "$WORK/out.log"
mv "$WORK/hook-secret-key.saved" "$S/hook-secret-key"

rm "$FAKE_STATE/secrets/HOOK_SECRET_KEY"
: >"$FAKE_STATE/calls"
run ok "a Worker without HOOK_SECRET_KEY gets it" production
check "  from the file" cmp -s "$S/hook-secret-key" "$FAKE_STATE/secrets/HOOK_SECRET_KEY"
check "  and only that one" test "$(count 'secret put')" = 1

# --- a first deploy learns its URL ------------------------------------------------------------
rm "$TC_OPS_DIR/deploy/servers.json"
# No network: the health check of the made-up address fails at once, after the URL is written.
if env https_proxy=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 "$ROOT/scripts/deploy-server.sh" production >"$WORK/out.log" 2>&1; then
  fail "deploy-server.sh with no URL (health of a made-up address should fail)"
else
  pass "deploy-server.sh with no URL runs the deploy"
fi
check "  writes the workers.dev URL into servers.json" \
  test "$(jq -r .production "$TC_OPS_DIR/deploy/servers.json")" = "https://chat-api-prod.fake-subdomain.workers.dev"
check "workers_dev_url finds nothing in other output" test -z "$(printf 'Uploaded\nhttps://example.com\n' | workers_dev_url)"

if [ "$fails" -gt 0 ]; then die "$fails check(s) failed"; fi
log "all setup checks passed"
