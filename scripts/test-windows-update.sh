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
# With CHAIN=1 (the default when the installers hold the daemon) it is the whole chain a
# person goes through, in one piece:
#   a. the installed app signs in to the local server with an invite code, starts a 话题,
#      sends a message, restores the session and signs out (`--selftest-live`);
#   b. this computer becomes a machine of the team as 用这台电脑 makes it one: a machine is
#      registered on the server, its config and token are written into a private folder, and
#      the installed daemon registers itself as the user's scheduled task (`install-service`)
#      and connects;
#   c. after the app's update (step 3 and 4) the running daemon finds its new program, leaves
#      when idle, the task starts the new one, and the server sees the machine connected
#      with the daemon of version B. Nothing stops the daemon during the update;
#   d. the rollback and the interruption run with the daemon alive;
#   e. the uninstall removes the task and the daemon's process.
#
# Usage: scripts/test-windows-update.sh <a-setup.exe> <b-setup.exe> [port]   (default 8826)
# Env: EXPECT_SIGNED=1, DAEMON=0 (the installers hold no daemon).
#      PAUSE_HELPER (required): an update helper built with `--features test-pause`, for the
#      interruption of step 5. The helper inside the installers must not have that switch.
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
[ -f "${PAUSE_HELPER:-}" ] || die "PAUSE_HELPER is not a file: build one with cargo build --release -p update-helper --features test-pause and copy it out of the target folder"
PAUSE_HELPER="$(cd "$(dirname "$PAUSE_HELPER")" && pwd)/$(basename "$PAUSE_HELPER")"
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
BUNDLE_ID="$(product bundleId)"
CHAIN="${CHAIN:-${DAEMON:-1}}"
[ "${DAEMON:-1}" = 1 ] || CHAIN=0
# Where 用这台电脑 keeps the machine's config and token, and the daemon its task and log.
MACHINE_CONFIG="$(cygpath -u "$USERPROFILE")/.config/$SLUG"
MACHINE_STATE="$(cygpath -u "$LOCALAPPDATA")/$SLUG-daemon"
TASK="$BUNDLE_ID.daemon"
task_exists() { run_native "$(cygpath -u "$SYSTEMROOT")/System32/schtasks.exe" /Query /TN "$TASK" >/dev/null 2>&1; }
# daemon_id: "<pid> <start time>" of the task's daemon, as it writes them next to its log.
daemon_id() { tr -d '\r\n' <"$MACHINE_STATE/daemon.pid" 2>/dev/null || true; }
# pid_alive <pid>
pid_alive() { run_native "$(cygpath -u "$SYSTEMROOT")/System32/tasklist.exe" /FI "PID eq $1" /FO CSV /NH 2>/dev/null | grep -q "\"$1\""; }
# self_restarts: how often the daemon left by itself for a new program (its own log line).
self_restarts() { grep -c "a new version of the daemon is installed; restarting" "$MACHINE_STATE/daemon.log" 2>/dev/null || true; }
daemon_runs() { run_native "$(cygpath -u "$SYSTEMROOT")/System32/tasklist.exe" /FI "IMAGENAME eq $SLUG-daemon.exe" /FO CSV /NH 2>/dev/null | grep -qi "$SLUG-daemon.exe"; }
if [ "$CHAIN" = 1 ]; then
  if [ -e "$MACHINE_CONFIG" ] || [ -e "$MACHINE_STATE" ] || task_exists; then
    die "this computer is a machine already ($MACHINE_CONFIG, $MACHINE_STATE or the task $TASK): not touching it"
  fi
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
  if [ "$CHAIN" = 1 ]; then
    # Whatever a failed run left of the machine: the task, the process, its folders (this
    # test made them: it refuses to start when they exist).
    if task_exists; then
      run_native "$(cygpath -u "$SYSTEMROOT")/System32/schtasks.exe" /End /TN "$TASK" >/dev/null 2>&1 || true
      run_native "$(cygpath -u "$SYSTEMROOT")/System32/schtasks.exe" /Delete /TN "$TASK" /F >/dev/null 2>&1 || true
    fi
    # Only this test's programs, by their path: the daemon of the install folder (also under
    # the name it was renamed to) and the task's repair copy. Never by image name alone.
    ARG1="$(cygpath -w "$DIR")\\$SLUG-daemon.exe" ARG2="$(cygpath -w "$MACHINE_STATE")\\repair.exe" ps_out \
      'Get-Process | Where-Object { $_.Path -and ($_.Path.StartsWith($env:ARG1, "OrdinalIgnoreCase") -or $_.Path -ieq $env:ARG2) } | Stop-Process -Force' >/dev/null 2>&1 || true
    mkdir -p "$WORK/daemon" && cp "$MACHINE_STATE"/daemon.log* "$WORK/daemon/" 2>/dev/null || true
    rm -rf "$MACHINE_CONFIG" "$MACHINE_STATE"
  fi
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

