#!/usr/bin/env bash
# Renders the bundled character SVGs (apps/desktop/assets/characters/) and the roster contact
# sheet from Joye's private jojo-friends library. The library is copied into a cache folder,
# never into this repository, and nothing is written to the jojo-friends repository.
#
# Usage: scripts/characters/render.sh [path to jojo-friends]   (default: ~/Documents/code/jojo-friends)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
friends="${1:-$HOME/Documents/code/jojo-friends}"
work="$HOME/.cache/team-chat/jojo-render"
sheet_dir="$HOME/.cache/team-chat/design/characters"

mkdir -p "$work" "$sheet_dir"
rm -rf "$work/library"
cp -R "$friends/src/library" "$work/library"
cp "$here/render.tsx" "$here/contact-sheet.tsx" "$work/"
react="$(cd "$friends" && bun -e 'console.log(require("./package.json").dependencies.react.replace(/^\^/, ""))')"
cd "$work"
[ -f package.json ] || echo '{ "name": "jojo-render", "private": true, "type": "module" }' > package.json
bun add "react@$react" "react-dom@$react" @resvg/resvg-js@2.6.2 >/dev/null
bun render.tsx "$work/out"
rm -rf "$repo/apps/desktop/assets/characters"
cp -R "$work/out" "$repo/apps/desktop/assets/characters"
bun contact-sheet.tsx "$work/out" "$sheet_dir/roster-contact-sheet.png"
