#!/usr/bin/env bash
# Prepares a fresh Ubuntu 24.04 host to run bot runs in Firecracker microVMs.
# Run as root on the host. Safe to run again: every step checks before it changes anything.
#
#   scp scripts/runner/host-setup.sh root@<host>:/root/ && ssh root@<host> bash /root/host-setup.sh
#
# What it does: system packages (with gh: the daemon hands the VMs this host's GitHub sign-in,
# see docs/SELF-HOSTING.md), Docker (builds the base image), Firecracker and jailer from the
# official release (checked against its SHA256SUMS), the `runner` user that runs the daemon (kvm
# and docker groups, systemd linger), the machine daemon from the release for that user
# (install-daemon.sh; DAEMON_VERSION picks one, default latest), /srv/runner for images, the
# daemon's runs and the VMs, a rustup without toolchains in /usr/local/lib/runner-rustup (the
# daemon installs each repository's toolchain into that repository's cache with it), and
# the VM network: IP forwarding, the NFS server, the static firewall (runner-vm.nft, loaded at
# boot by runner-vm-net.service) and the root helper runner-vm that the daemon runs with sudo
# (the tap devices and the NFS exports).
# It does not touch SSH settings. Run it from its folder in a checkout (it installs siblings).
# After an update of runner-vm or runner-vm.nft, run it again: it reinstalls both and reloads
# the firewall.
# VM slots per daemon user are in /etc/runner-vm.users (`<user> <first slot> <count>`); the team
# user gets slots 0..63. More daemons on this host, each its own Unix user and machine token:
#
#   host-setup.sh add-user <user> <first slot> <count> <disk GB>
#
# makes the user (kvm group), a disk image of <disk GB> mounted at /srv/runner/u/<user> for its
# runs, its slots in /etc/runner-vm.users and its sudoers entry for runner-vm, then prints how
# to install its daemon. Run the whole script once before.
# The AI members' home folders of earlier versions (/srv/runner/homes, their images in
# /srv/runner/home-images) are retired: only the CLIs' session folders stay, so old topics can
# resume; sign-ins and installed tools are dropped. Let the daemon finish its runs first: a home
# still in use is left for the next time this script runs.
set -euo pipefail

FC_VERSION="${FC_VERSION:-latest}"
RUNNER_USER="${RUNNER_USER:-runner}"
RUNNER_HOME_DATA="/srv/runner"
here="$(cd "$(dirname "$0")" && pwd)"
USERS_FILE="${USERS_FILE:-/etc/runner-vm.users}"
# The team user's slots: its daemon's slot_base 0 and up to 64 parallel runs.
TEAM_SLOTS=64

