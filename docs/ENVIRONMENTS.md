# Environments

Deployed server environments, how to deploy them, and where their secrets are. The procedures
for incidents, rollback, rotation and restore are in [RUNBOOK.md](RUNBOOK.md).

| Env | URL | Worker | D1 (id) | R2 bucket | Use |
| --- | --- | --- | --- | --- | --- |
| local | `http://127.0.0.1:<port>` | `wrangler dev` | local | local | development |
| e2e | `https://chat-api-e2e.huangdeshiou.workers.dev` | `chat-api-e2e` | `chat-e2e` (`67fb03a3-eabb-40d9-b446-808f301c21f3`) | `chat-files-e2e` | `tools/e2e` runs; data is disposable |
| staging | `https://chat-api-staging.huangdeshiou.workers.dev` | `chat-api-staging` | `chat-staging` (`95271ff5-2e69-4ce7-ba26-bbbdfe29f760`) | `chat-files-staging` | team trial before production |
| production | `https://chat-api-prod.huangdeshiou.workers.dev` | `chat-api-prod` | `chat-prod` (`6785a4f4-5089-4410-8d30-d38d5cadc413`) | `chat-files-prod` | daily use |

- The D1 databases are in location `oc`, next to the Durable Objects (location hint `oc`).
- The D1 ids are in `server/wrangler.jsonc` under `env.<env>`. The staging and production URLs
  are in `product.json` `servers`; the desktop app (its default server is production),
  `scripts/release.sh`, `ops/restore` and `server/scripts/admin.ts` read them there.
