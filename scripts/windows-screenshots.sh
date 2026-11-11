#!/usr/bin/env bash
# Runs the screenshot self-test of the built Windows client and writes the PNGs
# and screenshots.json to <dir>. CI's desktop-windows job calls it after
# `cargo build -p desktop` (docs/plans/windows.zh.md, 「怎么看 Windows 界面」).
#
#   scripts/windows-screenshots.sh <dir>
#
# SCENES: comma-separated scene names, `all` for every scene, empty for the
# main screens below.
set -euo pipefail

dir="${1:?usage: windows-screenshots.sh <dir>}"
exe="${CARGO_TARGET_DIR:-target}/debug/desktop.exe"
main_screens="login,main,main-sidebar-hidden,window-inactive,settings-shortcuts,group-chat,dm-chat,desk,topic-v2,group-v2,members-v2,artifacts-v2,new-topic,switcher,account-menu,search,cards,settings-profile,settings-about,about,files-quick-look,files-quick-look-handler,files-quick-look-page,settings-machine-created,media-video,media-audio,media-pdf"
scenes="${SCENES:-$main_screens}"

args=(--screenshot "$dir")
if [ "$scenes" != all ]; then
  args+=(--scenes "$scenes")
fi

mkdir -p "$dir"
# The log goes into the artifact too: a start failure leaves no PNG to look at.
set +e
"$exe" "${args[@]}" 2>&1 | tee "$dir/selftest.log"
code="${PIPESTATUS[0]}"
set -e
echo "screenshot self-test exit code: $code"
# The 产出 window is not a GPUI window: its own mode captures it from the screen (title
# bar in the theme's colors, the app's icon, the version picker), light and dark.
set +e
"$exe" --artifact-window "$dir" 2>&1 | tee -a "$dir/selftest.log"
window_code="${PIPESTATUS[0]}"
set -e
echo "artifact window exit code: $window_code"
if [ "$code" = 0 ]; then
  code="$window_code"
fi
if [ "$code" != 0 ]; then
  # A crash leaves no line in the log; Windows records the faulting module.
  powershell -NoProfile -Command "Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Application Error','Windows Error Reporting'} -MaxEvents 4 -ErrorAction SilentlyContinue | Format-List TimeCreated, ProviderName, Message" \
    2>&1 | tee -a "$dir/selftest.log" || true
fi
exit "$code"
