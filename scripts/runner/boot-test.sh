#!/usr/bin/env bash
# Boots the base image once (read-only root, 2 vCPU, 4 GiB cap) and times it. Run on the host
# as the runner user after host-setup.sh and build-base-image.sh.
set -euo pipefail
d=$(mktemp -d /srv/runner/runs/boottest.XXXX)
cat > $d/vm.json <<J
{ "boot-source": { "kernel_image_path": "/srv/runner/kernels/vmlinux",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off quiet ro init=/sbin/vm-init" },
  "drives": [ { "drive_id": "root", "path_on_host": "/srv/runner/images/base.ext4", "is_root_device": true, "is_read_only": true } ],
  "machine-config": { "vcpu_count": 2, "mem_size_mib": 4096 } }
J
start=$(date +%s.%N)
firecracker --no-api --config-file $d/vm.json --log-path /dev/null 2>&1 | while IFS= read -r l; do
  now=$(date +%s.%N); case "$l" in *vm-init*|*:\ *) printf "[+%.3fs] %s\n" "$(echo "$now - $start" | bc)" "$l";; esac
done
end=$(date +%s.%N); printf "firecracker exited after %.3fs\n" "$(echo "$end - $start" | bc)"
rm -rf $d
