#!/usr/bin/env bash
# Vendors the Jojo & Friends avatar pack into apps/desktop/assets/characters/.
#
# The art is CC BY 4.0 (https://github.com/joyehuang/jojo-friends, LICENSE-ART); this script
# copies the licence next to the files. It checks out the given tag of jojo-friends into .tmp/,
# runs its `export:pack` (which proves that a status never changes the face) and replaces the
# vendored folder.
#
# Usage: scripts/characters/sync.sh [tag]   (default: the version in the vendored manifest.json)
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
dest="$root/apps/desktop/assets/characters"
repo="https://github.com/joyehuang/jojo-friends.git"
tag="${1:-v$(jq -r .version "$dest/manifest.json")}"
work="$root/.tmp/jojo-friends"

rm -rf "$work"
git clone --quiet --depth 1 --branch "$tag" "$repo" "$work"
pnpm="pnpm"
command -v pnpm >/dev/null || pnpm="bunx pnpm"
(cd "$work" && $pnpm install --frozen-lockfile --silent && $pnpm run --silent export:pack "$work/out")
cp "$work/LICENSE-ART" "$work/out/LICENSE"
rm -rf "$dest"
mkdir -p "$(dirname "$dest")"
cp -R "$work/out" "$dest"
echo "vendored jojo-friends $tag: $(find "$dest" -name '*.svg' | wc -l | tr -d ' ') files, $(du -sh "$dest" | cut -f1)"
