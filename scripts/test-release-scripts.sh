#!/usr/bin/env bash
# Tests the release guards of scripts/deploy-server.sh and scripts/release-check.sh against a
# local stub of GET /v1/health, and the upload of scripts/release.sh (--upload-only, the DMG,
# the Windows installer and the Linux daemon) against a local stub of the release routes; the
# name scripts/build-daemon.sh gives the daemon; and scripts/runner/install-daemon.sh against a
# local stub of the GitHub API. Deploys nothing and needs no secret: the admin secret of the
# upload test is made up here.
#
# Usage: scripts/test-release-scripts.sh
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need bun curl jq

WORK="$ROOT/.tmp/test-release-scripts"
mkdir -p "$WORK"
PORT="${STUB_PORT:-18977}"
STUB_PID=""
cleanup() { if [ -n "$STUB_PID" ] && kill -0 "$STUB_PID" 2>/dev/null; then kill "$STUB_PID"; fi; }
trap cleanup EXIT

# stub <version>: serve {"ok":true,"version":<version>} on 127.0.0.1:$PORT.
stub() {
  cleanup
  bun -e "Bun.serve({ hostname: '127.0.0.1', port: $PORT, fetch: () => Response.json({ ok: true, version: process.argv[1] }) })" "$1" \
    >"$WORK/stub.log" 2>&1 &
  STUB_PID=$!
  local _
  for _ in $(seq 1 50); do
    [ "$(curl -s "http://127.0.0.1:$PORT/v1/health" | jq -r .version 2>/dev/null)" = "$1" ] && return 0
    sleep 0.1
  done
  die "stub did not start"
}

fails=0
expect() { # expect <ok|fail> <name> <command...>
  local want="$1" name="$2" got
  shift 2
  # The output of the command before this one stays readable in prev.log: a check about what
  # a command printed is itself a command.
  if [ -f "$WORK/out.log" ]; then cp "$WORK/out.log" "$WORK/prev.log"; fi
  if "$@" >"$WORK/out.log" 2>&1; then got=ok; else got=fail; fi
  if [ "$got" = "$want" ]; then
    printf 'pass  %s\n' "$name"
  else
    printf 'FAIL  %s (wanted %s, got %s)\n' "$name" "$want" "$got"
    sed 's/^/      /' "$WORK/out.log"
    fails=$((fails + 1))
  fi
}

export CHAT_API_BASE="http://127.0.0.1:$PORT" HEALTH_WAIT_SECONDS=0
D="$ROOT/scripts/deploy-server.sh"

expect fail "deploy refuses a 0.0.0 version" env DEPLOY_VERSION=0.0.0-staging "$D" staging --print-version
expect fail "deploy refuses version 0.0.0" env DEPLOY_VERSION=0.0.0 "$D" production --print-version
expect ok "deploy takes a release version" env DEPLOY_VERSION=1.1.0 "$D" staging --print-version

stub 0.0.0-staging
expect fail "health check fails on a placeholder version" "$D" staging --check-health
expect fail "release-check --env fails on a placeholder version" "$ROOT/scripts/release-check.sh" "$(workspace_version)" --env staging
stub 1.0.0
expect fail "health check fails on another version" "$D" staging --check-health --expect 1.1.0
expect ok "health check takes a real version" "$D" staging --check-health
stub "$(workspace_version)"
expect ok "health check takes the expected version" "$D" staging --check-health --expect "$(workspace_version)"
expect ok "release-check passes for the workspace version" "$ROOT/scripts/release-check.sh" "$(workspace_version)" --env staging
expect fail "release-check fails for another version" "$ROOT/scripts/release-check.sh" 9.9.9

# --- release.sh --upload-only: each platform's file goes to its own route ---------------------
# upload_stub <status>: answers every PUT with <status> when the admin secret is the test's
# (401 otherwise) and writes what it got to $WORK/upload.json. STUB_MACOS and STUB_WINDOWS give
# the DMG's route and the installer's route a status of their own; $WORK/uploads.log gets one
# line per upload, in order.
upload_stub() {
  cleanup
  rm -f "$WORK/upload.json" "$WORK/uploads.log"
  UPLOAD_OUT="$WORK/upload.json" UPLOAD_SECRET="$UPLOAD_SECRET" bun -e '
    const { createHash } = require("node:crypto");
    Bun.serve({ hostname: "127.0.0.1", port: Number(process.argv[1]), async fetch(req) {
      const url = new URL(req.url);
      if (req.method === "GET") return Response.json({ ok: true, version: "stub" });
      const body = new Uint8Array(await req.arrayBuffer());
      const ok = req.headers.get("x-admin-secret") === process.env.UPLOAD_SECRET;
      await Bun.write(process.env.UPLOAD_OUT, JSON.stringify({
        method: req.method, path: url.pathname, secretOk: ok,
        type: req.headers.get("content-type"), sha256Header: req.headers.get("x-sha256"),
        sha256Body: createHash("sha256").update(body).digest("hex"),
        notes: Buffer.from(req.headers.get("x-release-notes") ?? "", "base64").toString("utf8"),
      }));
      const windows = url.pathname.endsWith("/assets/windows");
      require("node:fs").appendFileSync(process.env.UPLOAD_OUT.replace(/upload\.json$/, "uploads.log"), (windows ? "windows" : "macos") + "\n");
      const status = (windows ? process.env.STUB_WINDOWS : process.env.STUB_MACOS) || process.argv[2];
      return new Response("{}", { status: ok ? Number(status) : 401 });
    } })' "$PORT" "$1" >"$WORK/stub.log" 2>&1 &
  STUB_PID=$!
  local _
  for _ in $(seq 1 50); do
    [ "$(curl -s "http://127.0.0.1:$PORT/v1/health" | jq -r .version 2>/dev/null)" = stub ] && return 0
    sleep 0.1
  done
  die "upload stub did not start"
}
# got <jq filter>: a field of the last upload the stub saw.
got() { jq -r "$1" "$WORK/upload.json" 2>/dev/null; }
is() { [ "$1" = "$2" ] || { printf 'got "%s", want "%s"\n' "$1" "$2"; return 1; }; }

NAME="$(product name)"
R="$ROOT/scripts/release.sh"
UPLOAD_SECRET="test-secret-$$"
export DIST="$WORK/dist" TEAM_CHAT_CONFIG_DIR="$WORK/config"
rm -rf "$DIST" "$TEAM_CHAT_CONFIG_DIR"
mkdir -p "$DIST" "$TEAM_CHAT_CONFIG_DIR/staging"
(umask 077; printf '%s\n' "$UPLOAD_SECRET" >"$TEAM_CHAT_CONFIG_DIR/staging/admin-secret")
printf 'a disk image' >"$DIST/$NAME-9.9.9.dmg"
printf 'an installer' >"$DIST/$NAME-9.9.9-setup.exe"
printf '# 9.9.9\n\n更新说明\n' >"$WORK/notes.md"
sha_of() { shasum -a 256 "$1" | awk '{ print $1 }'; }

upload_stub 200
expect ok "upload-only sends the DMG" "$R" 9.9.9 --env staging --upload-only --notes "$WORK/notes.md"
expect ok "the DMG goes to the release route" is "$(got '.method + " " + .path')" "PUT /v1/admin/releases/9.9.9"
expect ok "the DMG's type" is "$(got .type)" application/x-apple-diskimage
expect ok "the DMG's checksum is the body's" is "$(got '.sha256Header + " " + .sha256Body')" "$(sha_of "$DIST/$NAME-9.9.9.dmg") $(sha_of "$DIST/$NAME-9.9.9.dmg")"
expect ok "the notes arrive as written" is "$(got .notes)" "$(cat "$WORK/notes.md")"
expect ok "the admin secret is sent" is "$(got .secretOk)" true

expect ok "upload-only sends the Windows installer" "$R" 9.9.9 --env staging --upload-only --platform windows --notes "$WORK/notes.md"
expect ok "the installer goes to the Windows route" is "$(got '.method + " " + .path')" "PUT /v1/admin/releases/9.9.9/assets/windows"
expect ok "the installer's type" is "$(got .type)" application/vnd.microsoft.portable-executable
expect ok "the installer's checksum is the body's" is "$(got '.sha256Header + " " + .sha256Body')" "$(sha_of "$DIST/$NAME-9.9.9-setup.exe") $(sha_of "$DIST/$NAME-9.9.9-setup.exe")"
expect ok "the same notes go with the installer" is "$(got .notes)" "$(cat "$WORK/notes.md")"

LINUX_DAEMON="$DIST/$(product slug)-daemon-9.9.9-x86_64-unknown-linux-gnu"
printf '\177ELF a daemon' >"$LINUX_DAEMON"
expect ok "upload-only sends the Linux daemon" "$R" 9.9.9 --env staging --upload-only --platform linux-x86_64 --notes "$WORK/notes.md"
expect ok "the Linux daemon goes to its route" is "$(got '.method + " " + .path')" "PUT /v1/admin/releases/9.9.9/assets/linux-x86_64"
expect ok "the Linux daemon's type" is "$(got .type)" application/octet-stream
expect ok "the Linux daemon's checksum is the body's" is "$(got '.sha256Header + " " + .sha256Body')" "$(sha_of "$LINUX_DAEMON") $(sha_of "$LINUX_DAEMON")"
expect ok "the same notes go with the Linux daemon" is "$(got .notes)" "$(cat "$WORK/notes.md")"

rm -f "$WORK/upload.json"
expect fail "the Linux daemon is only uploaded by this script, never built here" "$R" 9.9.9 --env staging --platform linux-x86_64
expect fail "a missing Linux daemon stops the upload" "$R" 9.9.8 --env staging --upload-only --platform linux-x86_64
expect fail "a prerelease Linux daemon is not uploaded" "$R" 9.9.9-rc.1 --env staging --upload-only --platform linux-x86_64
expect fail "the installer is only uploaded, never built here" "$R" 9.9.9 --env staging --platform windows
expect fail "an unknown platform is refused" "$R" 9.9.9 --env staging --upload-only --platform linux
expect fail "a prerelease is not uploaded" "$R" 9.9.9-rc.1 --env staging --upload-only --platform windows
expect fail "a missing installer stops the upload" "$R" 9.9.8 --env staging --upload-only --platform windows
expect ok "none of these reached the server" test ! -e "$WORK/upload.json"
expect ok "a dry run uploads nothing" "$R" 9.9.9 --env staging --upload-only --platform windows --dry-run
expect ok "and says so" test ! -e "$WORK/upload.json"

printf 'another secret\n' >"$TEAM_CHAT_CONFIG_DIR/staging/admin-secret"
expect fail "a rejected admin secret fails the upload" "$R" 9.9.9 --env staging --upload-only --platform windows
expect ok "the secret is not in the output" bash -c "! grep -q 'another secret' '$WORK/prev.log'"
printf '%s\n' "$UPLOAD_SECRET" >"$TEAM_CHAT_CONFIG_DIR/staging/admin-secret"
upload_stub 404
expect fail "a server without the route fails the upload" "$R" 9.9.9 --env staging --upload-only --platform windows
expect ok "and says that nothing was uploaded" bash -c "grep -q 'has no route for the windows file' '$WORK/prev.log'"
expect ok "unless the operator says the server has no such route" "$R" 9.9.9 --env staging --upload-only --platform windows --allow-missing-route
upload_stub 500
expect fail "a server error fails the upload" "$R" 9.9.9 --env staging --upload-only --platform windows
# --- build-daemon.sh: the daemon alone is named by its target and has its .sha256 ---------------
# The release's Linux job builds the Linux daemon with it; release.sh --platform linux-x86_64
# uploads that name. No compile here: --skip-build takes a program already in the target folder.
if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
  FAKE_TARGET="$WORK/target"
  rm -rf "$FAKE_TARGET" "$WORK/daemon-dist" && mkdir -p "$FAKE_TARGET/release"
  # Any small program: build-daemon.sh signs it on a Mac.
  for t in /usr/bin/true /bin/true; do [ -x "$t" ] && { cp "$t" "$FAKE_TARGET/release/daemon"; break; }; done
  TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"
  built() { CARGO_TARGET_DIR="$FAKE_TARGET" DIST="$WORK/daemon-dist" "$ROOT/scripts/build-daemon.sh" --skip-build 2>/dev/null | tail -n 1; }
  DAEMON_OUT="$WORK/daemon-dist/$(product slug)-daemon-$(workspace_version)-$TRIPLE"
  expect ok "build-daemon names the daemon by version and target" is "$(built)" "$DAEMON_OUT"
  expect ok "its .sha256 checks" bash -c "cd '$WORK/daemon-dist' && shasum -a 256 -c '$(basename "$DAEMON_OUT").sha256'"
  expect ok "it is executable" test -x "$DAEMON_OUT"
  if [ "$TRIPLE" = x86_64-unknown-linux-gnu ]; then
    expect ok "the Linux daemon has the name release.sh uploads" test -f "$WORK/daemon-dist/$(product slug)-daemon-$(workspace_version)-x86_64-unknown-linux-gnu"
  fi
else
  printf 'skip  build-daemon.sh (no cargo here)\n'
fi

# --- the previous Windows installer, for the publisher comparison of a release -----------------
# A stand-in for gh: releases newest first; v9.9.9 is the release being made (the macOS job has
# created it already), v9.9.8 has no installer, v9.9.7 has one. GH_FAIL makes one call fail.
FAKE_BIN="$WORK/fake-bin"
rm -rf "$FAKE_BIN" "$WORK/previous" && mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/gh" <<'GH'
#!/usr/bin/env bash
case "$1 $2" in
  "release list")
    [ "${GH_FAIL:-}" != list ] || exit 1
    printf '%s\n' ${GH_TAGS:-v9.9.9 v9.9.8 v9.9.7 v9.9.6} ;;
  "release view")
    [ "${GH_FAIL:-}" != view ] || exit 1
    case "$3" in
      v9.9.9) printf 'X-9.9.9.dmg\nX-9.9.9-setup.exe\n' ;;
      v9.9.8) printf 'X-9.9.8.dmg\n' ;;
      v9.9.7) printf 'X-9.9.7.dmg\nX-9.9.7-setup.exe\nx-daemon-9.9.7-x86_64-pc-windows-msvc.exe\n' ;;
      v9.9.6) printf 'X-9.9.6-setup.exe\n' ;;
    esac ;;
  "release upload")
    [ "${GH_FAIL:-}" != upload ] || exit 1
    shift 2
    printf '%s\n' "$*" >>"$GH_LOG" ;;
  "release download")
    [ "${GH_FAIL:-}" != download ] || exit 1
    dir=""; name=""; tag="$3"; shift 3
    while [ $# -gt 0 ]; do case "$1" in --dir) dir="$2"; shift 2 ;; --pattern) name="$2"; shift 2 ;; *) shift ;; esac; done
    printf 'installer of %s' "$tag" >"$dir/$name" ;;
  *) exit 2 ;;
