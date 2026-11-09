#!/usr/bin/env bash
# Prepares a fresh Ubuntu 24.04 host to run bot runs in Firecracker microVMs.
# Run as root on the host. Safe to run again: every step checks before it changes anything.
#
#   scp scripts/runner/host-setup.sh root@<host>:/root/ && ssh root@<host> bash /root/host-setup.sh
#
# What it does: system packages, Docker (builds the base image), Firecracker and jailer from the
# official release (checked against its SHA256SUMS), the `runner` user that runs the daemon (kvm
# and docker groups, systemd linger), IP forwarding for the VMs' NAT, and /srv/runner for images,
# caches and run disks. It does not touch SSH settings.
set -euo pipefail

FC_VERSION="${FC_VERSION:-latest}"
RUNNER_USER="${RUNNER_USER:-runner}"
RUNNER_HOME_DATA="/srv/runner"

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
  docker.io docker-buildx \
  unattended-upgrades htop
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
  "$RUNNER_HOME_DATA/cache" "$RUNNER_HOME_DATA/runs"
# The daemon user gets its own copy of root's keys so an operator can log in as it.
if [[ -s /root/.ssh/authorized_keys ]]; then
  install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 700 "/home/$RUNNER_USER/.ssh"
  install -o "$RUNNER_USER" -g "$RUNNER_USER" -m 600 /root/.ssh/authorized_keys "/home/$RUNNER_USER/.ssh/authorized_keys"
fi

echo "== networking"
cat > /etc/sysctl.d/90-runner.conf <<'EOF'
# The microVMs reach the internet through NAT on this host.
net.ipv4.ip_forward = 1
EOF
sysctl -q --system

echo "== done"
echo "firecracker: $(command -v firecracker)  jailer: $(command -v jailer)"
echo "runner user: $RUNNER_USER ($(id -nG "$RUNNER_USER"))"
echo "data: $RUNNER_HOME_DATA"
