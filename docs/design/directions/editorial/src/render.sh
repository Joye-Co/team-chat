#!/bin/sh
# Renders every mockup to PNG with headless Chrome (fresh profile each time).
# Usage: sh src/render.sh [name ...]   (names without .html; default: all)
set -e
cd "$(dirname "$0")/.."
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
[ $# -eq 0 ] && set -- main-light main-dark settings-members settings-members-dark login login-dark switcher switcher-dark worst-case worst-case-dark icon icon-1024
for n in "$@"; do
  size=1440,900; scale=2
  [ "$n" = icon ] && size=1440,1180
  [ "$n" = icon-1024 ] && { size=1024,1024; scale=1; }
  prof=$(mktemp -d)
  "$CHROME" --headless=new --hide-scrollbars --force-device-scale-factor=$scale --window-size=$size \
    --default-background-color=00000000 --user-data-dir="$prof" --screenshot="$PWD/$n.png" "file://$PWD/$n.html" >/dev/null 2>&1
  rm -rf "$prof"
  echo "rendered $n.png"
done
[ -f icon-1024.png ] && mv icon-1024.png icon-1024px.png 2>/dev/null; true
