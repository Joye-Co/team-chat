#!/usr/bin/env bash
# The app's update on Windows, end to end, on this machine: install version A, serve version
# B from a local server (`wrangler dev`), let the installed app update itself, and check that
# B runs. The Windows twin of apps/desktop/src/updater/e2e.sh.
#
#   1. install <a-setup.exe> silently for this user;
#   2. start the local server and publish <b-setup.exe> as the Windows file of version B
#      (PUT /v1/admin/releases/<b>/assets/windows). The server has no DMG: a Mac would be
#      offered nothing, and Windows is offered B;
#   3. run the installed app's `--selftest-update --install-dir`: it checks as version A,
#      downloads the installer, verifies SHA-256, starts the update helper and exits;
#   4. the helper replaces the install folder and starts B, which finishes the update and
#      reports its version; check the files, the registry and that no backup is left;
#   5. rollback: run the helper with an installer whose app does not finish the update;
#      the folder must hold B again, byte for byte;
#      interruption: kill the helper while the install folder is empty (the old files are
#      in the backup, the installer has not run), check that the recovery is registered
#      for the next sign-in, run it, and find B again, byte for byte;
#   6. with EXPECT_SIGNED=1 (installers signed by one certificate): the helper must refuse a
#      changed installer and a program of another publisher;
#   7. uninstall and check that nothing is left.
#
# Usage: scripts/test-windows-update.sh <a-setup.exe> <b-setup.exe> [port]   (default 8826)
# Env: EXPECT_SIGNED=1, DAEMON=0 (the installers hold no daemon).
# Secrets go to ~/.config/team-chat/local-<port>/ and are never printed. Everything else
# lives in <worktree>/.tmp/windows-update/. Installs and removes the app: for a CI runner or
# a test machine.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
# shellcheck source=scripts/windows/lib.sh
. "$(dirname "$0")/windows/lib.sh"
need jq curl bun openssl

A_SETUP="${1:?usage: test-windows-update.sh <a-setup.exe> <b-setup.exe> [port]}"
B_SETUP="${2:?usage: test-windows-update.sh <a-setup.exe> <b-setup.exe> [port]}"
PORT="${3:-8826}"
[ -f "$A_SETUP" ] && [ -f "$B_SETUP" ] || die "an installer is missing"
A_SETUP="$(cd "$(dirname "$A_SETUP")" && pwd)/$(basename "$A_SETUP")"
B_SETUP="$(cd "$(dirname "$B_SETUP")" && pwd)/$(basename "$B_SETUP")"
NAME="$(product name)"
SLUG="$(product slug)"
version_of() { basename "$1" | sed -E "s/^$NAME-(.*)-setup\.exe$/\1/"; }
A="$(version_of "$A_SETUP")"
B="$(version_of "$B_SETUP")"
valid_release_version "$A" && valid_release_version "$B" || die "cannot read the versions from the file names"
[ "$A" != "$B" ] || die "the two installers have the same version"

