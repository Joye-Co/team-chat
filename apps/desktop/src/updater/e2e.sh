#!/usr/bin/env bash
# Update end to end against a local `wrangler dev`: publish a fake release DMG, then run
# `desktop --selftest-update` (check, download, verify, install over a fake bundle,
# relaunch, clean up). Nothing is installed and no window opens.
#
#   apps/desktop/src/updater/e2e.sh [port]        (default port 8806)
#
# Secrets go to ~/.config/team-chat/local-<port>/ (mode 600) and are never printed.
# Process files and the report live in <worktree>/.tmp/updater-e2e/.
set -euo pipefail

PORT="${1:-8806}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
WORK="$ROOT/.tmp/updater-e2e"
SECRETS="$HOME/.config/team-chat/local-$PORT"
URL="http://127.0.0.1:$PORT"
NAME="$(jq -r .name "$ROOT/product.json")"
BUNDLE_ID="$(jq -r .bundleId "$ROOT/product.json")"
VERSION="9.0.0"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"

rm -rf "$WORK" && mkdir -p "$WORK" "$SECRETS" && chmod 700 "$SECRETS"
umask 077
if [ ! -s "$SECRETS/admin-secret" ]; then
  openssl rand -hex 32 > "$SECRETS/admin-secret"
fi
chmod 600 "$SECRETS/admin-secret"
printf 'ADMIN_SECRET=%s\n' "$(cat "$SECRETS/admin-secret")" > "$ROOT/server/.dev.vars"

cd "$ROOT/server"
rm -rf .wrangler/state
bunx wrangler d1 migrations apply DB --local > "$WORK/migrate.log" 2>&1
bunx wrangler dev --port "$PORT" --inspector-port "$((PORT + 1000))" > "$WORK/wrangler.log" 2>&1 &
WRANGLER_PID=$!
echo "$WRANGLER_PID" > "$WORK/wrangler.pid"
cleanup() {
  if ps -p "$WRANGLER_PID" -o command= 2>/dev/null | grep -q wrangler; then
    kill "$WRANGLER_PID" 2>/dev/null || true
    wait "$WRANGLER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT
for _ in $(seq 1 120); do
  curl -sf "$URL/v1/health" > /dev/null 2>&1 && break
  sleep 0.5
done
curl -sf "$URL/v1/health" > /dev/null

# Owner token (never printed).
curl -sf -X POST "$URL/v1/admin/bootstrap" \
  -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H 'Content-Type: application/json' \
  -d '{"ownerName":"Updater"}' | jq -r .token > "$SECRETS/owner-token"
chmod 600 "$SECRETS/owner-token"

# Fake release: <name>.app whose executable writes to $UPDATER_TEST_OUT.
SRC="$WORK/src"
APP="$SRC/$NAME.app"
mkdir -p "$APP/Contents/MacOS"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$NAME</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
</dict></plist>
PLIST
printf '#!/bin/sh\necho started > "$UPDATER_TEST_OUT"\n' > "$APP/Contents/MacOS/$NAME"
chmod 755 "$APP/Contents/MacOS/$NAME"
ln -s /Applications "$SRC/Applications"
DMG="$WORK/$NAME-$VERSION.dmg"
hdiutil create -quiet -ov -format UDZO -fs HFS+ -volname "$NAME" -srcfolder "$SRC" "$DMG"
SHA="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"
NOTES="$(printf '测试版本' | base64)"
STATUS="$(curl -s -o "$WORK/publish.json" -w '%{http_code}' -X PUT "$URL/v1/admin/releases/$VERSION" \
  -H "X-Admin-Secret: $(cat "$SECRETS/admin-secret")" -H "X-Sha256: $SHA" \
  -H "X-Release-Notes: $NOTES" -H 'Content-Type: application/x-apple-diskimage' \
  --data-binary "@$DMG")"
echo "publish: HTTP $STATUS"
[ "${STATUS:0:1}" = 2 ]

"$CARGO_TARGET_DIR/debug/desktop" --selftest-update "$URL" --token-file "$SECRETS/owner-token" \
  --workdir "$WORK/run" --report "$WORK/report.json" && RC=0 || RC=$?
jq . "$WORK/report.json"
exit "$RC"
