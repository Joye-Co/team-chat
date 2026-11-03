#!/usr/bin/env bash
# Proves the in-app update of a released app against a local `wrangler dev`, without
# /Applications and without any keychain:
#
#   1. copy <name>.app from <old.dmg> into a folder under .tmp/;
#   2. publish a test build <next> signed with the same certificate (build-dmg.sh
#      --skip-build with APP_VERSION=<next>; the binary in CARGO_TARGET_DIR/release);
#   3. run the copied app's `--selftest-update --install-dir`: it must install <next> in
#      that folder with the same designated requirement;
#   4. publish an ad-hoc signed build <next+1>, run the self-test on a fresh copy of the old
#      app: the update must be refused and the folder must keep the old version.
#
#   apps/desktop/src/updater/e2e-install-dir.sh <old.dmg> [port]     (default port 8816)
#
# Test builds go only to the local server. Secrets go to ~/.config/team-chat/local-<port>/
# (mode 600) and are never printed. Everything else lives in <worktree>/.tmp/update-install-dir/;
# the summary is report.json there.
set -euo pipefail

OLD_DMG="${1:?usage: e2e-install-dir.sh <old.dmg> [port]}"
PORT="${2:-8816}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
WORK="$ROOT/.tmp/update-install-dir"
SECRETS="$HOME/.config/team-chat/local-$PORT"
URL="http://127.0.0.1:$PORT"
NAME="$(jq -r .name "$ROOT/product.json")"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
OLD_DMG="$(cd "$(dirname "$OLD_DMG")" && pwd)/$(basename "$OLD_DMG")"

