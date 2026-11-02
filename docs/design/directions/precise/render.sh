#!/bin/sh
# Renders every mockup to png/ at 2x. Usage: sh render.sh [name ...]
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
bun "$dir/build.ts" >/dev/null
names="${*:-main-light main-dark main-light-top main-dark-top settings-members login switcher worst-case icon}"
for n in $names; do
  size=1440,900
  [ "$n" = icon ] && size=1440,1000
  prof="$(mktemp -d)"
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=$size --user-data-dir="$prof" \
    --screenshot="$dir/png/$n.png" "file://$dir/$n.html" >/dev/null 2>&1
  rm -rf "$prof"
  echo "rendered png/$n.png"
done
