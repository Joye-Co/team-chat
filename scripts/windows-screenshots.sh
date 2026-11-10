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
main_screens="login,main,group-chat,dm-chat,desk,topic-v2,group-v2,members-v2,artifacts-v2,new-topic,switcher,account-menu,search,cards,settings-profile,settings-about,media-audio,media-pdf"
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
exit "$code"