log() { printf '==> %s\n' "$*" >&2; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
dr() { codesign -d -r- "$1" 2>/dev/null | sed -n 's/^\(# \)\{0,1\}designated => //p' | head -n 1; }
ver() { plutil -extract CFBundleShortVersionString raw -o - "$1/Contents/Info.plist"; }

rm -rf "$WORK" && mkdir -p "$WORK" "$SECRETS" && chmod 700 "$SECRETS"
umask 077
[ -s "$SECRETS/admin-secret" ] || openssl rand -hex 32 >"$SECRETS/admin-secret"
chmod 600 "$SECRETS/admin-secret"
printf 'ADMIN_SECRET=%s\n' "$(cat "$SECRETS/admin-secret")" >"$ROOT/server/.dev.vars"

# The old app, copied out of its DMG.
MNT="$WORK/mnt"
mkdir -p "$MNT"
hdiutil attach -quiet -nobrowse -noautoopen -readonly -mountpoint "$MNT" "$OLD_DMG"
mkdir -p "$WORK/pristine"
ditto "$MNT/$NAME.app" "$WORK/pristine/$NAME.app"
hdiutil detach -quiet "$MNT" || hdiutil detach -quiet -force "$MNT"
OLD_VERSION="$(ver "$WORK/pristine/$NAME.app")"
OLD_DR="$(dr "$WORK/pristine/$NAME.app")"
[ -n "$OLD_DR" ] || fail "the old app has no designated requirement"
IFS=. read -r MAJ MIN PAT <<<"$OLD_VERSION"
NEXT="$MAJ.$MIN.$((PAT + 1))"
ADHOC="$MAJ.$MIN.$((PAT + 2))"
log "old app $OLD_VERSION; test builds $NEXT (certificate) and $ADHOC (ad hoc)"

# Test builds from the release binary (local only).
APP_VERSION="$NEXT" DIST="$WORK/dist-signed" "$ROOT/scripts/build-dmg.sh" --skip-build >"$WORK/build-signed.log" 2>&1 ||
  { tail -20 "$WORK/build-signed.log"; fail "build of $NEXT"; }
APP_VERSION="$NEXT" "$ROOT/scripts/test-dmg.sh" "$WORK/dist-signed/$NAME-$NEXT.dmg" --same-as "$OLD_DMG" >"$WORK/test-signed.log" 2>&1 ||
  { cat "$WORK/test-signed.log"; fail "test-dmg of $NEXT"; }
SIGNING=adhoc APP_VERSION="$ADHOC" DIST="$WORK/dist-adhoc" "$ROOT/scripts/build-dmg.sh" --skip-build >"$WORK/build-adhoc.log" 2>&1 ||
  { tail -20 "$WORK/build-adhoc.log"; fail "build of $ADHOC"; }

# Local server with a fresh state.
cd "$ROOT/server"
bunx wrangler d1 migrations apply DB --local --persist-to "$WORK/state" >"$WORK/migrate.log" 2>&1
bunx wrangler dev --port "$PORT" --inspector-port "$((PORT + 1000))" --persist-to "$WORK/state" >"$WORK/wrangler.log" 2>&1 &
WRANGLER_PID=$!
echo "$WRANGLER_PID" >"$WORK/wrangler.pid"
cleanup() {
  if ps -p "$WRANGLER_PID" -o command= 2>/dev/null | grep -q wrangler; then
    kill "$WRANGLER_PID" 2>/dev/null || true
    wait "$WRANGLER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT
for _ in $(seq 1 120); do
  curl -sf "$URL/v1/health" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -sf "$URL/v1/health" >/dev/null || fail "wrangler dev did not start"

curl -sf -X POST "$URL/v1/admin/bootstrap" \
  -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H 'Content-Type: application/json' \
  -d '{"ownerName":"Updater"}' | jq -r .token >"$SECRETS/owner-token"
chmod 600 "$SECRETS/owner-token"
[ -s "$SECRETS/owner-token" ] || fail "no owner token"

publish() {
  local version="$1" dmg="$2" sha status
  sha="$(shasum -a 256 "$dmg" | cut -d' ' -f1)"
  status="$(curl -s -o "$WORK/publish-$version.json" -w '%{http_code}' -X PUT "$URL/v1/admin/releases/$version" \
    -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H "X-Sha256: $sha" \
    -H "X-Release-Notes: $(printf '测试版本' | base64)" -H 'Content-Type: application/x-apple-diskimage' \
    --data-binary "@$dmg")"
  log "publish $version: HTTP $status"
  [ "${status:0:1}" = 2 ] || fail "publish $version"
}

run_selftest() {
  local dir="$1" report="$2"
  "$dir/$NAME.app/Contents/MacOS/$NAME" --selftest-update "$URL" --token-file "$SECRETS/owner-token" \
    --install-dir "$dir" --report "$report" >"$report.log" 2>&1
}

# 1. Same certificate: the update installs.
publish "$NEXT" "$WORK/dist-signed/$NAME-$NEXT.dmg"
SIGNED="$WORK/apps-signed"
mkdir -p "$SIGNED" && ditto "$WORK/pristine/$NAME.app" "$SIGNED/$NAME.app"
run_selftest "$SIGNED" "$WORK/update-signed.json" && RC=0 || RC=$?
jq . "$WORK/update-signed.json" >&2
[ "$RC" = 0 ] || fail "the signed update did not install (exit $RC)"
[ "$(ver "$SIGNED/$NAME.app")" = "$NEXT" ] || fail "the folder does not hold $NEXT"
[ "$(dr "$SIGNED/$NAME.app")" = "$OLD_DR" ] || fail "the designated requirement changed"
codesign --verify --deep --strict "-R=$OLD_DR" "$SIGNED/$NAME.app" || fail "the new bundle does not satisfy the old requirement"
[ -z "$(find "$SIGNED" -mindepth 1 -maxdepth 1 ! -name "$NAME.app")" ] || fail "leftovers in $SIGNED"

# 2. Ad-hoc signature: the update is refused and the old app stays.
publish "$ADHOC" "$WORK/dist-adhoc/$NAME-$ADHOC.dmg"
REFUSED="$WORK/apps-adhoc"
mkdir -p "$REFUSED" && ditto "$WORK/pristine/$NAME.app" "$REFUSED/$NAME.app"
run_selftest "$REFUSED" "$WORK/update-adhoc.json" && RC=0 || RC=$?
jq . "$WORK/update-adhoc.json" >&2
[ "$RC" != 0 ] || fail "the ad-hoc update was installed"
jq -e '.error | test("signature does not match")' "$WORK/update-adhoc.json" >/dev/null || fail "refused for another reason"
[ "$(jq -r '.steps.check.version' "$WORK/update-adhoc.json")" = "$ADHOC" ] || fail "the ad-hoc build was not offered"
[ "$(ver "$REFUSED/$NAME.app")" = "$OLD_VERSION" ] || fail "the folder lost $OLD_VERSION"
codesign --verify --deep --strict "-R=$OLD_DR" "$REFUSED/$NAME.app" || fail "the kept app no longer verifies"
[ -z "$(find "$REFUSED" -mindepth 1 -maxdepth 1 ! -name "$NAME.app")" ] || fail "leftovers in $REFUSED"

jq -n --arg old "$OLD_VERSION" --arg next "$NEXT" --arg adhoc "$ADHOC" --arg dr "$OLD_DR" \
  --slurpfile signed "$WORK/update-signed.json" --slurpfile refused "$WORK/update-adhoc.json" \
  '{ok: true, old_version: $old, requirement: $dr,
    signed_update: {offered: $next, installed: $signed[0].steps.after.version, report: $signed[0]},
    adhoc_update: {offered: $adhoc, refused: true, kept: $old, report: $refused[0]}}' >"$WORK/report.json"
log "ok: $OLD_VERSION -> $NEXT installed with the same requirement; ad-hoc $ADHOC refused"
log "report: $WORK/report.json"