- The monitor Worker `chat-monitor` (`ops/monitor`) watches production and staging; see
  [RUNBOOK.md](RUNBOOK.md#monitor-alerts).
- Workers invocation logs are off on every `chat-api-*` Worker (`invocation_logs: false`;
  checked on 2026-11-03 with the Cloudflare API `script-settings`), because they store the full
  request URL. App logs (JSON lines with the path only) stay on.
- Health: `curl https://chat-api-<env>.huangdeshiou.workers.dev/v1/health` returns
  `{"ok":true,"version":"0.0.0-<env>"}`.
- Only the agents or people whose task says so deploy. Never deploy production without the
  RUNBOOK steps.

## Secret files

Each env has a folder `~/.config/team-chat/<env>/` (mode 700). Every file in it has mode 600.
Never print these files; pipe them into commands.

| File | What it is |
| --- | --- |
| `admin-secret` | Worker secret `ADMIN_SECRET` (protects `POST /v1/admin/bootstrap` and the admin routes) |
| `hook-secret-key` | Worker secret `HOOK_SECRET_KEY` (encrypts integration hook secrets in D1; if you lose it, every hook must be made again) |
| `anthropic-api-key` | Worker secret `ANTHROPIC_API_KEY` (optional: topics without a title get an AI title from Claude; without it they keep the first line). Local: `ANTHROPIC_API_KEY=...` in `server/.dev.vars` |
| `owner-token` | token of the first owner (`Joye` on staging and production, `e2e-owner` on e2e); `admin.ts` and `tools/e2e` read this file |
| `owner.token` | staging and production: a copy of `owner-token` |
| `monitor-token` | staging and production: token of the monitor bot `监测` (Worker secret `MONITOR_TOKEN_<ENV>` of `chat-monitor`) |
| `monitor-admin-secret` | production folder only: Worker secret `MONITOR_ADMIN_SECRET` of `chat-monitor`, the bearer of its `POST /test-alert` ([RUNBOOK.md](RUNBOOK.md#test-the-alert-path)) |
| `bots/<name>.token` | staging and production: token of each roster bot ([ROSTER.md](ROSTER.md)) |
| `joye-signin-code` | production: Joye's sign-in code (8 characters, single use, expires 7 days after it was made) |
| `invites/实习生<n>.code`, `invites/实习生<n>.link` | production: the four intern invite codes and their download links (`/download#code=...`) |
| `github-hook.json` | production: `{ hookId, groupId, url, secret, repo }` of the GitHub hook of the 开发 group; `scripts/github-webhook.sh` reads it |
| `invite-*.txt`, `bot-*.token`, `machine-*.token` | written by `admin.ts` |

## First setup of an env (done for e2e, staging and production)

```sh
cd server
bunx wrangler d1 create chat-<env> --location oc        # put the id into wrangler.jsonc env.<env>
bunx wrangler d1 migrations apply DB --env <env> --remote
bunx wrangler deploy --env <env>
d=~/.config/team-chat/<env>; mkdir -p -m 700 $d
for f in admin-secret hook-secret-key; do
  (umask 077; openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n' > $d/$f)
done
bunx wrangler secret put ADMIN_SECRET --env <env> < $d/admin-secret
bunx wrangler secret put HOOK_SECRET_KEY --env <env> < $d/hook-secret-key
cd .. && bun server/scripts/admin.ts bootstrap --env <env> --name <owner name>   # e2e: add --url
```

Bootstrap works only while the env has no member. It writes the owner token to
`~/.config/team-chat/<env>/owner-token`.

## Deploy

From a commit on `origin/main`:

```sh
cd server
bun run deploy:e2e        # D1 migrations, then wrangler deploy --env e2e
bun run deploy:staging    # the same for staging
```

The `deploy:*` scripts run `scripts/deploy-server.sh <env>`, which passes `VERSION` with
`wrangler deploy --var`: the release version when HEAD has a release tag `v<semver>`, otherwise
the 12-character commit (`-dirty` when `server/`, `packages/` or `protocol/` have uncommitted
changes). `DEPLOY_VERSION=<v>` overrides it, `--print-version` shows it, `--dry-run` builds
without uploading. The `VERSION` values in `server/wrangler.jsonc` are only for `wrangler dev`.

It refuses a `0.0.0*` version and, after the deploy, fails unless `/v1/health` reports the
deployed version (`--check-health [--expect <v>]` runs only that check). Then run `tools/e2e`
against the env (below).

State on 2026-11-03 (from `wrangler deployments list` and `wrangler d1 migrations list`):

| Env | Deployed (UTC) | D1 migrations applied | Pending |
| --- | --- | --- | --- |
| e2e | 2026-11-02 23:03, version `0566745e` (main `6ec0691`) | through `0006_ops` | none |
| staging | 2026-11-02 23:03, version `7b88d141` (main `6ec0691`) | through `0006_ops` | none |
| production | 2026-11-02 23:10, version `966b82ac` (main `fb10286`, server code as `6ec0691`) | through `0006_ops` | none |
| monitor | 2026-11-02 23:33, `chat-monitor` version `7fef8b5d` (main `2830ddd`, adds `POST /test-alert`), then the secret `MONITOR_ADMIN_SECRET` | (no D1) | |

First monitor checks (2026-11-02 23:25 UTC): production ok (health 230 ms, round trip 275 ms),
staging ok (health 113 ms, round trip 289 ms), archive fresh on both. First archive pass by hand
on 2026-11-02 23:13 UTC: production 4 groups, staging 6 groups, complete, no failure.

Deploys carry no commit message, so wrangler does not record the commit; the commit in the table
is the one the deploy was made from. No new Worker secret since the first setup. Update this table after each deploy:

```sh
cd server
bunx wrangler deployments list --env <env> | tail -n 4
bunx wrangler d1 migrations list DB --env <env> --remote
```

## Production setup (2026-11-03)

What exists in production after the first setup. Leave no test data in production: no test
groups, topics, members or bots. Check a deploy with `/v1/health`, the read-only WebSocket check
below and the monitor, never with `tools/e2e`.

| Kind | What |
| --- | --- |
| Owner | `Joye` (human, owner), `01M6F8FFH2K1FDRF123W1120YH`; signs in with `joye-signin-code` |
| Bots | the eight bots of [ROSTER.md](ROSTER.md) with their character, 岗位 and level (Jojo is owner); `监测` (monitor); `GitHub Joye-Co/team-chat` (the sender of the GitHub hook, made by `POST /v1/hooks`) |
| Groups | `全员` (`01M6F8FFH2K1FDRF123W1120YK`, the default group: every member; renamed from `General` with `PATCH /v1/groups/:id` on 2026-11-03), `开发` (Joye, Forge, Watch, Scout, Ledger, Tata, the GitHub hook sender), `运维` (Joye, Watch, 监测), `监测` (only 监测: the monitor round trip) |
| Runner | Jojo: labels `mac-mini`, runtime placeholder `claude`, **disabled** until Joye installs the daemon and runs `admin.ts runner` (see [USER-ACTIONS.md](USER-ACTIONS.md)) |
| Invites | 实习生1 to 实习生4 (human, member), made 2026-11-02 23:12 UTC, expire 2026-11-09 23:12 UTC |
| Hook | GitHub hook `01M6F8JV5Z23M3E2AF40J2564Z` into `开发`; the GitHub webhook is not added yet (`scripts/github-webhook.sh`) |
| Messages | one test alert from 监测 in `运维`, topic 监测告警测试（无需处理）; the monitor's round trips in `监测` |

Staging has the same roster, `开发`, `运维` and `监测` (its default group `全员`, renamed from `General`, also holds the
test bots `selftest-integrate` and `selftest-final` and the groups 自测 and 自测 final).

Read-only WebSocket check (hello, welcome, a `fetch` of every group, ping; no writes):

```sh
bun scripts/ws-check.ts https://chat-api-prod.huangdeshiou.workers.dev ~/.config/team-chat/production/owner-token
# {"ok":true,"welcomeMs":...,"groups":[...],"pong":true,...}; exit 0 on success
```

## admin.ts

`bun server/scripts/admin.ts <command> --env <env>` from the repository root. The base URL comes
from `--url`, else from `product.json` `servers.<env>`. For e2e there is no `product.json` entry,
so pass `--url https://chat-api-e2e.huangdeshiou.workers.dev`. It prints only ids and file
paths. It reads the owner token from `owner-token` and the admin secret from `admin-secret` of
the env folder.

```sh
bun server/scripts/admin.ts members --env staging
bun server/scripts/admin.ts invite --env staging --name 小王 --role dev       # code -> invite-*.txt
bun server/scripts/admin.ts bot --env staging --name 助手 --role dev          # token -> bot-*.token
bun server/scripts/admin.ts revoke --env staging --member <id>
bun server/scripts/admin.ts help                                             # all commands
```

## tools/e2e against a deployed env

```sh
. "$HOME/.cargo/env"; export RUSTC_WRAPPER=sccache CARGO_BUILD_JOBS=6 CARGO_TARGET_DIR=<your target dir>
cargo run -p e2e -- --base-url https://chat-api-e2e.huangdeshiou.workers.dev --secrets-dir ~/.config/team-chat/e2e
```

All scenarios except `soak` run by default (`--minutes <n>` adds soak). `pipeline` sends 50
writes on one socket without waiting for acks; it is the check for the subrequest depth limit
(see Logs), so run it on a deployed env after any change to how Durable Objects call each other.
Last run on e2e (`sz8xgx`, 2026-11-03): all 10 pass; load delivery p50 63 ms, p95 157 ms. Last soak (`p24gpf`, 30 minutes): pass.
First run on staging (`hj8xdn`, 2026-11-03): all 10 pass; load delivery p50 59 ms, p95 132 ms.
Never run `tools/e2e` against production: it leaves its groups and members behind.

Use the e2e env for routine runs. Each run leaves its groups and members in place (only their
tokens are revoked), so run against staging only when a release needs it.

## Bots and the desktop app against a deployed env

```sh
# Runner-backed bot: the real daemon with the fake Claude Code CLI. Makes its own group,
# member, bot runner and machine, then removes the runner and the machine.
CARGO_TARGET_DIR=<your target dir> bun apps/daemon/e2e/local-e2e.ts \
  --base-url https://chat-api-e2e.huangdeshiou.workers.dev \
  --owner-token-file ~/.config/team-chat/e2e/owner-token

# Bot SDK echo example (make the bot with admin.ts bot, add it to a group, then mention it)
CHAT_URL=https://chat-api-e2e.huangdeshiou.workers.dev CHAT_TOKEN_FILE=<bot token file> \
  bun examples/echo-bot.ts

# Desktop app from the DMG against staging, signed in with a test bot token (never the owner's)
TeamChat.app/Contents/MacOS/TeamChat --selftest-live https://chat-api-staging.huangdeshiou.workers.dev \
  --token-file ~/.config/team-chat/staging/bot-selftest-integrate-<id>.token --report <json>
```

The staging bot `selftest-integrate` and its group 自测 exist for this check only.

## Logs

```sh
cd server
bunx wrangler tail --env e2e                          # live, pretty
bunx wrangler tail --env staging --format json > ../.tmp/tail.log
bunx wrangler tail --env staging --status error       # failed requests only
bunx wrangler deployments list --env staging          # what is deployed
```

- Workers Logs (observability) is on for every env; older logs are in the dashboard under
  Workers > `chat-api-<env>` > Logs.
- `wrangler tail` samples under load and often misses Durable Object WebSocket events. To find
  the cause of an `internal` ack, use the Workers Logs dashboard, or reproduce the frames with
  a small WebSocket script against the e2e env.
- `wrangler dev` does not enforce some production limits, for example the subrequest depth
  limit (16). A Durable Object counts each outgoing call against its newest unfinished incoming
  request, so a gateway that called a room while a room delivery was its newest request nested
  one level deeper per write, and pipelined writes failed after about 8 with `internal`. The
  gateway now starts each write's room call when the frame arrives and the room keeps the
  socket's order (`server/src/core/gateway.ts`, `GroupRoom.ordered`). Run `tools/e2e` (with
  `pipeline`) against the e2e env after a change to how Durable Objects call each other.
