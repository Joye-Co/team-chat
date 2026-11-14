#!/usr/bin/env bash
# Tests of host-setup.sh (retire_homes, users_add, the host rustup step) and runner-vm (callers,
# slots, export rules, no home commands) in a throwaway privileged container, never on a real
# host:
#   docker run --rm --privileged -v "$PWD":/w ubuntu:24.04 bash /w/scripts/runner/host-scripts-test.sh
set -euo pipefail
[[ -f /.dockerenv ]] || { echo "run it in a container (see the header)" >&2; exit 1; }
here="$(cd "$(dirname "$0")" && pwd)"
apt-get update -qq >/dev/null
apt-get install -yqq e2fsprogs util-linux sudo curl ca-certificates >/dev/null 2>&1
id runner >/dev/null 2>&1 || useradd --create-home runner
# shellcheck disable=SC2034 # read by retire_homes
RUNNER_USER=runner
H=/srv/runner/homes I=/srv/runner/home-images
mkdir -p "$H" "$I" /srv/runner/cache

echo "== retire_homes"
fill() {
  local d="$1"
  mkdir -p "$d/.claude/projects/p1" "$d/.codex/sessions/2026" "$d/.pi/agent/sessions/s" \
    "$d/.claude/plugins" "$d/.config/gh" "$d/.cargo/bin" "$d/.pi/agent/other"
  echo chat > "$d/.claude/projects/p1/a.jsonl"
  echo rollout > "$d/.codex/sessions/2026/r.jsonl"
  echo pi > "$d/.pi/agent/sessions/s/x.jsonl"
  echo secret > "$d/.codex/auth.json"; echo secret > "$d/.config/gh/hosts.yml"
  echo secret > "$d/.git-credentials"; echo tool > "$d/.cargo/bin/tool"
  echo x > "$d/.claude/.credentials.json"; echo x > "$d/.pi/agent/auth.json"
}
# Two image homes (one mounted, one not) and one plain home.
src=$(mktemp -d); fill "$src"
for m in imgA imgB; do
  truncate -s 64M "$I/$m.ext4"
  mkfs.ext4 -q -F -m 0 -E root_owner=1000:1000 -d "$src" "$I/$m.ext4"
  mkdir -p "$H/$m"; touch "$I/$m.lock"
done
mount -o loop "$I/imgA.ext4" "$H/imgA"
fill "$H/plain"; chown -R root:root "$H/plain"
eval "$(sed -n '/^retire_homes() {/,/^}/p' "$here/host-setup.sh")"
# shellcheck disable=SC2034 # read by the line
RUNNER_HOME_DATA=/srv/runner
eval "$(grep -F 'rmdir "$RUNNER_HOME_DATA/cache"' "$here/host-setup.sh")"
retire_homes "$H" "$I"
expect=$'./.claude\n./.claude/projects\n./.claude/projects/p1\n./.claude/projects/p1/a.jsonl\n./.codex\n./.codex/sessions\n./.codex/sessions/2026\n./.codex/sessions/2026/r.jsonl\n./.pi\n./.pi/agent\n./.pi/agent/sessions\n./.pi/agent/sessions/s\n./.pi/agent/sessions/s/x.jsonl'
check() {
  [[ ! -e "$I" ]] || { echo "FAIL home-images still there"; exit 1; }
  [[ ! -e /srv/runner/cache ]] || { echo "FAIL cache still there"; exit 1; }
  for m in imgA imgB plain; do
    ! mountpoint -q "$H/$m" || { echo "FAIL $m mounted"; exit 1; }
    got="$(cd "$H/$m" && find . -mindepth 1 | sort)"
    [[ "$got" == "$expect" ]] || { echo "FAIL $m:"; echo "$got"; exit 1; }
    [[ -z "$(find "$H/$m" ! -user runner)" ]] || { echo "FAIL $m owner"; exit 1; }
  done
  [[ "$(ls "$H")" == $'imgA\nimgB\nplain' ]] || { echo "FAIL extra entries: $(ls "$H")"; exit 1; }
  if losetup -a | grep -q home-images; then echo "FAIL loop device left"; exit 1; fi
}
check; echo "PASS only the session folders remain, home-images is gone"
before="$(cd "$H" && find . -printf '%p %i %T@\n' | sort)"
out="$(retire_homes "$H" "$I" 2>&1)"
after="$(cd "$H" && find . -printf '%p %i %T@\n' | sort)"
check
[[ -z "$out" && "$before" == "$after" ]] || { echo "FAIL second run changed something: $out"; exit 1; }
echo "PASS second run is a no-op"
# A run that stopped after deleting a home and before renaming its copy.
mv "$H/plain" "$H/plain.legacy"
retire_homes "$H" "$I"; check; echo "PASS an interrupted run recovers"

echo "== users file"
eval "$(sed -n '/^users_add() {/,/^}/p' "$here/host-setup.sh")"
USERS_FILE=/etc/runner-vm.users
rm -f "$USERS_FILE"
users_add runner 0 64
users_add runner 0 64
users_add alice 64 2
users_add bob 66 1
for bad in "carol 65 1" "carol 0 1" "carol 60 10" "alice 70 1" "carol 16383 2" "Bad 100 1"; do
  # shellcheck disable=SC2086 # the words are the arguments
  if users_add $bad 2>/dev/null; then echo "FAIL users_add $bad"; exit 1; fi
done
[[ "$(cat "$USERS_FILE")" == $'runner 0 64\nalice 64 2\nbob 66 1' ]] || { echo "FAIL users file: $(cat "$USERS_FILE")"; exit 1; }
echo "PASS users_add is idempotent and refuses overlapping slots"