# machine_field <jq filter>: a field of this test's machine as the server sees it now.
machine_field() {
  curl -s -H "@$(m "$HEADERS")" "$URL/v1/machines" | jq -r --arg id "$MACHINE_ID" ".[] | select(.id == \$id) | $1"
}
# wait_daemon <version> <seconds>: until the server sees the machine connected with the
# daemon of that version.
wait_daemon() {
  local _
  for _ in $(seq 1 "$2"); do
    [ "$(machine_field '"\(.connected) \(.daemonVersion)"')" = "true $1" ] && return 0
    sleep 1
  done
  printf 'the machine as the server sees it: %s\n' "$(machine_field '{connected, daemonVersion, healthy}' | tr -d '\n')" >&2
  printf 'daemon log:\n' >&2; tail -n 40 "$MACHINE_STATE/daemon.log" 2>/dev/null | sed 's/^/      /' >&2 || true
  return 1
}

if [ "$CHAIN" = 1 ]; then
  # a. A new member signs in with an invite code in the installed app.
  log "the installed app signs in with an invite code"
  curl -sf -X POST "$URL/v1/invites" -H "@$(m "$HEADERS")" -H 'Content-Type: application/json' \
    -d '{"name":"Windows","kind":"human","level":"member"}' | jq -r .code >"$SECRETS/invite-code"
  [ -s "$SECRETS/invite-code" ] || fail "no invite code"
  "$EXE" --selftest-live "$URL" --token-file "$(m "$SECRETS/invite-code")" --report "$(m "$WORK/live.json")" && RC=0 || RC=$?
  jq 'del(.. | .token?, .code?)' "$WORK/live.json" >&2 || true
  [ "$RC" = 0 ] || fail "the installed app did not sign in and send a message (exit $RC)"
  [ "$(jq -r .ok "$WORK/live.json")" = true ] || fail "the sign-in report is not ok"

  # b. This computer as a machine of the team, the way 用这台电脑 sets it up
  #    (apps/desktop/src/provider/this_mac.rs): register the machine, write its config and
  #    token into a folder only this user, SYSTEM and Administrators can open, then let the
  #    installed daemon register its task.
  log "this computer becomes a machine (the daemon's scheduled task)"
  curl -sf -X POST "$URL/v1/machines" -H "@$(m "$HEADERS")" -H 'Content-Type: application/json' \
    -d '{"name":"ci-runner","labels":["windows"]}' >"$SECRETS/machine.json" || fail "cannot register a machine"
  MACHINE_ID="$(jq -r .machine.id "$SECRETS/machine.json")"
  [ -n "$MACHINE_ID" ] && [ "$MACHINE_ID" != null ] || fail "no machine id"
  SYS32="$(cygpath -u "$SYSTEMROOT")/System32"
  SID="$(run_native "$SYS32/whoami.exe" /user /fo csv /nh | tr -d '\r"' | awk -F, '{ print $NF }')"
  case "$SID" in S-1-*) ;; *) fail "cannot read this user's SID" ;; esac
  mkdir -p "$MACHINE_CONFIG"
  WIN_CONFIG="$(cygpath -w "$MACHINE_CONFIG")"
  run_native "$SYS32/icacls.exe" "$WIN_CONFIG" /inheritance:r >/dev/null || fail "icacls /inheritance:r"
  run_native "$SYS32/icacls.exe" "$WIN_CONFIG" /grant:r "*$SID:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" >/dev/null || fail "icacls /grant:r"
  jq -j .token "$SECRETS/machine.json" >"$MACHINE_CONFIG/machine-token"
  rm -f "$SECRETS/machine.json"
  # TOML literal strings: a Windows path needs no escaping in them.
  # The runner has no Claude Code or Codex, and a daemon without a runtime is refused; a
  # program that exists stands in for one (no run is started here), as in
  # scripts/test-install-daemon.ps1.
  printf "server_url = '%s'\ntoken_file = '%s'\nmachine_id = '%s'\nname = 'ci-runner'\nlabels = ['windows']\n\n[runtimes.ci]\nkind = \"claude\"\ncommand = '%s'\n" \
    "$URL" "$WIN_CONFIG\\machine-token" "$MACHINE_ID" "$(cygpath -w "$SYS32/whoami.exe")" >"$MACHINE_CONFIG/daemon.toml"
  "$DIR/$SLUG-daemon.exe" install-service --config "$WIN_CONFIG\\daemon.toml" >"$WORK/install-service.log" 2>&1 ||
    { sed 's/^/      /' "$WORK/install-service.log" >&2; fail "install-service"; }
  task_exists || fail "the daemon's task is not registered"
  # The task definition the daemon keeps (UTF-16) names the daemon of the install folder.
  iconv -f UTF-16 -t UTF-8 "$MACHINE_STATE/task.xml" | grep -qF "$(cygpath -w "$DIR/$SLUG-daemon.exe")" ||
    fail "the task does not run the daemon of the install folder"
  wait_daemon "$A" 120 || fail "the daemon of $A did not connect to the server"
  DAEMON_BEFORE="$(daemon_id)"
  RESTARTS_BEFORE="$(self_restarts)"
  case "$DAEMON_BEFORE" in [0-9]*" "[0-9]*) ;; *) fail "the daemon wrote no pid and start time ('$DAEMON_BEFORE')" ;; esac
  pid_alive "${DAEMON_BEFORE%% *}" || fail "the daemon's pid file names no running process"
  log "ok: the machine is connected, daemon $A, started by the task"