esac
GH
chmod +x "$FAKE_BIN/gh"
P="$ROOT/scripts/previous-windows-installer.sh"
prev() { PATH="$FAKE_BIN:$PATH" "$P" "$@" 2>"$WORK/prev.err"; }
expect ok "the release being made is not its own previous one" is "$(prev 9.9.9 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "a release without an installer is passed over" is "$(cat "$WORK/previous/X-9.9.7-setup.exe")" "installer of v9.9.7"
expect ok "a version given with v works" is "$(prev v9.9.9 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "the newest older release with an installer wins" is "$(prev 9.9.10 "$WORK/previous")" "$WORK/previous/X-9.9.9-setup.exe"
# An installer added to an older release is tested as an update from below it, never as a
# downgrade from a newer one. Versions compare as numbers (9.9.10 is above 9.9.9).
expect ok "an older version takes the release below it" is "$(prev 9.9.8 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "numbers, not text, order the versions" is "$(GH_TAGS="v9.9.9 v9.9.10 v9.9.6" prev 9.9.11 "$WORK/previous")" "$WORK/previous/X-9.9.9-setup.exe"
expect ok "a prerelease takes the release below its version" is "$(prev 9.9.9-rc.1 "$WORK/previous")" "$WORK/previous/X-9.9.7-setup.exe"
expect ok "no earlier installer is not an error" env PATH="$FAKE_BIN:$PATH" GH_TAGS="v9.9.9 v9.9.8" "$P" 9.9.9 "$WORK/previous"
expect ok "and names nothing" is "$(GH_TAGS="v9.9.9 v9.9.8" prev 9.9.9 "$WORK/previous")" ""
expect ok "the lowest release has nothing below it" is "$(prev 9.9.6 "$WORK/previous")" ""
for step in list view download; do
  expect fail "a failed 'gh release $step' fails the check" env PATH="$FAKE_BIN:$PATH" GH_FAIL="$step" "$P" 9.9.9 "$WORK/previous"