echo "== runner-vm"
mkdir -p /stub; printf '#!/bin/sh\necho exportfs "$@"\n' > /stub/exportfs; chmod 755 /stub/exportfs
for u in alice bob; do id "$u" >/dev/null 2>&1 || useradd --create-home "$u"; done
id carol >/dev/null 2>&1 || useradd --create-home carol
key=0123456789abcdef
mkdir -p /srv/runner/daemon/caches/$key /srv/runner/daemon/lines/k/cli
ln -sfn /srv/runner/daemon/caches /srv/runner/daemon/alias
mkdir -p /srv/runner/u/alice/runs/r1 /srv/runner/u/bob/runs/r1 /home/alice/work /home/alice/.config
rv() { SUDO_USER=runner PATH=/stub:$PATH bash "$here/runner-vm" "$@"; }
rv_as() { local u="$1"; shift; SUDO_USER="$u" PATH=/stub:$PATH bash "$here/runner-vm" "$@"; }
refused() { # refused <what the error says> <user> <args...>
  local want="$1"; shift
  if rv_as "$@" 2>/tmp/err; then echo "FAIL $* ran"; exit 1; fi
  grep -qe "$want" /tmp/err || { echo "FAIL $*: $(cat /tmp/err)"; exit 1; }
}
refused "not in /etc/runner-vm.users" carol up 70
refused "not in /etc/runner-vm.users" carol export 70 /home/carol rw
if SUDO_USER='' bash "$here/runner-vm" up 0 2>/tmp/err; then echo "FAIL no caller ran"; exit 1; fi
grep -q "cannot tell which user" /tmp/err || { echo "FAIL no caller: $(cat /tmp/err)"; exit 1; }
if RUNNER_USER=alice SUDO_USER='' bash "$here/runner-vm" up 64 2>/dev/null; then echo "FAIL RUNNER_USER trusted"; exit 1; fi
refused "--as is for root only" alice --as runner up 0
echo "PASS an unlisted user and a caller sudo does not name are refused"
refused "not one of alice's" alice up 0
refused "not one of alice's" alice down 66
refused "not one of alice's" alice export 66 /srv/runner/u/alice/runs/r1 rw
refused "not one of runner's" runner up 64
echo "PASS a slot outside the caller's range is refused"
refused "is not inside" alice export 64 /srv/runner/u/bob/runs/r1 rw
refused "belongs to another user" runner export 0 /srv/runner/u/alice/runs/r1 rw
refused "is not inside" alice export 64 /srv/runner/daemon/lines/k/cli rw
refused "is not inside" bob export 66 /home/alice/work rw
refused "private folder" alice export 64 /home/alice/.config rw
refused "is not shared" runner export 0 /srv/runner/u rw
rv_as alice export 65 /srv/runner/u/alice/runs/r1 rw | grep -q "anonuid=$(id -u alice),anongid=$(id -g alice) 10.200.1.6:/srv/runner/u/alice/runs/r1"
rv_as alice export 64 /home/alice/work rw | grep -q "10.200.1.2:/home/alice/work"
SUDO_USER='' PATH=/stub:$PATH bash "$here/runner-vm" --as alice export 64 /srv/runner/u/alice/runs/r1 rw | grep -q "10.200.1.2:"
echo "PASS exports stay in the caller's own trees"
for p in /srv/runner/daemon/caches /srv/runner/daemon/caches/ /srv/runner/daemon/alias /srv/runner/daemon/caches/$key/..; do
  if rv export 0 "$p" rw 2>/tmp/err; then echo "FAIL $p exported"; exit 1; fi
  grep -q "holds every repository's cache" /tmp/err || { echo "FAIL $p: $(cat /tmp/err)"; exit 1; }
done
rv export 0 /srv/runner/daemon/caches/$key rw | grep -q "10.200.0.2:/srv/runner/daemon/caches/$key"
rv export 0 /srv/runner/daemon/lines/k/cli rw >/dev/null
echo "PASS export refuses caches, allows caches/<key>"
for c in home-up home-clear; do
  if rv "$c" /srv/runner/homes m1 20 2>/tmp/err; then echo "FAIL $c ran"; exit 1; fi
  grep -q "bad slot" /tmp/err || { echo "FAIL $c: $(cat /tmp/err)"; exit 1; }
done
echo "PASS home-up and home-clear are gone"

echo "== host rustup"
# shellcheck disable=SC2034 # read by the step
arch="$(uname -m)"
step="$(sed -n '/^echo "== host rustup"/,/^echo "== VM network"/p' "$here/host-setup.sh" | sed '$d')"
eval "$step" >/dev/null 2>&1
t1=$(stat -c %Y /usr/local/lib/runner-rustup/cargo/bin/rustup)
sleep 1
eval "$step" >/dev/null 2>&1
[[ $(stat -c %Y /usr/local/lib/runner-rustup/cargo/bin/rustup) == "$t1" ]] || { echo "FAIL rustup reinstalled"; exit 1; }
v="$(sudo -u runner env RUSTUP_HOME=/tmp/c/rustup CARGO_HOME=/tmp/c/cargo \
  /usr/local/lib/runner-rustup/cargo/bin/rustup --version 2>/dev/null)"
[[ "$v" == "rustup "* ]] || { echo "FAIL the runner user cannot run rustup"; exit 1; }
tc=/usr/local/lib/runner-rustup/rustup/toolchains
[[ ! -d "$tc" || -z "$(ls -A "$tc")" ]] || { echo "FAIL a toolchain was installed"; exit 1; }
echo "PASS host rustup: no toolchain, runnable by runner, a second run keeps it"
