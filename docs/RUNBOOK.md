# Runbook

Operations for the API Worker (`server/`) and the monitor Worker (`ops/monitor/`). Commands run
from the repository root unless a step says otherwise. Never paste a token or secret into a
command line, a log or a chat: read it from its file.

| Env | API Worker | D1 | R2 bucket | Monitor |
| --- | --- | --- | --- | --- |
| e2e | `chat-api-e2e` | `chat-e2e` | `chat-files-e2e` | not watched |
| staging | `chat-api-staging` | `chat-staging` | `chat-files-staging` | `chat-monitor` |
| production | `chat-api-prod` | `chat-prod` | `chat-files-prod` | `chat-monitor` |

Production URL: `https://chat-api-prod.huangdeshiou.workers.dev` (`product.json`
`servers.production`). Monitor: `chat-monitor` (cron, plus one operator route: `POST /test-alert`,
see [Test the alert path](#test-the-alert-path)).

Secret files live in `~/.config/team-chat/<env>/` (folder mode 700, files mode 600). The full
list is in [ENVIRONMENTS.md](ENVIRONMENTS.md#secret-files).

| File | What |
| --- | --- |
| `admin-secret` | the Worker secret `ADMIN_SECRET` of that env |
| `hook-secret-key` | the Worker secret `HOOK_SECRET_KEY` (encrypts hook secrets in D1) |
| `owner-token` (and its copy `owner.token`) | the owner's token (from `bun server/scripts/admin.ts bootstrap`) |
| `monitor-token` | the monitor bot's token in that env (Worker secret `MONITOR_TOKEN_<ENV>` of `chat-monitor`) |
| `bots/<name>.token` | each roster bot's token |
| `github-hook.json` | production: URL and secret of the GitHub hook of `开发` |
| `monitor-admin-secret` | production folder only: the Worker secret `MONITOR_ADMIN_SECRET` of `chat-monitor` (bearer of `POST /test-alert`) |

In the commands below, `ADMIN=~/.config/team-chat/production/admin-secret` and the admin header
is sent with `-H @<(printf 'x-admin-secret: %s' "$(cat $ADMIN)")`, so the secret never shows in
the process list or the shell history.

## Deploy

1. `git fetch && git status`: deploy only a commit that is on `origin/main` and green in CI.
2. Before a deploy that has a new D1 migration, save a D1 bookmark (see
   [D1 Time Travel](#d1-time-travel-restore)): `cd server && bunx wrangler d1 time-travel info chat-prod`.
3. Staging first: `cd server && bun run deploy:staging` (applies D1 migrations, then deploys).
   The deploy script (`scripts/deploy-server.sh`) sets the `VERSION` var that `GET /v1/health`
   reports: the release version when HEAD has a release tag `v<semver>`, otherwise the
   12-character commit (`-dirty` when `server/`, `packages/` or `protocol/` have uncommitted
   changes); `DEPLOY_VERSION=<v>` overrides it. See it first with
   `scripts/deploy-server.sh staging --print-version`. A `0.0.0*` version is refused, and the
   script fails unless `/v1/health` reports the deployed version within 60 seconds; check it
   again any time with `scripts/deploy-server.sh staging --check-health --expect <v>`. Before a
   release, `scripts/release-check.sh <v> --env staging` checks the versions, the changelog and
   the notes as well.
4. Wait for two green monitor checks of staging (10 minutes, no alert in the `运维` group), or run
   the end-to-end clients in `tools/e2e` against staging.
5. Production: `cd server && bun run deploy:production`.
6. Watch the `运维` group for 15 minutes and the Workers logs (Cloudflare dashboard, Workers,
   `chat-api-prod`, Logs). Errors are JSON lines with `"level":"error"`.

Monitor deploy: `cd ops/monitor && bunx wrangler deploy`. The monitor reaches `chat-api-prod`
and `chat-api-staging` through service bindings (a Worker cannot fetch another Worker on the
same workers.dev subdomain: Cloudflare error 1042), so both API Workers must exist before the
first monitor deploy; wrangler refuses a binding to a missing Worker. First time only:

Done for staging and production on 2026-11-03 (bot `监测`, ops group `运维`, probe group `监测`).

1. In each of staging and production: make the bot and keep its token in a file:
   `bun server/scripts/admin.ts bot --env production --name 监测 --role 监测` (writes a token file
   and prints its path; rename it to `monitor-token`).
2. Make the ops group `运维` (`opsGroup` in `ops/monitor/wrangler.jsonc` is its exact name; an id
   also works) and add the monitor bot and the people on call to it. Every human member of `运维` is mentioned on
   alert cards (a group notifies only on mentions by default); set `oncall` (member ids of that
   env) in `TARGETS` to mention fewer people.
3. Make a group named `监测` with only the monitor bot in it (`probeGroup`): make it with the
   monitor bot's own token (`POST /v1/groups` `{ "name": "监测", "memberIds": [] }`), so no person
   is in it. The round-trip message every 5 minutes goes there, not into `运维`, so `运维` holds
   only alerts and summaries.
4. `cd ops/monitor && bunx wrangler secret put MONITOR_TOKEN_PRODUCTION < ~/.config/team-chat/production/monitor-token`,
   the same for `MONITOR_TOKEN_STAGING`.
5. Deploy. Within 5 minutes a 监测往返 topic appears in each `监测` group and the Workers logs of
   `chat-monitor` show `"msg":"check"` lines with `"ok":true` for both targets (a failing first
   check shows its reason there); at 01:00 UTC the daily summary card appears in the 监测告警
   topic of `运维`.
6. Before the first deploy, run one archive pass on each env (Daily archive, "Run it now"): a new
   env has no complete archive, and the monitor raises **备份过期** after two checks.

## Quality smoke

`bun scripts/staging-smoke.ts [--env staging] [--upload-mb 100] [--samples 50] [--warmup 2]`
measures the V1 bar of
[docs/PRODUCT.md](PRODUCT.md#v1-production-use-by-5-people-and-5-bots) against a deployed env with
the owner token and a bot token (it uses one group named `smoke`, made once). Report:
`.tmp/staging-smoke/report.json` (or `--out <folder>`), no tokens; each timed check lists every
sample in `samples` (ms). Run it after each staging deploy, before production, and run the
read-only WebSocket check first
(`bun scripts/ws-check.ts https://chat-api-staging.huangdeshiou.workers.dev ~/.config/team-chat/staging/owner-token`):
it fetches every group, so each room starts and runs its new named migrations before the smoke
measures it. The first access to a room after a deploy is slow (1.1.0 on staging, 2026-11-05:
search p95 418 ms on the first smoke after the deploy, 88–114 ms after). Each timed loop also
starts with `--warmup` untimed rounds. After a production deploy, run `ws-check` against
production at once for the same reason. `--base http://127.0.0.1:<port>` with `--owner-token` and `--bot-token`
files runs it against a local `wrangler dev`.

| Check | Bar | Staging, 2026-11-04 (server 1.0.0) |
| --- | --- | --- |
| Delivery, owner send to the bot's live event | p95 < 300 ms | p95 91 ms (20 samples; WebSocket ping 21 ms) |
| Search for a Chinese two-character word | p95 < 300 ms | p95 66 ms (20 samples, one group and all groups) |
| Catch-up: hello, then every group fetched from seq 0 | < 2 s | 498 ms (6 groups, 136 events) |
| Reconnect with a cursor after 5 missed messages | none lost or doubled | 0 lost, 0 doubled |
| Live notification for a mention | `notify: "mention"` | `mention` |
| 100 MB upload, read back | same size and SHA-256 | upload 31 s, download 3.8 s, same bytes |
| Card v2 ask from the bot (`ask.from` requester and owners), send to the owner's live event | p95 < 300 ms | not measured on staging yet (needs a 1.1 server); local `wrangler dev`: p95 6–8 ms (50 samples) |

Not proved by this script, so not claimed as measured: the reconnect after macOS sleep and the
notification banner on screen (app behavior, checked by hand on the app), a 10,000-message topic
opening in under 100 ms, 60 fps scrolling and cold start under 1 s (desktop, not measured on
1.1.0). The catch-up row fetches the whole staging log, which is smaller than a busy team day.

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

## Release from GitHub Actions

The **Release** workflow (`.github/workflows/release.yml`, Actions tab, manual) runs
`scripts/release.sh` on a macOS runner, so a release needs no particular Mac. Write
`docs/releases/<v>.md` and the `## <v> (unreleased)` CHANGELOG section on `main` first, as for a
local release.

```sh
gh workflow run release.yml --ref <branch> -f version=<v>                 # dry run on any branch
gh workflow run release.yml --ref main -f version=<v> -f mode=release     # the release
gh workflow run release.yml --ref main -f version=<v> -f mode=upload-only # upload again
```

- `dry-run` (default): preflight and plan, then builds, signs and checks the DMG and the daemon
  (against the previous release's DMG too) and keeps them as a workflow artifact for 7 days.
  Nothing is committed, tagged or uploaded. "would stop" lines name what a release would refuse.
- `release` (main only): `chore(release): v<v>` on `main`, tag `v<v>`, GitHub Release with the
  DMG, the daemon and the `.sha256` files, then `PUT /v1/admin/releases/<v>` on production.
  The release commit is pushed with the workflow token, which starts no CI run on `main`.
- `upload-only` (main only): downloads the DMG of release `v<v>`, checks its `.sha256` and uploads
  it to production again.

Repository secrets (Settings, Secrets and variables, Actions). The workflow never prints them:

| Secret | Value |
| --- | --- |
| `SIGNING_CERT_PEM` | the whole of `~/.config/team-chat/signing/cert.pem` |
| `SIGNING_KEY_PEM` | the whole of `~/.config/team-chat/signing/key.pem` |
| `ADMIN_SECRET_PRODUCTION` | the content of `~/.config/team-chat/production/admin-secret` |

Add them from the files, so they never pass through the clipboard or the shell history:
`gh secret set SIGNING_CERT_PEM < ~/.config/team-chat/signing/cert.pem`, the same for
`SIGNING_KEY_PEM` with `key.pem` and `ADMIN_SECRET_PRODUCTION` with `production/admin-secret`.
A release refuses to run without the two signing secrets (an ad-hoc signature changes the
designated requirement and the Keychain stops trusting the app) or with a key that does not
belong to the certificate; a non-prerelease release and `upload-only` also refuse without
`ADMIN_SECRET_PRODUCTION`. A dry run only warns and signs ad hoc. After rotating `ADMIN_SECRET`
(below), set `ADMIN_SECRET_PRODUCTION` again.

## Bad app release

A release that breaks the app (crash at launch, lost sign-in) must stop being offered at once:
every client checks for updates every 6 hours.

1. Withdraw it: `bun server/scripts/admin.ts withdraw-release --env production --version <v>`.
   `GET /v1/updates/latest` offers the previous good release again and the bad DMG cannot be
   downloaded. Clients that did not install it yet stay on their version.
2. Tell the people who installed it in the `运维` group: download the previous DMG from the
   GitHub release of that version and drag it over the app. The app does not go back by
   itself, and it never offers an older version as an update.
3. Fix forward: release a higher version with `scripts/release.sh` or the Release workflow; it
   is offered to everyone.
4. A withdrawn version cannot be uploaded again. `withdraw-release ... --restore` offers it
   again if it was withdrawn by mistake.

## Rotate secrets and tokens

**ADMIN_SECRET** (after a leak, when a person with access leaves, or once a quarter):

1. `(umask 077; openssl rand -base64 32 | tr -d '\n' > ~/.config/team-chat/production/admin-secret.new)`
   (the file is mode 600 from the start; `ops/restore` refuses a secret file with another mode)
2. `cd server && bunx wrangler secret put ADMIN_SECRET --env production < ~/.config/team-chat/production/admin-secret.new`
3. `mv ~/.config/team-chat/production/admin-secret.new ~/.config/team-chat/production/admin-secret`,
   then `gh secret set ADMIN_SECRET_PRODUCTION < ~/.config/team-chat/production/admin-secret`
   for the Release workflow.
4. Check: `curl -s -o /dev/null -w '%{http_code}\n' -H @<(printf 'x-admin-secret: %s' "$(cat ~/.config/team-chat/production/admin-secret)") https://chat-api-prod.huangdeshiou.workers.dev/v1/admin/import/d1` prints 200.

**A member's or bot's token** (lost Mac, leaked token):

- Revoke every token of the member: `bun server/scripts/admin.ts revoke --env production --member <id>`.
  Their open sockets close at once.
- A person signs in again with a sign-in code for their member (same member, new token):
  `curl -s -H @<(printf 'authorization: Bearer %s' "$(cat ~/.config/team-chat/production/owner-token)") -H 'content-type: application/json' -d '{"name":"<name>","kind":"human","level":"member","memberId":"<id>"}' https://chat-api-prod.huangdeshiou.workers.dev/v1/invites | jq -r .code > <file>`
  (make `<file>` with `umask 077` first; the code is a secret). A sign-in code is refused for a
  deactivated member. A bot gets its new token from
  the bot's owner flow. Until a route issues a new token for an existing member, a revoked bot
  member cannot get a new token: make a new bot member and move it into its groups.
- **Monitor token**: make a new monitor bot (Deploy, first time, step 1), add it to `运维`, put
  the new secret, then revoke the old bot's tokens.

**Webhook secrets** (`POST /v1/hooks`): delete the hook and make a new one; update the sender.
For the GitHub hook of `开发`: `DELETE /v1/hooks/<hookId>` (id in `github-hook.json`), make a new
github hook for the `开发` group, write its `{ hookId, groupId, url, secret, repo }` to
`~/.config/team-chat/production/github-hook.json` (mode 600), delete the old webhook in the
GitHub repository settings, then run `scripts/github-webhook.sh`.

**HOOK_SECRET_KEY**: it cannot be changed in place: every hook secret in D1 is encrypted with it.
After a leak: put a new key (as for ADMIN_SECRET, file `hook-secret-key`), then delete and make
again every hook (`GET /v1/hooks`) and update each sender.

**Invite and sign-in codes** expire 7 days after they are made and are single use. An unused code
that leaked cannot be revoked by a route: it stops working when it expires, or when the
member it was made for is deactivated. Make a new one with `admin.ts invite`.

## D1 Time Travel restore

D1 keeps every change for 30 days. A restore replaces the whole database; it cannot restore one
table. Members, tokens, groups, membership, files index and audit are in D1; messages are not
(they are in the group rooms).

**A restore also undoes revocations.** Token revokes, member deactivations, machine revokes and
hook deletes made after the restore point are lost with the rest, and the audit entries that
recorded them too. A token you revoked to stop an attacker works again. So save them first and
apply them again after (steps 3 and 6). Both commands print counts and a file path only.

1. Find the time just before the damage (Workers logs, audit log `GET /v1/audit`).
2. `cd server && bunx wrangler d1 time-travel info chat-prod --timestamp=<ISO time>` shows the bookmark.
3. Save the current state first so the restore can be undone: note the bookmark that
   `time-travel info` prints for now. Save the revocations:
   `bun server/scripts/admin.ts revocations-save --env production` (writes
   `~/.config/team-chat/production/revocations-<time>.json`, mode 600).
4. `bunx wrangler d1 time-travel restore chat-prod --timestamp=<ISO time>` (or `--bookmark=<b>`).
5. **Required**: `bun server/scripts/admin.ts revocations-apply --env production --file <that file>`.
   It sets the saved revokes and deactivations again and closes the sockets they opened.
6. Check: `GET /v1/members` and `GET /v1/groups` as the owner; the monitor round trip passes;
   a revoked token gets 401.
7. Groups made after the restore point keep their room data but lose their D1 row: recreate
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
5. The archive follows the restore by itself: the apply moves the group's archive files above the
   restore point to `archive/groups/<groupId>/superseded/<ms>/` (kept, never restored; delete
   them in the R2 dashboard when nobody needs them), keeps the part of a file up to that point,
   and moves the group's archive position back (`archiveFrom` in the answer). The next archive
   run then writes the new events, which reuse the seqs above that point.

## Daily archive (R2)

The cron `0 3 * * *` on `chat-api-*` writes to the env's own bucket:

| Key | Content |
| --- | --- |
| `archive/groups/<groupId>/<from>-<to>.ndjson.gz` | stored events `from..to` of the group log, one JSON per line (seq, type, at, actorId, topicId, messageId, data); at most 5,000 per file |
| `archive/groups/<groupId>/state/<YYYY-MM-DD>.json.gz` | per-member room state: per-topic read positions, follow flags, mute and notify preferences |
| `archive/members/<memberId>/state/<YYYY-MM-DD>.json.gz` | each member gateway's group read cursors and GitHub hook topic map (`hook_keys`, `hook_shas`, `hook_deliveries`), once a day after a pass reached every group |
| `archive/d1/<YYYY-MM-DD>.json.gz` | every D1 table in creation order (`d1_migrations` and the `ops_*` tables left out); tokens and invites only as SHA-256 hashes; columns named like a plain secret are written as `[redacted]` (`hooks.secret_enc` is kept: it is encrypted with the env's `HOOK_SECRET_KEY`) |

- The last archived seq per group is in D1 `ops_archive`; each file is written before that seq
  moves, so a crash only makes the next file overlap, and restore skips the overlap.
- A run stops at its budget (10 minutes for the cron) and the next run goes on after the last
  group it finished (D1 `ops_meta`, `archive.cursor`). Every run finishes at least one group.
- The state snapshot is written once a day per group, and again on a day the group has new
  events.
- One failing group does not stop the others: it is logged (`"msg":"group archive failed"` with
  its `groupId`), listed in `failed`, and retried by the next run from its last seq.
- `GET /v1/ops/archive` (any member token) answers `lastCompleteAt`, the end of the newest pass
  that reached every group with no failure, and `lastRun`. The monitor raises **备份过期** when
  `lastCompleteAt` is more than 26 hours old or missing, and **备份已恢复** when it is fresh again.
- **Deleted messages**: a delete scrubs the body and card from the room log at once. Archive
  files written before the delete still hold them until the next archive run, which rewrites
  those files (`scrubbed` in its answer). So deleted text stays in R2 for at most one day (or
  until the next manual run), and a restore from any older copy of the files also drops it,
  because importing the delete event scrubs the imported rows. Superseded files (after a
  point-in-time recovery) are not rewritten.
- Run it now: `curl -s -X POST -H @<(printf 'x-admin-secret: %s' "$(cat $ADMIN)") https://chat-api-prod.huangdeshiou.workers.dev/v1/admin/archive/run`
  (budget 20 s per call; repeat while `complete` is false: each call goes on where the last one
  stopped). `failed` lists groups that failed in this call; look them up in the Workers logs.
- List: `GET /v1/admin/archive?prefix=archive/d1/`. Download one object:
  `GET /v1/admin/archive/object?key=<key>`. Copy all of it to a folder:
  `bun ops/restore/restore.ts fetch --from production --from-url https://chat-api-prod.huangdeshiou.workers.dev --out /tmp/archive-prod`
  (`--from-url` and `--to-url` are optional for staging and production: the URLs are in
  `product.json`).
- 产出 and their versions are rebuilt from their events; ready-file records from the D1 `files`
  table. Not archived:
  - uploaded file bytes (`files/`, `avatars/`, `releases/` in R2). They stay in the env's
    bucket, which is durable. A restore into **another** env's bucket does not copy them: copy
    those prefixes with the R2 dashboard or an S3 client, or attachments and 产出 files answer
    404 there.
  - in-memory presence and typing.
- Member gateway state is a daily snapshot: read positions and hook topics made after it are
  lost in an env restore. A lost cursor makes that group show its newer history as unread; a
  lost hook topic makes the next event of that pull request open a second topic.

## Restore from the R2 archive

`ops/restore/restore.ts` restores into a target env through its admin routes. Every step is
idempotent: run the same command again after a failure. The target must run the same or a newer
server version, with its D1 migrations applied.

Last real-archive check: 2026-11-04. Staging's archive (`lastRun` 2026-11-04 03:02 UTC, complete,
0 failed) was copied with `restore.ts fetch --from staging` (165 objects) and restored with
`restore.ts env --from-dir` into an empty local `wrangler dev`: 418 D1 rows, 18 groups,
1,568 events, 0 failures.

**One group** (its room lost or damaged; its D1 row and members exist in the target):

1. If the room still has data, it cannot be imported over: use point-in-time recovery, or copy
   the archive into another group (below).
2. `bun ops/restore/restore.ts group --from production --from-url https://chat-api-prod.huangdeshiou.workers.dev --to production --to-url https://chat-api-prod.huangdeshiou.workers.dev --group <groupId>`
   (events, then room state, then the group's ready-file records from D1).
3. Members reset their cache for that group (see point-in-time recovery, step 3).

**One group's archive into another group** (`--from-group`): for example the room of group A
holds damaged data you must keep for a look, or the history of a deleted or damaged group must
come back next to it.

1. Make the target group in the target env with the members who may read the copy
   (`POST /v1/groups` as the owner, `{ "name": "<name>", "memberIds": [...] }`). Do not post in
   it: the import needs a group with no events. Its members come from D1, not from the archive,
   so only the members you add see the copy.
2. `bun ops/restore/restore.ts group --from production --to production --group <targetGroupId> --from-group <sourceGroupId>`
   (add `--date YYYY-MM-DD` for an older room state). It reads every archive file of the source
   group, gives the target group id, and new ids to every topic, message, 产出 and file of the
   source (the app keys them by id, so a copy next to its source must not reuse them), keeps the
   seqs, then writes the events and the room state into the target. Then
   `POST /v1/admin/import/groups/<target>/copy-files` (20 files and 产出 per call) copies the D1
   file and 产出 rows and the R2 bytes of the source under the new ids, so attachments and 产出
   of the copy open. A re-run makes the same ids, imports nothing twice and copies no byte twice.
   The answer has `fromGroup`, `imported`, `lastSeq` and `copiedFiles`.
3. Limits: the source group's D1 rows and R2 objects must be in the target env (same env, or
   after a whole-env restore); files whose source row or object is gone are counted as `missing`
   and stay 404 in the copy. Links to the source in message text are not rewritten. Member read
   cursors of the copy start at 0 (all unread).
4. The drill tests this (below).

**A whole environment** (lost D1 and rooms, or a copy into a fresh env):

1. Make the target empty: a new D1 (`wrangler d1 create chat-<name>`), its id in
   `server/wrangler.jsonc`, `bunx wrangler d1 migrations apply DB --env <env> --remote`, deploy.
2. If the source Worker is down, download `archive/` from the R2 dashboard into a folder and use
   `--from-dir <folder>` instead of `--from <env>`.
3. Give the target the source's `HOOK_SECRET_KEY` (hook secrets in D1 are encrypted with it; with
   another key every hook fails its signature check):
   `cd server && bunx wrangler secret put HOOK_SECRET_KEY --env <target> < ~/.config/team-chat/<source>/hook-secret-key`.
4. If the source still answers, save its revocations (the D1 dump is up to a day older than
   them): `bun server/scripts/admin.ts revocations-save --env production --url https://chat-api-prod.huangdeshiou.workers.dev`.
5. `bun ops/restore/restore.ts env --from production --from-url https://chat-api-prod.huangdeshiou.workers.dev --to staging --revocations <that file>`
   (D1 first and the revocations right after it, then each group's events, its state and its
   ready-file records, then each member gateway's state). `--date YYYY-MM-DD` picks an older D1 dump and state snapshot.
   `--resume` goes on with a D1 restore that stopped. Without a revocations file, revoke again
   by hand every token and member revoked after the dump (audit log of the source).
6. Into another env's bucket: copy the file bytes (see "Not archived" above).
7. Tokens keep working (their hashes are restored), so clients reconnect by themselves; they
   reset their cache only when the target is behind what they saw.
8. Check: the owner's `GET /v1/groups` shows every group with the expected `lastSeq`; one 产出
   opens; the monitor round trip passes.

**Restore drill** (run after any change to the archive, import or room tables, and once a
month): `bun ops/restore/drill.ts [--port 8798]`. It starts a local stack A, fills it through the
public API (messages, an attachment, a 产出 with two versions), archives, deletes one message,
archives again, stops A, starts stack B with empty D1 and rooms (A's R2 bucket stays), restores
the whole env from A's archive and compares members, groups, every event, every topic's history
and every 产出 with its files as two members see them. It also checks that the late-deleted
text is in no archive file and that B takes a new message, an old attachment and a new 产出
version. Then it copies the group's archive into a new empty group (`--from-group`), runs the copy
twice, and checks the copy: same events, topic titles and messages, no reused topic or message
id, no source group id, the source group unchanged, the copy's 产出 open with the same pages
and its attachment downloads under a new id with the right SHA-256, and a new message in the
copy continues the seq (`groupCopy` in the report). Exit 0 means all pass; the report is
`ops/restore/.drill/report.json`. Other agents run local stacks at the same time: pass a free
`--port` (the inspector uses that port plus 1000).

## Monitor alerts

Every 5 minutes `chat-monitor` checks each target: `GET /v1/health`, then the monitor bot opens
the WebSocket, sends a message in its 监测往返 topic of the `监测` group and waits for its own
`message.created` event, then reads the archive freshness (`GET /v1/ops/archive`). Two failed
checks in a row (`FAIL_THRESHOLD`) raise one **不可用** card (or **备份过期** for the archive);
the next good check raises one **已恢复** card with the outage length. Nothing is posted in
between. Cards go to the 监测告警 topic of the ops group (`运维`) of the first healthy env in `TARGETS`
order (production, then staging) and mention the people of that ops group; when no env is
healthy they wait in the monitor and are posted when one is back (dropped after 3 days).

| Reason in the card | Means | First step |
| --- | --- | --- |
| `network error` | the Worker URL does not answer (DNS, TLS, Cloudflare outage, Worker deleted) | Cloudflare status page; `wrangler deployments list` |
| `health: 5xx` or `health: timeout` | the Worker runs but fails or hangs at once | Workers logs; roll back the last deploy |
| `websocket: 401` | the monitor token was revoked or the D1 tokens table is damaged | `admin.ts members`; D1 Time Travel if tokens are gone |
| `websocket: 5xx`, `welcome: …` | the member gateway Durable Object fails | Workers logs for `MemberGateway` errors |
| `round trip: monitor bot is not in the `运维` group` | membership changed | add the bot to `运维` again |
| `round trip: no monitor token configured` | the Worker secret is missing | `wrangler secret put MONITOR_TOKEN_<ENV>` |
| `round trip ack: timeout` or `round trip: internal` | the group room fails or is overloaded | Workers logs for `GroupRoom`; roll back |
| `round trip event: timeout` | the message was stored but fan-out to gateways is broken: people do not get messages live | Workers logs for `deliver failed`; roll back |
| `round trip: rate_limited` | someone else uses the monitor token | revoke it and make a new monitor bot |
| `round trip: monitor bot is not in the probe group` | the `监测` group is missing or the bot left it | make it or add the bot again |
| `config: service binding … is missing` | the monitor was deployed without its `services` | check `ops/monitor/wrangler.jsonc`; deploy again |
| `backup: no complete archive yet`, `backup: last complete archive N h ago` | the daily archive did not finish a pass | Workers logs of the API for `group archive failed` or `archive run`; run the archive by hand (Daily archive) |

### Test the alert path

`POST /test-alert` on `chat-monitor` sends one alert card worded as a test (**【测试】<env>
告警测试**, tone info, label 测试, "不需要处理") through the code of a real alert: it probes the
targets as a cron run does (without recording the checks, so alert state does not change),
picks the sink env, builds the card, mentions the people of that ops group, and posts it into the
监测告警 topic of `运维`. Run it after a monitor deploy, after a change of the monitor bot, its
token or the `运维` members, and once a month.

```sh
S=~/.config/team-chat/production/monitor-admin-secret
curl -s -X POST -H @<(printf 'authorization: Bearer %s' "$(cat $S)") \
  -H 'content-type: application/json' -d '{"sink":"staging"}' \
  https://chat-monitor.huangdeshiou.workers.dev/test-alert
```

- `sink` is a target name from `TARGETS` (`staging` or `production`); leave the body out to use
  the first healthy target, as for a real alert. Use `staging` unless you test production.
- Answers: `200 {"ok":true,"sink":...}`; `401` wrong secret; `404` the Worker secret
  `MONITOR_ADMIN_SECRET` is not set (the route is off); `429` one test per minute; `503 no_sink`
  the chosen env failed its check (the reason is in `message`); `502 post_failed`.
- Check the card in `运维` of that env, and that the people on call got a mention notification.
- Last run: 2026-11-04, `{"sink":"staging"}` from the deployed Worker answered `200`.
- First time: `(umask 077; openssl rand -base64 33 | tr -d '\n=' | tr '+/' '-_' > $S)`, then
  `cd ops/monitor && bunx wrangler secret put MONITOR_ADMIN_SECRET < $S`. Rotate the same way.
- First run: 2026-11-02 23:34 UTC, to staging, card posted in `运维` with one
  mention.

**Daily summary** (01:00 UTC, 09:00 Beijing time): per env the status, number of checks and
failures in 24 hours, availability and round-trip p50/p95. A missing summary means the monitor
itself is down: check `chat-monitor` in the dashboard (Workers, Logs, cron events).

## Incident checklist

1. **Acknowledge** in the `运维` group: who handles it, start time.
2. **Scope**: which env; health or round trip; one group or all (try another group with
   the `tools/e2e` clients); since which deploy (`wrangler deployments list`).
3. **Stop the damage**: a bad deploy → roll back first, debug later. A leaked secret → rotate it
   now (see Rotate). Data being destroyed → stop writers (revoke the credential that writes).
4. **Restore data** only after the code is good: D1 Time Travel, room point-in-time recovery, or
   the R2 archive, in that order of preference (newest data first). Save the revocations of
   step 3 before any D1 restore and apply them again right after it (see D1 Time Travel
   restore): a D1 restore to a time before the revoke brings the revoked credential back.
5. **Verify**: the monitor's 已恢复 card; the owner and one intern see the latest messages; a
   bot replies.
6. **Tell the team** in the `运维` group what happened, what was lost (time range), and whether
   members must reset their cache.
7. **Follow up** within two days: a short write-up in the `运维` group (cause, fix, what to change),
   and a task for each change.