done

# --- scripts/runner/install-daemon.sh: a Linux host takes the released daemon -----------------
# A stand-in for the GitHub API: release v9.9.9 has the Linux daemon (with the digest GitHub
# keeps), v9.9.8 has none. GH_DIGEST overrides the announced digest.
gh_stub() {
  cleanup
  SLUG="$(product slug)" GH_DIGEST="${1:-}" bun -e '
    const { createHash } = require("node:crypto");
    const port = Number(process.argv[1]);
    const name = `${process.env.SLUG}-daemon-9.9.9-x86_64-unknown-linux-gnu`;
    const body = "\x7fELF the daemon of 9.9.9";
    const digest = process.env.GH_DIGEST || `sha256:${createHash("sha256").update(body).digest("hex")}`;
    const base = `http://127.0.0.1:${port}`;
    const v999 = { tag_name: "v9.9.9", assets: [
      { name: "X-9.9.9.dmg", browser_download_url: `${base}/dl/X-9.9.9.dmg`, digest: "sha256:" + "0".repeat(64) },
      { name, browser_download_url: `${base}/dl/${name}`, digest },
    ] };
    const v998 = { tag_name: "v9.9.8", assets: [{ name: "X-9.9.8.dmg", browser_download_url: `${base}/dl/X-9.9.8.dmg`, digest: "sha256:" + "0".repeat(64) }] };
    Bun.serve({ hostname: "127.0.0.1", port, fetch(req) {
      const p = new URL(req.url).pathname;
      if (p === "/v1/health") return Response.json({ ok: true, version: "gh" });
      if (p === "/repos/o/r/releases/latest" || p === "/repos/o/r/releases/tags/v9.9.9") return Response.json(v999);
      if (p === "/repos/o/r/releases/tags/v9.9.8") return Response.json(v998);
      if (p === `/dl/${name}`) return new Response(body);
      return new Response("not found", { status: 404 });
    } })' "$PORT" >"$WORK/stub.log" 2>&1 &
  STUB_PID=$!
  local _
  for _ in $(seq 1 50); do
    [ "$(curl -s "http://127.0.0.1:$PORT/v1/health" | jq -r .version 2>/dev/null)" = gh ] && return 0
    sleep 0.1
  done
  die "GitHub stub did not start"
}
HOST="$WORK/host"
rm -rf "$HOST" && mkdir -p "$HOST/bin"
HOST_DAEMON="$HOST/bin/$(product slug)-daemon"
install_daemon() {
  GITHUB_API="http://127.0.0.1:$PORT" DAEMON_REPO=o/r DAEMON_ARCH=x86_64 DAEMON_UNIT_DIR="$HOST/units" \
    "$ROOT/scripts/runner/install-daemon.sh" --dest "$HOST_DAEMON" "$@"
}
gh_stub
expect ok "the host installs the released Linux daemon" install_daemon
expect ok "it is the release's file" is "$(cat "$HOST_DAEMON")" "$(printf '\177ELF the daemon of 9.9.9')"
expect ok "and executable" test -x "$HOST_DAEMON"
expect ok "a version can be named" install_daemon --version 9.9.9
cp "$WORK/out.log" "$WORK/said.log"
expect ok "the same file is not installed twice" grep -q "installed already" "$WORK/said.log"
printf 'an older daemon' >"$HOST_DAEMON"
gh_stub "sha256:$(printf '0%.0s' $(seq 1 64))"
expect fail "a download that does not match the digest stops" install_daemon
expect ok "and leaves the installed daemon alone" is "$(cat "$HOST_DAEMON")" "an older daemon"
expect ok "and no half file" bash -c "! ls '$HOST/bin' | grep -qE 'new|install'"
gh_stub
expect ok "a matching download replaces the older daemon" install_daemon
expect ok "with the release's file" is "$(cat "$HOST_DAEMON")" "$(printf '\177ELF the daemon of 9.9.9')"
expect fail "a version that is not a release is refused" install_daemon --version 9.9
# The service: none yet, one that starts a hand-built daemon elsewhere, one that starts this file.
UNIT="$HOST/units/$(product slug)-daemon.service"
expect ok "without a service" install_daemon
cp "$WORK/out.log" "$WORK/said.log"
expect ok "it says how to install one" grep -q "no daemon service yet.*install-service" "$WORK/said.log"
mkdir -p "$HOST/units"
printf '[Service]\nExecStart="%s" run --config "/x/daemon.toml"\nRestart=always\n' "$HOST/elsewhere/daemon" >"$UNIT"
expect ok "with a service that starts another file" install_daemon
cp "$WORK/out.log" "$WORK/said.log"
expect ok "it says the service does not take the new file" grep -q "service starts $HOST/elsewhere/daemon, not $HOST_DAEMON.*install-service" "$WORK/said.log"
printf '[Service]\nExecStart=%s run --config /x/daemon.toml\n' "$HOST/elsewhere/daemon" >"$UNIT"
expect ok "with an unquoted ExecStart" install_daemon
cp "$WORK/out.log" "$WORK/said.log"
expect ok "it reads the file too" grep -q "service starts $HOST/elsewhere/daemon, not" "$WORK/said.log"
printf '[Service]\nExecStart="%s" run --config "/x/daemon.toml"\n' "$HOST_DAEMON" >"$UNIT"
expect ok "with a service that starts this file" install_daemon
cp "$WORK/out.log" "$WORK/said.log"
expect ok "it has nothing to say about the service" bash -c "! grep -q 'service' '$WORK/said.log'"
# The daemon's own waiting download ($dest.new) is not the script's to touch.
printf 'a download waiting for idle' >"$HOST_DAEMON.new"
expect ok "a run next to a waiting download" install_daemon
expect ok "leaves the download alone" is "$(cat "$HOST_DAEMON.new")" "a download waiting for idle"
printf 'an older daemon' >"$HOST_DAEMON"
expect ok "an install next to a waiting download" install_daemon
expect ok "leaves it alone too" is "$(cat "$HOST_DAEMON.new")" "a download waiting for idle"
expect ok "and leaves no file of its own" bash -c "! ls '$HOST/bin' | grep -q install"
rm -f "$HOST_DAEMON.new"
# Without a released Linux daemon it builds one, which needs cargo: a PATH without it fails.
NO_CARGO="$WORK/no-cargo"
rm -rf "$NO_CARGO" && mkdir -p "$NO_CARGO"
for t in bash curl jq sha256sum sed dirname id cut getent uname cat mktemp rm git; do
  if command -v "$t" >/dev/null 2>&1; then ln -sf "$(command -v "$t")" "$NO_CARGO/$t"; fi
done
expect fail "no released Linux daemon and no cargo stops" env HOME="$NO_CARGO" PATH="$NO_CARGO" GITHUB_API="http://127.0.0.1:$PORT" DAEMON_REPO=o/r DAEMON_ARCH=x86_64 \
  "$NO_CARGO/bash" "$ROOT/scripts/runner/install-daemon.sh" --dest "$HOST_DAEMON" --version 9.9.8
cp "$WORK/out.log" "$WORK/said.log"
expect ok "and says what to do" grep -q "no released Linux daemon in o/r and no cargo" "$WORK/said.log"

# --- a release is for both systems --------------------------------------------------------------
# The preflight of a release, as a dry run prints it ("would stop: ..."): never a real run here.
plan() { "$R" 9.9.9 --env staging --dry-run "$@" 2>&1; }
SETUP="$DIST/$NAME-9.9.9-setup.exe"
printf 'a daemon' >"$DIST/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe"
has() { case "$1" in *"$2"*) ;; *) printf 'want "%s" in:\n%s\n' "$2" "$1"; return 1 ;; esac; }
hasnt() { case "$1" in *"$2"*) printf 'do not want "%s" in:\n%s\n' "$2" "$1"; return 1 ;; esac; }
out="$(plan)"
expect ok "a release without the Windows installer would stop" has "$out" "would stop: a release needs the Windows installer of 9.9.9"
expect ok "and the plan says it is missing" has "$out" "missing: a real run would stop"
out="$(plan --windows-installer "$SETUP" --asset "$DIST/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe")"
expect ok "with the installer nothing about Windows stops it" hasnt "$out" "Windows installer"
expect ok "and the plan publishes it with the DMG" has "$out" "$NAME-9.9.9-setup.exe and 1 more file(s), on the GitHub Release and the server with the DMG"
out="$(plan --without-windows)"
expect ok "the Mac alone is possible only on purpose" hasnt "$out" "would stop: a release needs the Windows installer"
expect ok "and the plan says so" has "$out" "none, on purpose (--without-windows)"
expect ok "an installer of another version would stop it" has "$(plan --windows-installer "$DIST/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe")" "would stop: the Windows installer must be $NAME-9.9.9-setup.exe"
expect ok "a missing installer would stop it" has "$(plan --windows-installer "$DIST/nope-setup.exe")" "would stop: no Windows installer at"
expect fail "the installer and --without-windows contradict" "$R" 9.9.9 --env staging --dry-run --windows-installer "$SETUP" --without-windows
expect fail "a missing --asset is an error" "$R" 9.9.9 --env staging --dry-run --windows-installer "$SETUP" --asset "$DIST/nope.exe"