WORK="$ROOT/.tmp/windows-update"
SECRETS="$HOME/.config/team-chat/local-$PORT"
URL="http://127.0.0.1:$PORT"
DIR="$(default_install_dir)"
EXE="$DIR/$NAME.exe"
KEY="$(uninstall_key)"
BACKUPS="$(dirname "$DIR")/.$NAME.old-"
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
m() { cygpath -m "$1"; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

if reg_key_exists "$KEY" || [ -e "$DIR" ]; then
  die "$NAME is installed on this machine already; this test installs and removes it"
fi
mkdir -p "$ROOT/.tmp"
(cd "$ROOT/.tmp" && rm -rf windows-update && mkdir windows-update)
mkdir -p "$SECRETS"
umask 077
[ -s "$SECRETS/admin-secret" ] || openssl rand -hex 32 >"$SECRETS/admin-secret"
printf 'ADMIN_SECRET=%s\n' "$(cat "$SECRETS/admin-secret")" >"$ROOT/server/.dev.vars"

WRANGLER_PID=""
cleanup() {
  if [ -n "$WRANGLER_PID" ]; then
    # The whole tree: wrangler starts the server as a child process.
    winpid="$(cat "/proc/$WRANGLER_PID/winpid" 2>/dev/null || true)"
    if [ -n "$winpid" ]; then run_native taskkill /F /T /PID "$winpid" >/dev/null 2>&1 || true; fi
    kill "$WRANGLER_PID" 2>/dev/null || true
  fi
  silent_uninstall "$DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# 1. Version A, installed.
log "installing $A"
silent_install "$A_SETUP" || fail "install of $A"
"$EXE" --selftest-installed --report "$(m "$WORK/a.json")" || fail "the installed app does not report"
[ "$(jq -r .version "$WORK/a.json")" = "$A" ] || fail "the installed app is not $A"
A_FILES="$(cd "$DIR" && sha256sum ./*.exe | sort)"

# 2. The local server with version B for Windows.
log "starting the local server"
(
  cd "$ROOT/server"
  bunx wrangler d1 migrations apply DB --local --persist-to "$(m "$WORK/state")" >"$WORK/migrate.log" 2>&1
) || { tail -n 20 "$WORK/migrate.log" >&2; fail "migrations"; }
(cd "$ROOT/server" && exec bunx wrangler dev --port "$PORT" --inspector-port "$((PORT + 1000))" --persist-to "$(m "$WORK/state")") \
  >"$WORK/wrangler.log" 2>&1 &
WRANGLER_PID=$!
for _ in $(seq 1 240); do
  curl -sf "$URL/v1/health" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -sf "$URL/v1/health" >/dev/null || { tail -n 30 "$WORK/wrangler.log" >&2; fail "wrangler dev did not start"; }

curl -sf -X POST "$URL/v1/admin/bootstrap" \
  -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H 'Content-Type: application/json' \
  -d '{"ownerName":"Updater"}' | jq -r .token >"$SECRETS/owner-token"
[ -s "$SECRETS/owner-token" ] || fail "no owner token"
# Headers with secrets go through a file, never the command line.
HEADERS="$SECRETS/headers"
printf 'Authorization: Bearer %s\n' "$(cat "$SECRETS/owner-token")" >"$HEADERS"

STATUS="$(curl -s -o "$WORK/publish.json" -w '%{http_code}' -X PUT "$URL/v1/admin/releases/$B/assets/windows" \
  -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H "X-Sha256: $(sha "$B_SETUP")" \
  -H "X-Release-Notes: $(printf 'test build' | base64)" -H 'Content-Type: application/vnd.microsoft.portable-executable' \
  --data-binary "@$(m "$B_SETUP")")"
log "publish $B for Windows: HTTP $STATUS"
[ "${STATUS:0:1}" = 2 ] || { cat "$WORK/publish.json" >&2; fail "publish"; }
[ "$(curl -s -H "@$(m "$HEADERS")" "$URL/v1/updates/latest?platform=windows" | jq -r '.version + " " + .platform')" = "$B windows" ] ||
  fail "the server does not offer $B to Windows"
[ "$(curl -s -o /dev/null -w '%{http_code}' -H "@$(m "$HEADERS")" "$URL/v1/updates/latest")" = 404 ] ||
  fail "the server offers the Windows file as a DMG"

# 3. The installed app updates itself.
log "the installed app updates itself"
"$EXE" --selftest-update "$URL" --token-file "$(m "$SECRETS/owner-token")" --install-dir "$(m "$DIR")" \
  --workdir "$(m "$WORK/run")" --report "$(m "$WORK/update.json")" --finish-report "$(m "$WORK/finish.json")" && RC=0 || RC=$?
jq . "$WORK/update.json" >&2 || true
[ "$RC" = 0 ] || fail "the app did not start the update (exit $RC)"
[ "$(jq -r .steps.check.version "$WORK/update.json")" = "$B" ] || fail "the app was not offered $B"
[ "$(jq -r .steps.check.platform "$WORK/update.json")" = windows ] || fail "the offer is not for Windows"
[ "$(jq -r .steps.download.sha256_ok "$WORK/update.json")" = true ] || fail "the download was not verified"

# 4. The helper installs B and B says it runs.
UPDATES="$WORK/run/data/updates"
wait_result() { # <result.json>
  local _
  for _ in $(seq 1 480); do
    [ -s "$1" ] && return 0
    sleep 0.5
  done
  return 1
}
show_log() { printf 'helper log:\n' >&2; sed 's/^/      /' "$1" >&2 2>/dev/null || true; }
wait_result "$UPDATES/update-result.json" || { show_log "$UPDATES/update.log"; fail "the helper did not finish"; }
show_log "$UPDATES/update.log"
[ "$(jq -r .outcome "$UPDATES/update-result.json")" = updated ] || fail "the update ended as $(jq -c . "$UPDATES/update-result.json")"
jq . "$WORK/finish.json" >&2 || fail "version $B wrote no report"
[ "$(jq -r .version "$WORK/finish.json")" = "$B" ] || fail "the program that started is not $B"
[ "$(jq -r .ok "$WORK/finish.json")" = true ] || fail "$B did not finish the update"
[ "$(jq -r .old_removed "$WORK/finish.json")" = true ] || fail "$B did not remove the backup"
"$EXE" --selftest-installed --report "$(m "$WORK/b.json")" || fail "the updated app does not report"
[ "$(jq -r .version "$WORK/b.json")" = "$B" ] || fail "the install folder does not hold $B"
[ "$(product_version "$EXE")" = "$B" ] || fail "the program's version resource is not $B"
[ "$(reg_value "$KEY" DisplayVersion)" = "$B" ] || fail "Windows does not show $B"
[ ! -e "$UPDATES/pending.json" ] || fail "the marker is still there"
if compgen -G "$BACKUPS*" >/dev/null; then fail "a backup folder is left"; fi
for f in "$SLUG-update.exe" uninstall.exe; do [ -f "$DIR/$f" ] || fail "$f is missing after the update"; done
BEFORE="$(cd "$DIR" && sha256sum ./*.exe | sort)"
if [ "${DAEMON:-1}" = 1 ]; then
  [ -f "$DIR/$SLUG-daemon.exe" ] || fail "the daemon is missing after the update"
  # The daemon is replaced with the app: its file is the one of the B build.
  [ "$(grep "$SLUG-daemon.exe" <<<"$A_FILES")" != "$(grep "$SLUG-daemon.exe" <<<"$BEFORE")" ] || fail "the daemon was not replaced"
  # By its rename protocol; with no daemon running, the renamed old file is gone again.
  if compgen -G "$DIR/$SLUG-daemon.exe.*" >/dev/null; then ls "$DIR" >&2; fail "files of the daemon's replacement are left"; fi
fi
[ ! -e "$UPDATES/update-journal.json" ] || fail "the journal is left after the update"
RUN_ONCE='Software\Microsoft\Windows\CurrentVersion\RunOnce'
RECOVERY="$NAME.update-recovery"
[ -z "$(reg_value "$RUN_ONCE" "$RECOVERY")" ] || fail "the recovery is still registered after the update"
log "ok: $A -> $B, version $B runs"

# The helper outside the install folder, as the app copies it.
HELPER="$WORK/helper.exe"
cp "$DIR/$SLUG-update.exe" "$HELPER"
# helper <name> <installer> <launch args...>: one update of the installed app; prints the
# helper's exit code.
helper() {
  local name="$1" installer="$2" args=() a
  shift 2
  for a in "$@"; do args+=(--launch-arg "$a"); done
  if [ "${DAEMON:-1}" = 1 ]; then args+=(--keep "$SLUG-daemon.exe"); fi
  "$HELPER" --installer "$(m "$installer")" --sha256 "$(sha "$installer")" \
    --install-dir "$(cygpath -w "$DIR")" --backup-dir "$(cygpath -w "${BACKUPS}test")" \
    --app-exe "$NAME.exe" --version "$A" --marker "$(m "$WORK/$name-pending.json")" \
    --result "$(m "$WORK/$name-result.json")" --log "$(m "$WORK/$name.log")" \
    --uninstall-key "$KEY" --confirm-secs 60 "${args[@]}" && echo 0 || echo $?
}
snapshot() { (cd "$DIR" && sha256sum ./*.exe | sort); }

# 5. Rollback: the installer of A as "the update", started so that it never finishes the
#    update (it is told another marker, finds none and exits).
log "rollback"
RC="$(helper rollback "$A_SETUP" --selftest-updated "$(m "$WORK/no-such-marker.json")")"
show_log "$WORK/rollback.log"
[ "$RC" = 3 ] || fail "the helper's exit code after a failed update is $RC, not 3"
[ "$(jq -r .outcome "$WORK/rollback-result.json")" = rolled_back ] || fail "not rolled back: $(jq -c . "$WORK/rollback-result.json")"
[ "$(snapshot)" = "$BEFORE" ] || fail "the install folder is not what it was before the failed update"
[ "$(reg_value "$KEY" DisplayVersion)" = "$B" ] || fail "Windows shows $(reg_value "$KEY" DisplayVersion) after the rollback"
[ ! -e "$WORK/rollback-pending.json" ] || fail "the marker is left after the rollback"
if compgen -G "$BACKUPS*" >/dev/null; then fail "a backup folder is left after the rollback"; fi
"$EXE" --selftest-installed --report "$(m "$WORK/after-rollback.json")" || fail "the app does not start after the rollback"
[ "$(jq -r .version "$WORK/after-rollback.json")" = "$B" ] || fail "the app after the rollback is not $B"
log "ok: a version that does not start is rolled back to $B"

# Interruption: the helper is killed while the install folder holds no app (the old files
# are in the backup and the installer has not run), as a sign-out or a power cut would
# leave it. The helper is held at that step for the kill (UPDATE_HELPER_TEST_PAUSE_AT).
log "interrupted update"
mkdir -p "$WORK/kill"
JOURNAL="$WORK/kill/update-journal.json"
KEEP=()
if [ "${DAEMON:-1}" = 1 ]; then KEEP=(--keep "$SLUG-daemon.exe"); fi
UPDATE_HELPER_TEST_PAUSE_AT=moved "$HELPER" --installer "$(m "$A_SETUP")" --sha256 "$(sha "$A_SETUP")" \
  --install-dir "$(cygpath -w "$DIR")" --backup-dir "$(cygpath -w "${BACKUPS}test")" \
  --app-exe "$NAME.exe" --version "$A" --marker "$(m "$WORK/kill/pending.json")" \
  --result "$(m "$WORK/kill/result.json")" --log "$(m "$WORK/kill/update.log")" \
  --uninstall-key "$KEY" ${KEEP[@]+"${KEEP[@]}"} --launch-arg --selftest-installed &
HELPER_PID=$!
for _ in $(seq 1 240); do
  [ "$(jq -r .stage "$JOURNAL" 2>/dev/null)" = installing ] && break
  sleep 0.5
done
[ "$(jq -r .stage "$JOURNAL" 2>/dev/null)" = installing ] || { show_log "$WORK/kill/update.log"; fail "the helper did not reach the installer step"; }
run_native taskkill /F /PID "$(cat "/proc/$HELPER_PID/winpid")" >/dev/null
wait "$HELPER_PID" 2>/dev/null || true
# What the user would find: no app, the old files hidden next to the install folder.
[ ! -e "$EXE" ] || fail "the app is still in the install folder at the kill"
[ -f "${BACKUPS}test/$NAME.exe" ] || fail "the old app is not in the backup"
[ -f "$JOURNAL" ] || fail "the kill left no journal"
# Windows runs this at the next sign-in.
REGISTERED="$(reg_value "$RUN_ONCE" "$RECOVERY")"
case "$REGISTERED" in
  *"$(cygpath -w "$HELPER")"*"--recover"*"update-journal.json"*) ;;
  *) fail "the recovery is not registered for the next sign-in: '$REGISTERED'" ;;
esac
"$HELPER" --recover "$(cygpath -w "$JOURNAL")" || fail "the recovery failed"
show_log "$WORK/kill/update.log"
[ "$(snapshot)" = "$BEFORE" ] || { ls -la "$DIR" >&2; fail "the install folder is not what it was before the interrupted update"; }
[ "$(reg_value "$KEY" DisplayVersion)" = "$B" ] || fail "Windows shows $(reg_value "$KEY" DisplayVersion) after the recovery"
[ ! -e "$JOURNAL" ] || fail "the journal is left after the recovery"
[ -z "$(reg_value "$RUN_ONCE" "$RECOVERY")" ] || fail "the recovery is still registered"
if compgen -G "$BACKUPS*" >/dev/null; then fail "a backup folder is left after the recovery"; fi
"$EXE" --selftest-installed --report "$(m "$WORK/after-recovery.json")" || fail "the app does not start after the recovery"
[ "$(jq -r .version "$WORK/after-recovery.json")" = "$B" ] || fail "the app after the recovery is not $B"
# A second run finds nothing to do.
"$HELPER" --recover "$(cygpath -w "$JOURNAL")" || fail "a second recovery failed"
[ "$(snapshot)" = "$BEFORE" ] || fail "a second recovery changed the install folder"
log "ok: an update killed with an empty install folder is undone by the recovery"

# A changed installer is refused before anything moves.
RC="$("$HELPER" --installer "$(m "$A_SETUP")" --sha256 "$(printf '0%.0s' $(seq 1 64))" \
  --install-dir "$(cygpath -w "$DIR")" --backup-dir "$(cygpath -w "${BACKUPS}test")" \
  --app-exe "$NAME.exe" --version "$A" --marker "$(m "$WORK/checksum-pending.json")" \
  --result "$(m "$WORK/checksum-result.json")" --launch-arg --selftest-installed && echo 0 || echo $?)"
[ "$RC" = 2 ] || fail "a wrong checksum ended with exit code $RC, not 2"
[ "$(snapshot)" = "$BEFORE" ] || fail "a wrong checksum changed the install folder"
log "ok: an installer with another checksum is refused"

# 6. Signed builds: only the same publisher may update the app.
if [ "${EXPECT_SIGNED:-0}" = 1 ]; then
  log "signatures"
  [ "$(signature_status "$EXE")" = Valid ] || fail "the installed app is not signed"
  # Bytes changed after signing (in the middle of the file, inside what the signature
  # covers): the signature no longer matches the content.
  cp "$A_SETUP" "$WORK/tampered-setup.exe"
  printf 'TAMPERED' | dd of="$WORK/tampered-setup.exe" bs=1 seek="$(($(stat -c %s "$A_SETUP") / 2))" conv=notrunc 2>/dev/null
  TAMPERED="$(signature_status "$WORK/tampered-setup.exe")"
  [ "$TAMPERED" != Valid ] || fail "the tampered installer still verifies"
  RC="$(helper tampered "$WORK/tampered-setup.exe" --selftest-installed)"
  [ "$RC" = 2 ] || { show_log "$WORK/tampered.log"; fail "a tampered installer ended with exit code $RC, not 2"; }
  log "a tampered installer (Windows says $TAMPERED) is refused: $(jq -r .error "$WORK/tampered-result.json")"
  jq -e '.error | test("signature|signed")' "$WORK/tampered-result.json" >/dev/null || fail "refused for another reason: $(jq -c . "$WORK/tampered-result.json")"
  # A validly signed program of another publisher (Windows' own).
  cp "$(cygpath -u "$SYSTEMROOT")/System32/whoami.exe" "$WORK/other-setup.exe"
  RC="$(helper other "$WORK/other-setup.exe" --selftest-installed)"
  [ "$RC" = 2 ] || { show_log "$WORK/other.log"; fail "another publisher's program ended with exit code $RC, not 2"; }
  log "a program of another publisher is refused: $(jq -r .error "$WORK/other-result.json")"
  jq -e '.error | test("publisher|signed")' "$WORK/other-result.json" >/dev/null || fail "refused for another reason: $(jq -c . "$WORK/other-result.json")"
  [ "$(snapshot)" = "$BEFORE" ] || fail "a refused installer changed the install folder"
  log "ok: a tampered installer and another publisher's program are refused"
fi

# 7. Uninstall.
silent_uninstall "$DIR" || fail "uninstall"
assert_uninstalled "$DIR" || fail "the uninstall left something"
jq -n --arg a "$A" --arg b "$B" --arg signed "${EXPECT_SIGNED:-0}" \
  --slurpfile update "$WORK/update.json" --slurpfile finish "$WORK/finish.json" --slurpfile rollback "$WORK/rollback-result.json" \
  '{ok: true, from: $a, to: $b, signed: ($signed == "1"), update: $update[0], finish: $finish[0], rollback: $rollback[0]}' >"$WORK/report.json"
log "ok: update $A -> $B, rollback, interruption and recovery, uninstall. Report: $WORK/report.json"
