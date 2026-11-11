#!/usr/bin/env bash
# Prepares a fresh Ubuntu 24.04 host to run bot runs in Firecracker microVMs.
# Run as root on the host. Safe to run again: every step checks before it changes anything.
#
#   scp scripts/runner/host-setup.sh root@<host>:/root/ && ssh root@<host> bash /root/host-setup.sh
#
# What it does: system packages, Docker (builds the base image), Firecracker and jailer from the
# official release (checked against its SHA256SUMS), the `runner` user that runs the daemon (kvm
# and docker groups, systemd linger), the machine daemon from the release for that user
# (install-daemon.sh; DAEMON_VERSION picks one, default latest), /srv/runner for images, the
# daemon's runs and the VMs, and
# the VM network: IP forwarding, the NFS server, the static firewall (runner-vm.nft, loaded at
# boot by runner-vm-net.service) and the root helper runner-vm that the daemon runs with sudo
# (the tap devices, the NFS exports and the AI members' home images in /srv/runner/home-images).
# It does not touch SSH settings. Run it from its folder in a checkout (it installs siblings).
# After an update of runner-vm or runner-vm.nft, run it again: it reinstalls both and reloads
# the firewall; the members' homes in /srv/runner/homes are kept.
set -euo pipefail

FC_VERSION="${FC_VERSION:-latest}"
RUNNER_USER="${RUNNER_USER:-runner}"
RUNNER_HOME_DATA="/srv/runner"
here="$(cd "$(dirname "$0")" && pwd)"

[[ $EUID -eq 0 ]] || { echo "run as root" >&2; exit 1; }
[[ -e /dev/kvm ]] || { echo "/dev/kvm is missing: this host cannot run Firecracker" >&2; exit 1; }

echo "== packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get upgrade -yq
apt-get install -yq --no-install-recommends \
  ca-certificates curl git jq unzip zstd xz-utils tar \
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
  "$RUNNER_HOME_DATA/cache" "$RUNNER_HOME_DATA/daemon" "$RUNNER_HOME_DATA/vms"
# The AI members' home folders (the daemon's [vm].homes) and their fixed-size disk images, which
# only root reads (the runner's group lists the folder) (runner-vm home-up makes and mounts each; existing homes move into theirs on
# first use and are kept). Existing folders are left as they are.
[[ -d "$RUNNER_HOME_DATA/homes" ]] || install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 755 "$RUNNER_HOME_DATA/homes"
install -d -o root -g "$RUNNER_USER" -m 750 "$RUNNER_HOME_DATA/home-images"
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

echo "== VM network"
cat > /etc/sysctl.d/90-runner.conf <<'EOF'
# The microVMs reach the internet through NAT on this host.
net.ipv4.ip_forward = 1
EOF
sysctl -q --system
install -m 755 "$here/runner-vm" /usr/local/sbin/runner-vm
install -m 644 "$here/runner-vm.nft" /etc/runner-vm.nft
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