# --- --add-windows: a release that exists gets its installer ------------------------------------
export GH_LOG="$WORK/gh.log"
add() { PATH="$FAKE_BIN:$PATH" "$R" 9.9.9 --env staging --add-windows "$SETUP" --asset "$DIST/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe" "$@"; }
upload_stub 200
rm -f "$GH_LOG"
expect ok "add-windows uploads the installer" add
expect ok "to the Windows route of the server" is "$(got '.method + " " + .path')" "PUT /v1/admin/releases/9.9.9/assets/windows"
expect ok "and then to the GitHub Release, with the daemon" is "$(cat "$GH_LOG")" "v9.9.9 $SETUP $DIST/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe --clobber"
upload_stub 409
rm -f "$GH_LOG"
expect fail "a version that has an installer on the server is refused" add
expect ok "and says that a published file is never replaced" bash -c "grep -q 'never replaced' '$WORK/prev.log'"
expect ok "and GitHub is left alone" test ! -e "$GH_LOG"
upload_stub 200
rm -f "$WORK/upload.json"
expect fail "a release that does not exist gets nothing" env GH_FAIL=view PATH="$FAKE_BIN:$PATH" "$R" 9.9.9 --env staging --add-windows "$SETUP"
expect ok "and the server is not asked" test ! -e "$WORK/upload.json"
expect fail "an installer of another version is refused" env PATH="$FAKE_BIN:$PATH" "$R" 9.9.8 --env staging --add-windows "$SETUP"
expect ok "a dry run of add-windows changes nothing" add --dry-run
expect ok "nothing reached the server or GitHub" bash -c "[ ! -e '$WORK/upload.json' ] && [ ! -e '$GH_LOG' ]"
expect fail "add-windows does not mix with a release" env PATH="$FAKE_BIN:$PATH" "$R" 9.9.9 --env staging --add-windows "$SETUP" --without-windows
upload_stub 200
expect fail "a failed GitHub upload fails add-windows" env GH_FAIL=upload PATH="$FAKE_BIN:$PATH" "$R" 9.9.9 --env staging --add-windows "$SETUP"
expect ok "and says the installer is on the server" bash -c "grep -q 'on the server but not on the GitHub Release' '$WORK/prev.log'"