fi

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
  if [ "$CHAIN" = 1 ]; then
    # c. The daemon was running a machine through the update: it was not stopped (the old
    #    process lives on from its renamed file), finds the new program within a minute,
    #    leaves when idle, and the task starts version B, which removes the renamed file.
    # Not stopped means the very process from before the update (same pid, same start time)
    # is alive, or it has left by itself for the new program and says so in its log. A
    # daemon that was killed and started again by its task has a new pid and no such line.
    if [ "$(daemon_id)" = "$DAEMON_BEFORE" ]; then
      pid_alive "${DAEMON_BEFORE%% *}" || fail "the daemon was stopped by the update (its process is gone)"
      log "the daemon of before the update is still the same process (${DAEMON_BEFORE%% *})"
    elif [ "$(self_restarts)" -gt "$RESTARTS_BEFORE" ]; then
      log "the daemon has already left by itself for the new program"
    else
      tail -n 20 "$MACHINE_STATE/daemon.log" | sed 's/^/      /' >&2 || true
      fail "the daemon was stopped by the update: another process ($(daemon_id), before $DAEMON_BEFORE) and it did not leave by itself"
    fi
    task_exists || fail "the daemon's task is gone after the update"
    log "waiting for the running daemon to move to $B"
    wait_daemon "$B" 420 || fail "the daemon did not restart on $B after the app's update"
    [ "$(self_restarts)" -gt "$RESTARTS_BEFORE" ] || fail "the daemon is on $B but its log does not say it left by itself"
    [ "$(daemon_id)" != "$DAEMON_BEFORE" ] || fail "the daemon reports $B from the process of before the update"
    for _ in $(seq 1 60); do
      compgen -G "$DIR/$SLUG-daemon.exe.*" >/dev/null || break
      sleep 1
    done
    log "ok: the machine is connected again, daemon $B, without anyone stopping it"
  fi
  # By its rename protocol; with no daemon running from it, the renamed old file is gone again.
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
# leave it. The helper is held at that step for the kill (UPDATE_HELPER_TEST_PAUSE_AT), which
# only a helper built with the test-pause feature obeys: the installed one has no such switch.
log "interrupted update"
if grep -qa UPDATE_HELPER_TEST_PAUSE_AT "$HELPER"; then fail "the installed update helper holds the test's pause switch"; fi
PAUSE="$WORK/helper-pause.exe"
cp "$PAUSE_HELPER" "$PAUSE"
mkdir -p "$WORK/kill"
JOURNAL="$WORK/kill/update-journal.json"
KEEP=()
if [ "${DAEMON:-1}" = 1 ]; then KEEP=(--keep "$SLUG-daemon.exe"); fi
UPDATE_HELPER_TEST_PAUSE_AT=moved "$PAUSE" --installer "$(m "$A_SETUP")" --sha256 "$(sha "$A_SETUP")" \
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
  *"$(cygpath -w "$PAUSE")"*"--recover"*"update-journal.json"*) ;;
  *) fail "the recovery is not registered for the next sign-in: '$REGISTERED'" ;;
