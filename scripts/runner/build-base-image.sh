#!/usr/bin/env bash
# Builds the microVM base image: base-image/Dockerfile -> docker export -> ext4 file.
# Run on the runner host as root (the image keeps root-owned files and setuid bits):
#   scripts/runner/build-base-image.sh [out dir, default /srv/runner/images]
# Writes base-<date>.ext4 and points base.ext4 at it; runs that already started keep their image.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo "run as root" >&2; exit 1; }

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-/srv/runner/images}"
tag="runner-base:$(date -u +%Y%m%d-%H%M%S)"
name="base-${tag#runner-base:}.ext4"

docker build --pull -t "$tag" "$here/base-image"

work="$(mktemp -d)"
trap 'rm -rf "$work"; [[ -n "${cid:-}" ]] && docker rm -f "$cid" >/dev/null 2>&1 || true' EXIT
cid="$(docker create "$tag")"
mkdir "$work/root"
docker export "$cid" | tar -xpf - -C "$work/root" --numeric-owner

# Size: the files plus 20% and 512 MiB, so the read-only image never runs out of inodes or blocks.
used_mb="$(du -sm "$work/root" | cut -f1)"
size_mb=$(( used_mb * 12 / 10 + 512 ))
mkfs.ext4 -q -F -L runner-base -E root_owner=0:0 -d "$work/root" "$work/$name" "${size_mb}M"
install -m 644 -o runner -g runner "$work/$name" "$out/$name"
ln -sfn "$name" "$out/base.ext4"
echo "built $out/$name (${size_mb} MiB, files ${used_mb} MiB) from $tag"
