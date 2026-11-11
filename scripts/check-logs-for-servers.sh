#!/usr/bin/env bash
# Fails when the log of a finished job of this workflow run shows a server host. The logs of a
# public repository are public, and a deployment's addresses must not be in them; the workflows
# mask them (a called workflow's secret plus ::add-mask::), and this proves it on every run.
#
# Usage (in GitHub Actions): scripts/check-logs-for-servers.sh
# Env: TC_SERVERS_JSON  { "<env>": "<url>", ... }; empty (a fork): nothing to look for
#      GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT
#      LOG_DIR          (tests) read *.log of this folder instead of asking GitHub
# Prints job ids and counts, never a host.
set -euo pipefail
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"
need jq

hosts="$(printf '%s' "${TC_SERVERS_JSON:-}" | jq -r '.. | strings' 2>/dev/null |
  sed -E 's#^[A-Za-z][A-Za-z0-9+.-]*://##; s#[/:?].*$##' | awk 'length($0) > 3' | sort -u || true)"
if [ -z "$hosts" ]; then
  log "no server URLs in this run: nothing to look for"
  exit 0
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/check-logs.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
if [ -n "${LOG_DIR:-}" ]; then
  cp "$LOG_DIR"/*.log "$WORK/"
else
  need gh
  # Finished jobs that ran (a skipped job has no log). This job itself is still running.
  jobs="$(gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/attempts/${GITHUB_RUN_ATTEMPT:-1}/jobs?per_page=100" \
    -q '.jobs[] | select(.status == "completed" and .conclusion != "skipped") | "\(.id)\t\(.name)"')"
  [ -n "$jobs" ] || die "this run has no finished job to read"
  missing=0
  while IFS=$'\t' read -r id name; do
    got=0
    for _ in 1 2 3; do
      if gh api "repos/$GITHUB_REPOSITORY/actions/jobs/$id/logs" >"$WORK/$id.log" 2>"$WORK/err"; then got=1; break; fi
      sleep 5
    done
    if [ "$got" = 1 ]; then
      printf 'read  %s (%s bytes)\n' "$name" "$(wc -c <"$WORK/$id.log" | tr -d ' ')"
    else
      # A log that cannot be read cannot be shown clean.
      printf '::error::cannot read the log of job %s: %s\n' "$name" "$(head -n 1 "$WORK/err" | cut -c1-200)"
      rm -f "$WORK/$id.log"
      missing=1
    fi
  done <<<"$jobs"
  [ "$missing" = 0 ] || die "some job logs could not be read"
fi

bad=0 read=0
for f in "$WORK"/*.log; do
  [ -f "$f" ] || continue
  read=$((read + 1))
  n="$(grep -cFf <(printf '%s\n' "$hosts") "$f" || true)"
  if [ "$n" != 0 ]; then
    printf '::error::the log of job %s shows a server address on %s line(s)\n' "$(basename "$f" .log)" "$n"
    bad=1
  fi
done
[ "$read" -gt 0 ] || die "no job log could be read"
[ "$bad" = 0 ] || die "server addresses are in the logs of this run: mask them before the step that prints them"
log "read $read job log(s): no server address in them"
