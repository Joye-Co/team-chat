#!/usr/bin/env bash
# Post one message over HTTP (POST /v1/groups/:id/messages). For cron jobs and CI scripts.
#
#   CHAT_URL=http://127.0.0.1:8789 CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-ci-<id>.token \
#     examples/notify.sh <groupId> "build 142 passed"
#
# Optional: TOPIC_ID (post in this topic; without it a new topic starts), TITLE (title of the
# new topic), CLIENT_MSG_ID (idempotency key; a retry with the same key does not post twice).
# Needs curl and jq. The token is read from the file and is never printed.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <groupId> <body>" >&2
  exit 2
fi
: "${CHAT_URL:?set CHAT_URL}"
: "${CHAT_TOKEN_FILE:?set CHAT_TOKEN_FILE}"
token_file="${CHAT_TOKEN_FILE/#\~/$HOME}"

group_id="$1"
body="$2"
client_msg_id="${CLIENT_MSG_ID:-$(uuidgen | tr '[:upper:]' '[:lower:]')}"

payload="$(jq -n \
  --arg body "$body" \
  --arg id "$client_msg_id" \
  --arg topic "${TOPIC_ID:-}" \
  --arg title "${TITLE:-}" \
  '{clientMsgId: $id, body: $body}
   + (if $topic != "" then {topicId: $topic} else {} end)
   + (if $title != "" and $topic == "" then {title: $title} else {} end)')"

# The header comes from a file so the token is not in the process list.
header_file="$(mktemp)"
# The answer goes to a file: curl starts the file again on each retry, so only the last answer is in it.
response_file="$(mktemp)"
trap 'rm -f "$header_file" "$response_file"' EXIT
chmod 600 "$header_file"
printf 'Authorization: Bearer %s\n' "$(tr -d '[:space:]' < "$token_file")" > "$header_file"

status=0
curl --silent --show-error --fail-with-body \
  --retry 3 --retry-connrefused --max-time 30 \
  -H @"$header_file" \
  -H 'Content-Type: application/json' \
  --data "$payload" \
  --output "$response_file" \
  "${CHAT_URL%/}/v1/groups/${group_id}/messages" || status=$?

if [[ $status -ne 0 ]]; then
  # The server's error as "code: message"; any other answer (a proxy page) as it came.
  if [[ -s "$response_file" ]]; then
    jq -er '.error | select(.) | "\(.code): \(.message)"' "$response_file" >&2 2>/dev/null || { cat "$response_file"; echo; } >&2
  fi
  exit "$status"
fi
jq -c '{messageId: .message.id, topicId: .message.topicId, duplicate: (.duplicate // false)}' "$response_file"