# users_add <user> <first> <count>: lists the user's VM slots in $USERS_FILE. Running it again
# with the same range changes nothing; a range that overlaps another user's, or a second range
# for a listed user, is refused.
users_add() {
  local user="$1" first="$2" count="$3" u f n
  [[ "$user" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || { echo "bad user name: $user" >&2; return 1; }
  [[ "$first" =~ ^[0-9]{1,5}$ && "$count" =~ ^[0-9]{1,5}$ ]] || { echo "slots must be numbers" >&2; return 1; }
  first=$(( 10#$first )) count=$(( 10#$count ))
  (( count >= 1 && first + count <= 16384 )) || { echo "slots must be 1 or more, below 16384" >&2; return 1; }
  [[ -f "$USERS_FILE" ]] || install -m 644 /dev/null "$USERS_FILE"
  while read -r u f n _; do
    [[ -n "$u" && "$u" != \#* ]] || continue
    f=$(( 10#$f )) n=$(( 10#$n ))
    if [[ "$u" == "$user" ]]; then
      (( f == first && n == count )) && return 0
      echo "$user has slots $f..$(( f + n - 1 )) already in $USERS_FILE" >&2
      return 1
    fi
    if (( first < f + n && f < first + count )); then
      echo "slots $first..$(( first + count - 1 )) overlap $u's ($f..$(( f + n - 1 )))" >&2
      return 1
    fi
  done < "$USERS_FILE"
  echo "$user $first $count" >> "$USERS_FILE"
  chmod 644 "$USERS_FILE"
}

# add_user <user> <first slot> <count> <disk GB>: another daemon user on this host (see the
# header). Safe to run again.
add_user() {
  local user="$1" first="$2" count="$3" gb="$4"
  [[ "$gb" =~ ^[0-9]{1,5}$ ]] && (( 10#$gb >= 1 )) || { echo "disk GB must be a number" >&2; return 1; }
  [[ -x /usr/local/sbin/runner-vm && -d "$RUNNER_HOME_DATA/images" ]] ||
    { echo "run host-setup.sh without arguments first" >&2; return 1; }
  users_add "$user" "$first" "$count"
  id "$user" >/dev/null 2>&1 || useradd --create-home --shell /bin/bash "$user"
  passwd -l "$user" >/dev/null
  usermod -aG kvm "$user"
  loginctl enable-linger "$user"
  # The user's firecracker opens the shared kernel and base image (read-only); /srv/runner
  # lets it through to its own tree without listing the folder.
  chmod 711 "$RUNNER_HOME_DATA"
  chmod -R o+rX "$RUNNER_HOME_DATA/images" "$RUNNER_HOME_DATA/kernels"
  install -d -o root -g root -m 711 "$RUNNER_HOME_DATA/u"
  install -d -o root -g root -m 700 "$RUNNER_HOME_DATA/u-images"
  local img="$RUNNER_HOME_DATA/u-images/$user.ext4" dir="$RUNNER_HOME_DATA/u/$user"
  if [[ ! -f "$img" ]]; then
    truncate -s "$(( 10#$gb ))G" "$img"
    mkfs.ext4 -q -m 0 "$img"
  fi
  [[ -d "$dir" ]] || install -d -m 700 "$dir"
  grep -qF "$img $dir " /etc/fstab || echo "$img $dir ext4 loop,nosuid,nodev,nofail 0 2" >> /etc/fstab
  mountpoint -q "$dir" || mount "$dir"
  chown "$user:$user" "$dir"
  chmod 700 "$dir"
  echo "$user ALL=(root) NOPASSWD: /usr/local/sbin/runner-vm" > "/etc/sudoers.d/runner-vm-$user"
  chmod 440 "/etc/sudoers.d/runner-vm-$user"
  visudo -cq
  local slug
  slug="$(jq -r .slug "$here/../../product.json" 2>/dev/null || echo '<slug>')"
  cat <<NEXT
== $user is ready: slots $first..$(( 10#$first + 10#$count - 1 )), $gb GB at $dir
Next steps:
  1. Add a machine for $user in the app (its own machine token) and save the token, as $user,
     at ~$user/.config/$slug/machine-token with mode 600.
  2. Install the daemon for $user:  $here/install-daemon.sh --owner $user
  3. As $user (sudo -iu $user), write ~/.config/$slug/daemon.toml with:
       token_file = "~/.config/$slug/machine-token"
       work_root = "$dir/runs"
       max_parallel_runs = 1
       [vm]
       kernel = "$RUNNER_HOME_DATA/kernels/vmlinux"     # shared, read-only
       rootfs = "$RUNNER_HOME_DATA/images/base.ext4"    # shared, read-only
       dir = "$dir/vms"
       slot_base = $(( 10#$first ))
     then run: ~/.local/bin/$slug-daemon doctor && ~/.local/bin/$slug-daemon install-service
NEXT
}

# Retires the AI members' home folders of earlier versions (<homes>/<member>, each maybe a loop
# mount of <images>/<member>.ext4): keeps only the CLIs' session folders, as a plain folder of
# the runner user, so the daemon can still resume old topics, then deletes the images. Never
# moves sign-ins (auth.json, hosts.yml, .git-credentials) or installed tools. Running it again
# changes nothing.
retire_homes() {
  local homes="$1" images="$2" dir id img legacy keep left=0
  local -a kept=(.claude/projects .codex/sessions .pi/agent/sessions)
  if [[ -d "$homes" ]]; then
    # A run that stopped after deleting a home and before renaming its copy.
    for legacy in "$homes"/*.legacy; do
      if [[ -d "$legacy" && ! -e "${legacy%.legacy}" ]]; then
        mv "$legacy" "${legacy%.legacy}"
      fi
    done
    for dir in "$homes"/*; do
      id="$(basename "$dir")"
      [[ -d "$dir" && "$id" != *.legacy ]] || continue
      img="$images/$id.ext4"
      # Already retired: a plain folder that holds nothing but the session folders.
      if ! mountpoint -q "$dir" && [[ ! -f "$img" && -z "$(cd "$dir" && find . -mindepth 1 \
          \( -path ./.claude/projects -o -path ./.codex/sessions -o -path ./.pi/agent/sessions \) -prune \
          -o ! \( -path ./.claude -o -path ./.codex -o -path ./.pi -o -path ./.pi/agent \) -print -quit)" ]]; then
        continue
      fi
      if ! mountpoint -q "$dir" && [[ -f "$img" ]] && ! mount -o loop,ro,nosuid,nodev "$img" "$dir"; then
        echo "warning: $img does not mount; it is left on disk" >&2
        left=1
        continue
      fi
      legacy="$dir.legacy"
      rm -rf --one-file-system "$legacy"
      install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 755 "$legacy"
      for keep in "${kept[@]}"; do
        if [[ -d "$dir/$keep" && ! -L "$dir/$keep" ]]; then
          mkdir -p "$legacy/$(dirname "$keep")"
          cp -a "$dir/$keep" "$legacy/$keep"
        fi
      done
      chown -R "$RUNNER_USER:$RUNNER_USER" "$legacy"
      if mountpoint -q "$dir" && ! umount "$dir"; then
        echo "warning: $dir is in use; it is retired the next time this script runs" >&2
        rm -rf --one-file-system "$legacy"
        left=1
        continue
      fi
      rm -f "$img" "$images/$id.unlimited" "$images/$id.lock"
      rm -rf --one-file-system "$dir"
      mv "$legacy" "$dir"
      echo "retired the home of $id"
    done
  fi
  (( left )) || rm -rf --one-file-system "$images"
}

[[ $EUID -eq 0 ]] || { echo "run as root" >&2; exit 1; }
[[ -e /dev/kvm ]] || { echo "/dev/kvm is missing: this host cannot run Firecracker" >&2; exit 1; }

case "${1:-}" in
  "") ;;
  add-user)
    (( $# == 5 )) || { echo "usage: host-setup.sh add-user <user> <first slot> <count> <disk GB>" >&2; exit 1; }
    add_user "$2" "$3" "$4" "$5"
    exit 0 ;;
  *) echo "usage: host-setup.sh [add-user <user> <first slot> <count> <disk GB>]" >&2; exit 1 ;;
esac

echo "== packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get upgrade -yq
apt-get install -yq --no-install-recommends \
  ca-certificates curl git gh jq unzip zstd xz-utils tar \
  nftables iproute2 iptables e2fsprogs acl \
  build-essential pkg-config \
  docker.io docker-buildx nfs-kernel-server \
  unattended-upgrades htop bc
systemctl enable --now docker
# The stock nftables.service flushes the whole ruleset on start, which drops Docker's NAT rules.
# The daemon adds its own nftables table for the VMs instead.
systemctl disable --now nftables 2>/dev/null || true

echo "== firecracker"
arch="$(uname -m)"
if [[ "$FC_VERSION" == latest ]]; then
  FC_VERSION="$(curl -fsSL https://api.github.com/repos/firecracker-microvm/firecracker/releases/latest | jq -r .tag_name)"
fi
if ! firecracker --version 2>/dev/null | grep -q "${FC_VERSION#v}"; then
  tmp="$(mktemp -d)"
  base="https://github.com/firecracker-microvm/firecracker/releases/download/${FC_VERSION}"
  tgz="firecracker-${FC_VERSION}-${arch}.tgz"
  curl -fsSL -o "$tmp/$tgz" "$base/$tgz"
  curl -fsSL -o "$tmp/$tgz.sha256.txt" "$base/$tgz.sha256.txt"
  (cd "$tmp" && sha256sum -c "$tgz.sha256.txt")
  tar -xzf "$tmp/$tgz" -C "$tmp"
  dir="$tmp/release-${FC_VERSION}-${arch}"
  install -m 755 "$dir/firecracker-${FC_VERSION}-${arch}" /usr/local/bin/firecracker
  install -m 755 "$dir/jailer-${FC_VERSION}-${arch}" /usr/local/bin/jailer
  rm -rf "$tmp"
fi
firecracker --version | head -1

echo "== runner user"
id "$RUNNER_USER" >/dev/null 2>&1 || useradd --create-home --shell /bin/bash "$RUNNER_USER"
usermod -aG kvm,docker "$RUNNER_USER"
loginctl enable-linger "$RUNNER_USER"
install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 750 \
  "$RUNNER_HOME_DATA" "$RUNNER_HOME_DATA/images" "$RUNNER_HOME_DATA/kernels" \
  "$RUNNER_HOME_DATA/daemon" "$RUNNER_HOME_DATA/vms"
# Users of add-user reach their trees and the shared kernel and base image (see add_user).
if [[ -d "$RUNNER_HOME_DATA/u" ]]; then
  chmod 711 "$RUNNER_HOME_DATA"
  chmod -R o+rX "$RUNNER_HOME_DATA/images" "$RUNNER_HOME_DATA/kernels"
fi
# An unused folder of earlier versions: the repositories' caches live in the daemon's work root.
if [[ -d "$RUNNER_HOME_DATA/cache" ]]; then rmdir "$RUNNER_HOME_DATA/cache" 2>/dev/null || true; fi
retire_homes "$RUNNER_HOME_DATA/homes" "$RUNNER_HOME_DATA/home-images"
# The daemon user gets its own copy of root's keys so an operator can log in as it.
if [[ -s /root/.ssh/authorized_keys ]]; then
  install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 700 "/home/$RUNNER_USER/.ssh"
  install -o "$RUNNER_USER" -g "$RUNNER_USER" -m 600 /root/.ssh/authorized_keys "/home/$RUNNER_USER/.ssh/authorized_keys"
fi

echo "== daemon"
# The released Linux daemon for the runner user, in a folder it owns so the daemon can update
# itself; built from this checkout only when the release has none. A daemon whose service
# already starts that file restarts on it once it has no run; a service that starts another
# file (a daemon built by hand) keeps running that one, and install-daemon.sh prints the
# `install-service` command that moves it over. Registering the machine, daemon.toml and
# `install-service` stay steps of their own (docs/bots.md).
if ! "$here/install-daemon.sh" --owner "$RUNNER_USER" --version "${DAEMON_VERSION:-latest}"; then
  echo "warning: the daemon is not installed; see the message above" >&2
fi

echo "== host rustup"
# rustup without a toolchain for the daemon, which runs it as the runner user with RUSTUP_HOME
# and CARGO_HOME in a repository's cache. Pinned rustup-init, checked against the SHA-256 here.
RUSTUP_VERSION=1.29.3
RUSTUP_DIR=/usr/local/lib/runner-rustup
have="$("$RUSTUP_DIR/cargo/bin/rustup" --version 2>/dev/null || true)"
if [[ "$have" != "rustup $RUSTUP_VERSION "* ]]; then
  case "$arch" in
    x86_64) sum=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55 ;;
    aarch64) sum=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282 ;;
    *) echo "no rustup for $arch" >&2; exit 1 ;;
  esac
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/rustup-init" \
    "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${arch}-unknown-linux-gnu/rustup-init"
  echo "$sum  $tmp/rustup-init" | sha256sum -c -
  chmod 755 "$tmp/rustup-init"
  (umask 022; CARGO_HOME="$RUSTUP_DIR/cargo" RUSTUP_HOME="$RUSTUP_DIR/rustup" \
    "$tmp/rustup-init" -y --no-modify-path --default-toolchain none --profile minimal)
  rm -rf "$tmp"
fi
chmod -R a+rX "$RUSTUP_DIR"
"$RUSTUP_DIR/cargo/bin/rustup" --version 2>/dev/null

echo "== VM network"
cat > /etc/sysctl.d/90-runner.conf <<'EOF'
# The microVMs reach the internet through NAT on this host.
net.ipv4.ip_forward = 1
EOF
sysctl -q --system
install -m 755 "$here/runner-vm" /usr/local/sbin/runner-vm
install -m 644 "$here/runner-vm.nft" /etc/runner-vm.nft
users_add "$RUNNER_USER" 0 "$TEAM_SLOTS"
echo "$RUNNER_USER ALL=(root) NOPASSWD: /usr/local/sbin/runner-vm" > /etc/sudoers.d/runner-vm
chmod 440 /etc/sudoers.d/runner-vm
visudo -cq
cat > /etc/systemd/system/runner-vm-net.service <<'EOF'
[Unit]
Description=Firewall and NAT for the runner's microVMs
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/sbin/nft -f /etc/runner-vm.nft
# Docker may set the FORWARD policy to drop: let the VMs' traffic through its chain.
ExecStart=/bin/sh -c 'iptables -L DOCKER-USER >/dev/null 2>&1 || exit 0; for d in -i -o; do iptables -C DOCKER-USER $d rvm+ -j ACCEPT 2>/dev/null || iptables -I DOCKER-USER $d rvm+ -j ACCEPT; done'

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now nfs-server
systemctl enable runner-vm-net.service
systemctl restart runner-vm-net.service

echo "== done"
echo "firecracker: $(command -v firecracker)  jailer: $(command -v jailer)"
echo "runner user: $RUNNER_USER ($(id -nG "$RUNNER_USER"))"
echo "data: $RUNNER_HOME_DATA"