esac
"$PAUSE" --recover "$(cygpath -w "$JOURNAL")" || fail "the recovery failed"
show_log "$WORK/kill/update.log"
[ "$(snapshot)" = "$BEFORE" ] || { ls -la "$DIR" >&2; fail "the install folder is not what it was before the interrupted update"; }
[ "$(reg_value "$KEY" DisplayVersion)" = "$B" ] || fail "Windows shows $(reg_value "$KEY" DisplayVersion) after the recovery"
[ ! -e "$JOURNAL" ] || fail "the journal is left after the recovery"
[ -z "$(reg_value "$RUN_ONCE" "$RECOVERY")" ] || fail "the recovery is still registered"
if compgen -G "$BACKUPS*" >/dev/null; then fail "a backup folder is left after the recovery"; fi
"$EXE" --selftest-installed --report "$(m "$WORK/after-recovery.json")" || fail "the app does not start after the recovery"
[ "$(jq -r .version "$WORK/after-recovery.json")" = "$B" ] || fail "the app after the recovery is not $B"
# A second run finds nothing to do.
"$PAUSE" --recover "$(cygpath -w "$JOURNAL")" || fail "a second recovery failed"
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
if [ "$CHAIN" = 1 ]; then
  # d. The rollback and the interruption above ran with the daemon alive; it is still this
  #    computer's machine, on version B.
  wait_daemon "$B" 420 || fail "the daemon is not connected on $B after the rollback and the recovery"
fi
silent_uninstall "$DIR" || fail "uninstall"
assert_uninstalled "$DIR" || fail "the uninstall left something"
if [ "$CHAIN" = 1 ]; then
  # e. The uninstall took the task and the process; the machine's identity and log stay.
  if task_exists; then fail "the uninstall left the daemon's task"; fi
  for _ in $(seq 1 30); do daemon_runs || break; sleep 1; done
  if daemon_runs; then fail "the uninstall left the daemon running"; fi
  [ -f "$MACHINE_CONFIG/daemon.toml" ] || fail "the uninstall removed the machine's config"
  for _ in $(seq 1 60); do
    [ "$(machine_field .connected)" = false ] && break
    sleep 1
  done
  [ "$(machine_field .connected)" = false ] || fail "the server still sees the machine connected after the uninstall"
  log "ok: the uninstall removed the task and the daemon; the machine's config stays"
fi
jq -n --arg a "$A" --arg b "$B" --arg signed "${EXPECT_SIGNED:-0}" \
  --slurpfile update "$WORK/update.json" --slurpfile finish "$WORK/finish.json" --slurpfile rollback "$WORK/rollback-result.json" \
  '{ok: true, from: $a, to: $b, signed: ($signed == "1"), update: $update[0], finish: $finish[0], rollback: $rollback[0]}' >"$WORK/report.json"
log "ok: update $A -> $B, rollback, interruption and recovery, uninstall. Report: $WORK/report.json"
