#!/usr/bin/env bash
# Adds a group's GitHub hook (POST /v1/hooks, kind github) as a webhook on a GitHub repository,
# then checks that GitHub's ping delivery succeeded. Run it only after the repo owner said yes.
#
#   scripts/github-webhook.sh                         # this repo, production hook file
#   scripts/github-webhook.sh --repo <owner/name> --hook-file <file>
#   scripts/github-webhook.sh --body-out <file>       # only write the request body (mode 600), no GitHub call
#
# The hook file is JSON with `url` and `secret` (made by POST /v1/hooks, kind github; see
# docs/integrations.md). The secret goes from that file into a temp file (mode 600) that
# `gh api --input` reads, and the temp file is deleted. The script never prints the secret.
set -euo pipefail

REPO="Joye-Co/team-chat"
HOOK_FILE="$HOME/.config/team-chat/production/github-hook.json"
BODY_OUT=""
EVENTS='["pull_request","pull_request_review","check_suite","workflow_run"]'

while [ $# -gt 0 ]; do
  case "$1" in
    --repo) REPO="$2"; shift 2 ;;
    --hook-file) HOOK_FILE="$2"; shift 2 ;;
    --body-out) BODY_OUT="$2"; shift 2 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 2 ;;
  esac
done

command -v jq >/dev/null || { echo "error: jq is required" >&2; exit 1; }
[ -f "$HOOK_FILE" ] || { echo "error: hook file not found: $HOOK_FILE" >&2; exit 1; }
mode=$(stat -f '%Lp' "$HOOK_FILE" 2>/dev/null || stat -c '%a' "$HOOK_FILE")
[ "$mode" = "600" ] || { echo "error: $HOOK_FILE must have mode 600 (has $mode)" >&2; exit 1; }
jq -e '(.url | type == "string") and (.url | test("^(https://|http://(127\\.0\\.0\\.1|localhost)[:/])"))
       and (.url | test("/v1/hooks/[^/]+/github$")) and (.secret | type == "string") and (.secret | length >= 16)' \
  "$HOOK_FILE" >/dev/null || { echo "error: $HOOK_FILE needs a github hook url and a secret" >&2; exit 1; }
URL=$(jq -r .url "$HOOK_FILE")

TMP=$(umask 077; mktemp "${TMPDIR:-/tmp}/github-webhook.XXXXXX")
trap 'rm -f "$TMP"' EXIT
jq --argjson events "$EVENTS" '{
  name: "web",
  active: true,
  events: $events,
  config: { url: .url, content_type: "json", secret: .secret, insecure_ssl: "0" }
}' "$HOOK_FILE" > "$TMP"

if [ -n "$BODY_OUT" ]; then
  (umask 077; cp "$TMP" "$BODY_OUT"); chmod 600 "$BODY_OUT"
  echo "request body (mode 600): $BODY_OUT"
  exit 0
fi

command -v gh >/dev/null || { echo "error: gh is required" >&2; exit 1; }
case "$URL" in https://*) ;; *) echo "error: GitHub needs an https hook url" >&2; exit 1 ;; esac

existing=$(gh api "repos/$REPO/hooks" --paginate --jq ".[] | select(.config.url == \"$URL\") | .id" | head -n 1)
if [ -n "$existing" ]; then
  echo "webhook $existing on $REPO already posts to this hook; nothing added"
  ID="$existing"
  gh api -X POST "repos/$REPO/hooks/$ID/pings" >/dev/null
else
  ID=$(gh api -X POST "repos/$REPO/hooks" --input "$TMP" --jq .id)
  echo "webhook $ID added on $REPO (events: $(jq -r 'join(", ")' <<<"$EVENTS"))"
fi
rm -f "$TMP"

# GitHub sends a ping right after it makes the hook. Wait for its delivery and check its status.
for _ in $(seq 1 20); do
  delivery=$(gh api "repos/$REPO/hooks/$ID/deliveries?per_page=20" \
    --jq '[.[] | select(.event == "ping")] | sort_by(.delivered_at) | last | select(. != null) | "\(.status_code) \(.status)"')
  if [ -n "$delivery" ]; then
    code=${delivery%% *}
    if [ "$code" = "200" ]; then
      echo "ping delivery: ok ($delivery)"
      exit 0
    fi
    echo "error: ping delivery failed: $delivery (see the repository's webhook settings, Recent Deliveries)" >&2
    exit 1
  fi
  sleep 3
done
echo "error: no ping delivery after 60 s (see the repository's webhook settings, Recent Deliveries)" >&2
exit 1