upload_stub 404
rm -f "$GH_LOG"
expect fail "add-windows fails on a server without the route" add
expect ok "and leaves GitHub alone" test ! -e "$GH_LOG"

# --- the real publish path, in a sandbox ---------------------------------------------------------
# The real scripts/release.sh in a throwaway repository with its own "origin": the builds are
# stand-ins that write a file, gh is a stand-in that keeps releases in a folder, the server is
# the stub. Nothing here can reach the real repository, GitHub or a server.
SB="$WORK/sandbox"
sandbox() {
  rm -rf "$SB" && mkdir -p "$SB/home" "$SB/gh" "$SB/bin" "$SB/win"
  git init -q --bare "$SB/origin.git"
  git init -q -b main "$SB/repo"
  mkdir -p "$SB/repo/scripts" "$SB/repo/docs/releases"
  cp "$ROOT/scripts/release.sh" "$ROOT/scripts/lib.sh" "$ROOT/scripts/check-release-digest.sh" "$SB/repo/scripts/"
  cp "$ROOT/product.json" "$SB/repo/"
  printf '[workspace.package]\nversion = "9.9.8"\n' >"$SB/repo/Cargo.toml"
  : >"$SB/repo/Cargo.lock"
  printf '# Changelog\n\n## 9.9.9 (unreleased)\n\n- a change\n' >"$SB/repo/CHANGELOG.md"
  printf '# 9.9.9\n\nnotes\n' >"$SB/repo/docs/releases/9.9.9.md"
  printf 'dist/\n.tmp/\n' >"$SB/repo/.gitignore"
  # The builds: one file each, named as the real ones name them.
  cat >"$SB/repo/scripts/build-dmg.sh" <<'B'
#!/usr/bin/env bash
. "$(dirname "$0")/lib.sh"
mkdir -p "$DIST" && printf 'dmg of %s' "$(workspace_version)" >"$DIST/$(product name)-$(workspace_version).dmg"
B
  cat >"$SB/repo/scripts/build-daemon.sh" <<'B'
#!/usr/bin/env bash
. "$(dirname "$0")/lib.sh"
mkdir -p "$DIST" && printf 'daemon' >"$DIST/x-daemon-$(workspace_version)-aarch64-apple-darwin" && printf '%s\n' "$DIST/x-daemon-$(workspace_version)-aarch64-apple-darwin"
B
  printf '#!/usr/bin/env bash\nexit 0\n' >"$SB/repo/scripts/test-dmg.sh"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$SB/repo/scripts/release-check.sh"
  chmod +x "$SB/repo/scripts/"*.sh
  git -C "$SB/repo" config user.name test
  git -C "$SB/repo" config user.email test@example.invalid
  git -C "$SB/repo" add -A
  git -C "$SB/repo" commit -q -m "feat: something to release"
  git -C "$SB/repo" remote add origin "$SB/origin.git"
  git -C "$SB/repo" push -q origin main
  git -C "$SB/repo" fetch -q origin
  printf 'installer of 9.9.9' >"$SB/win/$NAME-9.9.9-setup.exe"
  printf 'windows daemon' >"$SB/win/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$SB/bin/cargo"
  # gh: releases are folders under $SB/gh; every call is a line of $SB/gh.log.
  cat >"$SB/bin/gh" <<'GH'
#!/usr/bin/env bash
S="$GH_SANDBOX"
printf '%s\n' "$*" >>"$S.log"
case "$1 $2" in
  "auth status") exit 0 ;;
  "repo view") echo "sandbox/repo" ;;
  "release view")
    [ -d "$S/$3" ] || exit 1
    case "$*" in
      *assets*) for f in "$S/$3"/*; do printf '%s\tsha256:%s\n' "$(basename "$f")" "$(shasum -a 256 "$f" | awk '{ print $1 }')"; done ;;
      *) echo "$3" ;;
    esac ;;
  "release create")
    [ "${GH_FAIL:-}" != create ] || exit 1
    tag="$3"; shift 3
    mkdir -p "$S/$tag"
    while [ $# -gt 0 ]; do
      case "$1" in --title | --notes-file) shift 2 ;; --*) shift ;; *) cp "$1" "$S/$tag/"; shift ;; esac
    done ;;
  "release upload")
    tag="$3"; shift 3
    [ -d "$S/$tag" ] || exit 1
    for f in "$@"; do case "$f" in --*) ;; *) cp "$f" "$S/$tag/" ;; esac; done ;;
  "release download")
    tag="$3"; shift 3; dir=""; pattern=""
    while [ $# -gt 0 ]; do case "$1" in --dir) dir="$2"; shift 2 ;; --pattern) pattern="$2"; shift 2 ;; *) shift ;; esac; done
    found=0
    for f in "$S/$tag"/*; do
      # shellcheck disable=SC2254
      case "$(basename "$f")" in $pattern) mkdir -p "$dir" && cp "$f" "$dir/" && found=1 ;; esac
    done
    [ "$found" = 1 ] ;;
  *) exit 2 ;;
esac
GH
  chmod +x "$SB/bin/cargo" "$SB/bin/gh"
}
# rel <args...>: scripts/release.sh of the sandbox for 9.9.9 on staging.
rel() {
  (cd "$SB/repo" && env HOME="$SB/home" PATH="$SB/bin:$PATH" GH_SANDBOX="$SB/gh" DIST="$SB/repo/dist" \
    TC_SERVERS_JSON='{"staging":"http://127.0.0.1:1"}' scripts/release.sh 9.9.9 --env staging "$@")
}
# The same with a GitHub that cannot create the release.
rel_create_fails() { GH_FAIL=create rel "$@"; }
BOTH=(--notes docs/releases/9.9.9.md --windows-installer "$SB/win/$NAME-9.9.9-setup.exe" --asset "$SB/win/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe")
origin_log() { git -C "$SB/origin.git" log --format=%s "$@"; }
released() { ls "$SB/gh/v9.9.9" 2>/dev/null | sort | tr '\n' ' '; }
# uploads: the uploads the server saw, in order (curl's retries of a failed one count once).
uploads() { uniq "$WORK/uploads.log" 2>/dev/null | tr '\n' ' ' || true; }

# A release of both systems.
sandbox; upload_stub 200
expect ok "a release runs to the end" rel "${BOTH[@]}"
expect ok "the GitHub Release is made once, with the DMG, the installer and both daemons" is "$(released)" "$NAME-9.9.9-setup.exe $NAME-9.9.9.dmg x-daemon-9.9.9-aarch64-apple-darwin x-daemon-9.9.9-x86_64-pc-windows-msvc.exe "
expect ok "by one gh release create" is "$(grep -c '^release create v9.9.9 ' "$SB/gh.log")" 1
expect ok "the DMG goes to the server first, then the installer" is "$(uploads)" "macos windows "
expect ok "main has the release commit" is "$(origin_log -1 main)" "chore(release): v9.9.9"
expect ok "the tag is on that commit" is "$(git -C "$SB/origin.git" rev-parse 'v9.9.9^{commit}')" "$(git -C "$SB/origin.git" rev-parse main)"
expect ok "the changelog has the date" grep -qE '^## 9\.9\.9 \([0-9]{4}-[0-9]{2}-[0-9]{2}\)$' "$SB/repo/CHANGELOG.md"
expect fail "the same version cannot be released twice" rel "${BOTH[@]}"

# The installer's upload fails after the DMG's: the repair is --reupload.
sandbox; STUB_WINDOWS=500 upload_stub 200
expect fail "a release whose installer upload fails is red" rel "${BOTH[@]}"
expect ok "and names the repair" bash -c "grep -q -- '--reupload' '$WORK/prev.log'"
expect ok "the DMG arrived, the installer was tried" is "$(uploads)" "macos windows "
expect ok "the GitHub Release holds both files all the same" is "$(released)" "$NAME-9.9.9-setup.exe $NAME-9.9.9.dmg x-daemon-9.9.9-aarch64-apple-darwin x-daemon-9.9.9-x86_64-pc-windows-msvc.exe "
upload_stub 200
expect ok "reupload sends both again (the server answers 200 for the DMG it has)" rel --reupload
expect ok "both, the DMG first" is "$(uploads)" "macos windows "
# A server from before it took the same file twice answers 409 for the DMG: the installer
# must still arrive, and the run says that the DMG could not be confirmed.
STUB_MACOS=409 upload_stub 200
expect fail "a 409 for the DMG fails the reupload" rel --reupload
expect ok "and the run names what is not confirmed" bash -c "grep -q 'not on the server after --reupload: macos' '$WORK/prev.log'"
expect ok "but the installer is uploaded all the same" is "$(uploads)" "macos windows "
upload_stub 200
rm -f "$SB/gh/v9.9.9/$NAME-9.9.9.dmg"
expect fail "a file missing on GitHub fails the reupload, for the DMG" rel --reupload
expect ok "and the other file is still uploaded" is "$(uploads)" "windows "

# Without the admin secret a repair uploads nothing: it must say so and fail, not report
# that the files are on the server.
upload_stub 200
mv "$TEAM_CHAT_CONFIG_DIR/staging/admin-secret" "$WORK/admin-secret.away"
expect fail "reupload without the admin secret fails" rel --reupload
expect ok "and says why" bash -c "grep -q 'no admin secret' '$WORK/prev.log' && ! grep -q 'are on the server' '$WORK/prev.log'"
expect ok "nothing was uploaded" test ! -e "$WORK/uploads.log"
expect fail "upload-only without the admin secret fails too" "$R" 9.9.9 --env staging --upload-only --platform windows
expect ok "and uploads nothing" test ! -e "$WORK/uploads.log"
mv "$WORK/admin-secret.away" "$TEAM_CHAT_CONFIG_DIR/staging/admin-secret"

# The run stops between the tag and the GitHub Release: --resume finishes it.
sandbox; upload_stub 200
expect fail "a release that cannot create the GitHub Release is red" rel_create_fails "${BOTH[@]}"
expect ok "the tag is on main, there is no release" bash -c "git -C '$SB/origin.git' rev-parse -q --verify refs/tags/v9.9.9 >/dev/null && [ ! -d '$SB/gh/v9.9.9' ]"
expect ok "nothing was uploaded" test ! -e "$WORK/uploads.log"
expect fail "the same command again stops" rel "${BOTH[@]}"
expect ok "and says how to go on" bash -c "grep -q 'Finish it with --resume' '$WORK/prev.log'"
before="$(git -C "$SB/origin.git" rev-parse main)"
expect ok "--resume finishes the release" rel "${BOTH[@]}" --resume
expect ok "without another commit on main" is "$(git -C "$SB/origin.git" rev-parse main)" "$before"
expect ok "with the GitHub Release of both systems" is "$(released)" "$NAME-9.9.9-setup.exe $NAME-9.9.9.dmg x-daemon-9.9.9-aarch64-apple-darwin x-daemon-9.9.9-x86_64-pc-windows-msvc.exe "
expect ok "and both uploads" is "$(uploads)" "macos windows "
expect fail "--resume has nothing to finish afterwards" rel "${BOTH[@]}" --resume
sandbox
expect fail "--resume without a tag is refused" rel "${BOTH[@]}" --resume
sandbox; upload_stub 200
rel_create_fails "${BOTH[@]}" >/dev/null 2>&1 || true
echo "// later work" >>"$SB/repo/Cargo.toml"
expect fail "--resume refuses a source that is not the tag's" rel "${BOTH[@]}" --resume

# The Mac alone, on purpose, and its installer later.
sandbox; upload_stub 200
expect ok "a release --without-windows runs" rel --notes docs/releases/9.9.9.md --without-windows
expect ok "with the Mac's files only" is "$(released)" "$NAME-9.9.9.dmg x-daemon-9.9.9-aarch64-apple-darwin "
expect ok "and one upload" is "$(uploads)" "macos "
upload_stub 200
expect fail "its reupload says the installer is missing" rel --reupload
expect ok "and how to add it" bash -c "grep -q -- '--add-windows' '$WORK/prev.log'"
upload_stub 200
expect ok "add-windows completes it" rel --add-windows "$SB/win/$NAME-9.9.9-setup.exe" --asset "$SB/win/x-daemon-9.9.9-x86_64-pc-windows-msvc.exe"
expect ok "the server got the installer" is "$(uploads)" "windows "
expect ok "and the GitHub Release has both systems" is "$(released)" "$NAME-9.9.9-setup.exe $NAME-9.9.9.dmg x-daemon-9.9.9-aarch64-apple-darwin x-daemon-9.9.9-x86_64-pc-windows-msvc.exe "

# A server without the installer's route in a release: red, not a silent skip.
sandbox; STUB_WINDOWS=404 upload_stub 200
expect fail "a 404 for the installer fails the release" rel "${BOTH[@]}"
expect ok "after the DMG arrived" is "$(uploads)" "macos windows "

# --- the logs of a run must not show a server address -----------------------------------------
L="$ROOT/scripts/check-logs-for-servers.sh"
rm -rf "$WORK/logs" && mkdir -p "$WORK/logs"
printf 'build\nuploading to ***\n' >"$WORK/logs/1.log"
SERVERS='{"staging":"https://staging.chat.example.org","production":"https://chat.example.org:8443/api"}'
expect ok "masked logs pass" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
printf 'GET https://chat.example.org:8443/api/v1/health\n' >"$WORK/logs/2.log"
expect fail "a log with the production host fails" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
expect ok "the failure does not print the host" bash -c "! grep -q 'example.org' '$WORK/prev.log'"
printf 'TC_SERVERS_JSON: {"staging":"https://staging.chat.example.org"}\n' >"$WORK/logs/2.log"
expect fail "a log with the staging host fails" env TC_SERVERS_JSON="$SERVERS" LOG_DIR="$WORK/logs" "$L"
expect ok "a run without server URLs has nothing to check" env TC_SERVERS_JSON= LOG_DIR="$WORK/logs" "$L"
# A rerun of only the failed jobs: each job's newest finished attempt is read, also when the
# newest attempt holds one job.
expect ok "a rerun reads the jobs of the earlier attempts too" is \
  "$(printf '11\tBuild\n12\tLog scan\n\n21\tLog scan\n' | "$L" --newest-per-job)" "$(printf '11\tBuild\n21\tLog scan')"

jq -n --argjson fails "$fails" --arg version "$(workspace_version)" '{ version: $version, failures: $fails }' >"$WORK/report.json"
[ "$fails" = 0 ] || die "$fails release script test(s) failed"
log "release script tests passed; report $WORK/report.json"
