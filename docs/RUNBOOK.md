# Runbook

Operations for the API Worker (`server/`) and the monitor Worker (`ops/monitor/`). Commands run
from the repository root unless a step says otherwise. Never paste a token or secret into a
command line, a log or a chat: read it from its file.

| Env | API Worker | D1 | R2 bucket | Monitor |
| --- | --- | --- | --- | --- |
| e2e | `chat-api-e2e` | `chat-e2e` | `chat-files-e2e` | not watched |
| staging | `chat-api-staging` | `chat-staging` | `chat-files-staging` | `chat-monitor` |
| production | `chat-api-prod` | `chat-prod` | `chat-files-prod` | `chat-monitor` |

Secret files live in `~/.config/team-chat/<env>/` (mode 600):

| File | What |
| --- | --- |
| `admin-secret` | the Worker secret `ADMIN_SECRET` of that env |
| `owner-token` | the owner's token (from `bun server/scripts/admin.ts bootstrap`) |
| `monitor-token` | the monitor bot's token in that env |

In the commands below, `ADMIN=~/.config/team-chat/production/admin-secret` and the admin header
is sent with `-H @<(printf 'x-admin-secret: %s' "$(cat $ADMIN)")`, so the secret never shows in
the process list or the shell history.

## Deploy

1. `git fetch && git status`: deploy only a commit that is on `origin/main` and green in CI.
2. Before a deploy that has a new D1 migration, save a D1 bookmark (see
   [D1 Time Travel](#d1-time-travel-restore)): `cd server && bunx wrangler d1 time-travel info chat-prod`.
3. Staging first: `cd server && bun run deploy:staging` (applies D1 migrations, then deploys).
4. Wait for two green monitor checks of staging (10 minutes, no alert in the ops group), or run
   the end-to-end clients in `tools/e2e` against staging.
5. Production: `cd server && bun run deploy:production`.
6. Watch the ops group for 15 minutes and the Workers logs (Cloudflare dashboard, Workers,
   `chat-api-prod`, Logs). Errors are JSON lines with `"level":"error"`.

Monitor deploy: `cd ops/monitor && bunx wrangler deploy`. First time only:

1. In each of staging and production: make the bot and keep its token in a file:
   `bun server/scripts/admin.ts bot --env production --name 监测 --role ops` (writes a token file
   and prints its path; rename it to `monitor-token`).
2. Make a group named `ops` (or set `opsGroup` in `ops/monitor/wrangler.jsonc` to its id) and add
   the monitor bot and the people on call to it.
3. `cd ops/monitor && bunx wrangler secret put MONITOR_TOKEN_PRODUCTION < ~/.config/team-chat/production/monitor-token`,
   the same for `MONITOR_TOKEN_STAGING`.
4. Deploy. Within 5 minutes a 监测往返 topic appears in each ops group; at 01:00 UTC the daily
   summary card appears in the 监测告警 topic.

## Roll back

A rollback changes the code only. D1 schema, D1 data, Durable Object data and R2 stay as they
are, so roll back only to a version that reads the current data (every migration is additive,
so the previous version normally does).

1. `cd server && bunx wrangler deployments list --env production` and pick the last good version.
2. `bunx wrangler rollback <version-id> --env production -m "<reason>"`.
3. Wrangler refuses a rollback across a Durable Object class migration. Then fix forward: revert
   the commit on `main` and deploy.
4. If data was damaged, restore it (sections below) after the code is good again.

The monitor rolls back the same way: `cd ops/monitor && bunx wrangler rollback <version-id>`.

## Rotate secrets and tokens

**ADMIN_SECRET** (after a leak, when a person with access leaves, or once a quarter):

1. `openssl rand -base64 32 > ~/.config/team-chat/production/admin-secret.new && chmod 600 $_`
2. `cd server && bunx wrangler secret put ADMIN_SECRET --env production < ~/.config/team-chat/production/admin-secret.new`
3. `mv ~/.config/team-chat/production/admin-secret.new ~/.config/team-chat/production/admin-secret`
4. Check: `curl -s -o /dev/null -w '%{http_code}\n' -H @<(printf 'x-admin-secret: %s' "$(cat ~/.config/team-chat/production/admin-secret)") https://chat-api-prod.huangdeshiou.workers.dev/v1/admin/import/d1` prints 200.

**A member's or bot's token** (lost Mac, leaked token):

- Revoke every token of the member: `bun server/scripts/admin.ts revoke --env production --member <id>`.
  Their open sockets close at once.
- A person signs in again with a new invite (`admin.ts invite`). A bot gets its new token from
  the bot's owner flow. Until a route issues a new token for an existing member, a revoked bot
  member cannot get a new token: make a new bot member and move it into its groups.
- **Monitor token**: make a new monitor bot (Deploy, first time, step 1), add it to `ops`, put
  the new secret, then revoke the old bot's tokens.

**Webhook secrets** (`POST /v1/hooks`): delete the hook and make a new one; update the sender.

## D1 Time Travel restore

D1 keeps every change for 30 days. A restore replaces the whole database; it cannot restore one
table. Members, tokens, groups, membership, files index and audit are in D1; messages are not
(they are in the group rooms).

1. Find the time just before the damage (Workers logs, audit log `GET /v1/audit`).
2. `cd server && bunx wrangler d1 time-travel info chat-prod --timestamp=<ISO time>` shows the bookmark.
3. Save the current state first so the restore can be undone: note the bookmark that
   `time-travel info` prints for now.
4. `bunx wrangler d1 time-travel restore chat-prod --timestamp=<ISO time>` (or `--bookmark=<b>`).
5. Check: `GET /v1/members` and `GET /v1/groups` as the owner; the monitor round trip passes.
6. Groups made after the restore point keep their room data but lose their D1 row: recreate
   them from the D1 archive dump, or accept the loss.

## Durable Object point-in-time recovery

Each group room (SQLite-backed Durable Object) can go back to any time in the last 30 days.

1. Dry run, which returns a bookmark and the current `lastSeq`:
   `curl -s -H @<(printf 'x-admin-secret: %s' "$(cat $ADMIN)") -H 'content-type: application/json' -d '{"at":<ms>}' https://chat-api-prod.huangdeshiou.workers.dev/v1/admin/pitr/groups/<groupId>`
2. Apply with `{"at":<ms>,"apply":true}`. The room restarts on the restored database; the action
   is in the audit log (`admin.pitr.group`). Every write after `at` is gone.
3. **Clients ahead of the restored log**: their cached `lastApplied` is above the room's new
   `lastSeq`, so they ignore new events with reused seqs. Every member of that group must drop
   the app's local cache for the env (sign out and in again, or delete the cache folder) after
   the restore. Same rule for an archive restore that is behind what clients saw.
4. Local `wrangler dev` has no point-in-time recovery; the route answers `invalid` there.

## Daily archive (R2)

The cron `0 3 * * *` on `chat-api-*` writes to the env's own bucket:

| Key | Content |
| --- | --- |
| `archive/groups/<groupId>/<from>-<to>.ndjson.gz` | stored events `from..to` of the group log, one JSON per line (seq, type, at, actorId, topicId, messageId, data); at most 5,000 per file |
| `archive/groups/<groupId>/state/<YYYY-MM-DD>.json.gz` | per-member room state: per-topic read positions, follow flags, mute and notify preferences |
| `archive/d1/<YYYY-MM-DD>.json.gz` | every D1 table in creation order (`d1_migrations` and `ops_archive` left out); tokens and invites only as SHA-256 hashes; columns named like a plain secret are written as `[redacted]` |

- The last archived seq per group is in D1 `ops_archive`. A run stops after 10 minutes and the
  next run goes on; each file is written before its state moves, so a crash only makes the next
  file overlap, and restore skips the overlap. Deleted messages are already scrubbed in the log.
- Run it now: `curl -s -X POST -H @<(printf 'x-admin-secret: %s' "$(cat $ADMIN)") https://chat-api-prod.huangdeshiou.workers.dev/v1/admin/archive/run`
  (budget 20 s per call; repeat while `complete` is false).
- List: `GET /v1/admin/archive?prefix=archive/d1/`. Download one object:
  `GET /v1/admin/archive/object?key=<key>`. Copy all of it to a folder:
  `bun ops/restore/restore.ts fetch --from production --out /tmp/archive-prod`.
- Not archived: uploaded file bytes and 产出 versions (they stay in R2, which is durable),
  each member's group read cursor (in their gateway), and in-memory presence and typing.

## Restore from the R2 archive

`ops/restore/restore.ts` restores into a target env through its admin routes. Every step is
idempotent: run the same command again after a failure. The target must run the same or a newer
server version, with its D1 migrations applied.

**One group** (its room lost or damaged; its D1 row and members exist in the target):

1. If the room still has data, it cannot be imported over: use point-in-time recovery, or
   restore into a new group (make it, then pass its id; the archive's events carry no group id).
2. `bun ops/restore/restore.ts group --from production --to production --group <groupId>`
3. Members reset their cache for that group (see point-in-time recovery, step 3).

**A whole environment** (lost D1 and rooms, or a copy into a fresh env):

1. Make the target empty: a new D1 (`wrangler d1 create chat-<name>`), its id in
   `server/wrangler.jsonc`, `bunx wrangler d1 migrations apply DB --env <env> --remote`, deploy.
2. If the source Worker is down, download `archive/` from the R2 dashboard into a folder and use
   `--from-dir <folder>` instead of `--from <env>`.
3. `bun ops/restore/restore.ts env --from production --to staging` (D1 first, then each group's
   events, then its state). `--date YYYY-MM-DD` picks an older D1 dump and state snapshot.
   `--resume` goes on with a D1 restore that stopped.
4. Tokens keep working (their hashes are restored), so clients reconnect by themselves; they
   reset their cache only when the target is behind what they saw.
5. Check: the owner's `GET /v1/groups` shows every group with the expected `lastSeq`; the
   monitor round trip passes.

**Restore drill** (run after any change to the archive, import or room tables, and once a
month): `bun ops/restore/drill.ts`. It starts a local stack A on port 8798, fills it through the
public API, archives, stops A, starts an empty stack B, restores the whole env from A's archive
and compares members, groups, every event and every topic's history as two members see them.
Exit 0 means equal; the report is `ops/restore/.drill/report.json`.

## Monitor alerts

Every 5 minutes `chat-monitor` checks each target: `GET /v1/health`, then the monitor bot opens
the WebSocket, sends a message in its 监测往返 topic of the ops group and waits for its own
`message.created` event. Two failed checks in a row (`FAIL_THRESHOLD`) raise one **不可用** card;
the next good check raises one **已恢复** card with the outage length. Nothing is posted in
between. Cards go to the 监测告警 topic of the ops group of the first healthy env in `TARGETS`
order (production, then staging); when no env is healthy they wait in the monitor and are posted
when one is back (dropped after 3 days).

| Reason in the card | Means | First step |
| --- | --- | --- |
| `network error` | the Worker URL does not answer (DNS, TLS, Cloudflare outage, Worker deleted) | Cloudflare status page; `wrangler deployments list` |
| `health: 5xx` or `health: timeout` | the Worker runs but fails or hangs at once | Workers logs; roll back the last deploy |
| `websocket: 401` | the monitor token was revoked or the D1 tokens table is damaged | `admin.ts members`; D1 Time Travel if tokens are gone |
| `websocket: 5xx`, `welcome: …` | the member gateway Durable Object fails | Workers logs for `MemberGateway` errors |
| `round trip: monitor bot is not in the ops group` | membership changed | add the bot to `ops` again |
| `round trip: no monitor token configured` | the Worker secret is missing | `wrangler secret put MONITOR_TOKEN_<ENV>` |
| `round trip ack: timeout` or `round trip: internal` | the group room fails or is overloaded | Workers logs for `GroupRoom`; roll back |
| `round trip event: timeout` | the message was stored but fan-out to gateways is broken: people do not get messages live | Workers logs for `deliver failed`; roll back |
| `round trip: rate_limited` | someone else uses the monitor token | revoke it and make a new monitor bot |

**Daily summary** (01:00 UTC, 09:00 Beijing time): per env the status, number of checks and
failures in 24 hours, availability and round-trip p50/p95. A missing summary means the monitor
itself is down: check `chat-monitor` in the dashboard (Workers, Logs, cron events).

## Incident checklist

1. **Acknowledge** in the ops group: who handles it, start time.
2. **Scope**: which env; health or round trip; one group or all (try another group with
   the `tools/e2e` clients); since which deploy (`wrangler deployments list`).
3. **Stop the damage**: a bad deploy → roll back first, debug later. A leaked secret → rotate it
   now (see Rotate). Data being destroyed → stop writers (revoke the credential that writes).
4. **Restore data** only after the code is good: D1 Time Travel, room point-in-time recovery, or
   the R2 archive, in that order of preference (newest data first).
5. **Verify**: the monitor's 已恢复 card; the owner and one intern see the latest messages; a
   bot replies.
6. **Tell the team** in the ops group what happened, what was lost (time range), and whether
   members must reset their cache.
7. **Follow up** within two days: a short write-up in the ops group (cause, fix, what to change),
   and a task for each change.
