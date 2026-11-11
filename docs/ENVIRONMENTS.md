# Environments

How to set up and deploy your own server environments, and where their config and secrets live.
The procedures for incidents, rollback, rotation and restore are in [RUNBOOK.md](RUNBOOK.md).

This repository holds no deployment. Every operator keeps their own server URLs, Worker config
and secrets outside it (see [Ops checkout](#ops-checkout)). The names below (`chat-api-<env>`,
`chat-<env>`, `chat-files-<env>`) are the defaults the docs use; pick your own if you like.

| Env | URL | Worker | D1 | R2 bucket | Use |
| --- | --- | --- | --- | --- | --- |
| local | `http://127.0.0.1:<port>` | `wrangler dev` | local | local | development |
| sandbox | `http://127.0.0.1:<port>` (`.tmp/sandbox/port`) | `wrangler dev` per worktree | local, in `.tmp/sandbox/state` | local | `bun run dev:desktop`: the real app on the sample team of `scripts/sandbox/seed.ts`; secrets in `.tmp/sandbox/` (mode 600) |
| e2e | `https://chat-api-e2e.<your-subdomain>.workers.dev` | `chat-api-e2e` | `chat-e2e` | `chat-files-e2e` | `tools/e2e` runs; data is disposable |
| staging | `https://chat-api-staging.<your-subdomain>.workers.dev` | `chat-api-staging` | `chat-staging` | `chat-files-staging` | team trial before production |
| production | `https://chat-api-prod.<your-subdomain>.workers.dev` | `chat-api-prod` | `chat-prod` | `chat-files-prod` | daily use |

- Put the D1 databases in location `oc`, next to the Durable Objects (location hint `oc`).
- The monitor Worker `chat-monitor` (`ops/monitor`) watches production and staging; see
  [RUNBOOK.md](RUNBOOK.md#monitor-alerts).
- Turn Workers invocation logs off on every deployed `chat-api-*` Worker
  (`"observability": { "logs": { "invocation_logs": false } }`), because they store the full
  request URL. App logs (JSON lines with the path only) stay on.
- Health: `curl <base>/v1/health` returns `{"ok":true,"version":"<deployed version>"}`.
- Never deploy production without the RUNBOOK steps.

## Ops checkout

An operator's deployment details live in a private folder or repository, the ops checkout. The
scripts find it through `TC_OPS_DIR`, else `~/.config/team-chat/ops` (a folder, or a symlink to a
private git checkout: `ln -s <checkout> ~/.config/team-chat/ops`). Never commit its content to
this repository.

| Path | What it is |
| --- | --- |
| `deploy/servers.json` | `{ "e2e": "<url>", "staging": "<url>", "production": "<url>" }`: the base URLs of the deployed envs |
| `deploy/server.wrangler.jsonc` | the full API Worker config: the top level as `server/wrangler.jsonc`, plus one `env.<name>` block per deployed env (Worker name, D1 id, R2 bucket, `EMAIL_FROM`, `VERSION`) |
| `deploy/monitor.wrangler.jsonc` | the full monitor Worker config (`TARGETS` with your URLs and group names, service bindings) |
| `roster/roster.json` | optional: your team's bots (name, character, 岗位, level) for your own scripts and notes |
| `docs/` | optional: your own runbook notes, deploy state and to-dos |

Who reads what:

- **The desktop app** (at build time): `deploy/servers.json`. Its staging and production URLs
  become the 正式服务器 and 测试服务器 items of the server menu. With no file, the items are
  hidden and a person types the server URL at sign-in.
- **Scripts**: `scripts/lib.sh` (`env_base`), `scripts/servers.ts` (`deployedServer()`) and
  through them `server/scripts/admin.ts`, `ops/restore`, `scripts/cleanup-test-data.ts`,
  `scripts/staging-smoke.ts` and `scripts/release.sh` take the URL of an env from
  `deploy/servers.json`. `CHAT_API_BASE`, `--url` or `--base` override it.
- **Deploys**: `server/wrangler.jsonc` is for local development only (`wrangler dev`, tests).
  `scripts/deploy-server.sh <env>` copies `deploy/server.wrangler.jsonc` to
  `server/wrangler.deploy.jsonc` (git ignores it) and runs wrangler with
  `-c wrangler.deploy.jsonc --env <env>`. Any manual wrangler command on a deployed env does the
  same, inside `server/`: `bunx wrangler -c wrangler.deploy.jsonc <command> --env <env>`. The
  monitor works the same way with `deploy/monitor.wrangler.jsonc` in `ops/monitor/`.

**Releases** embed the server URLs only; the character art is in the repository
(`apps/desktop/assets/characters/`). The roster is for local development and is never part of
a release: `scripts/release.sh` builds against a folder that
holds just `deploy/servers.json`, taken from `TC_SERVERS_JSON` when set, else from the ops
checkout. The Release workflow sets `TC_SERVERS_JSON` from the repository variable
`SERVERS_JSON`:

```sh
gh variable set SERVERS_JSON --body '{"staging":"<url>","production":"<url>"}'
```

The workflows never print it: the jobs that use it are called workflows (`release-jobs.yml`,
`windows-installer.yml`) that get it as a secret, each job masks every URL and host first, and
a last job (`scripts/check-logs-for-servers.sh`) fails the run when a job log shows a host. Do
not read `vars.SERVERS_JSON` through `env:` in a workflow: Actions prints the environment of
every step, and the logs of this repository are public.

The commands in these docs assume the copy exists. Make it by hand when you run wrangler
without the deploy script:

```sh
cp "${TC_OPS_DIR:-$HOME/.config/team-chat/ops}/deploy/server.wrangler.jsonc" server/wrangler.deploy.jsonc
cp "${TC_OPS_DIR:-$HOME/.config/team-chat/ops}/deploy/monitor.wrangler.jsonc" ops/monitor/wrangler.deploy.jsonc
```

## Secret files

Each env has a folder `~/.config/team-chat/<env>/` (mode 700). Every file in it has mode 600.
Never print these files; pipe them into commands.

| File | What it is |
| --- | --- |
| `admin-secret` | Worker secret `ADMIN_SECRET` (protects `POST /v1/admin/bootstrap` and the admin routes) |
| `hook-secret-key` | Worker secret `HOOK_SECRET_KEY` (encrypts integration hook secrets and the AI 服务 key in D1; if you lose it, every hook must be made again and an owner must enter the AI 服务 key again) |
| `owner-token` | token of the first owner (from `admin.ts bootstrap`); `admin.ts` and `tools/e2e` read this file |
| `owner.token` | optional: a copy of `owner-token` for tools that expect that name |
| `monitor-token` | staging and production: token of the monitor bot (Worker secret `MONITOR_TOKEN_<ENV>` of `chat-monitor`) |
| `monitor-admin-secret` | production folder only: Worker secret `MONITOR_ADMIN_SECRET` of `chat-monitor`, the bearer of its `POST /test-alert` ([RUNBOOK.md](RUNBOOK.md#test-the-alert-path)) |
| `bots/<name>.token` | token of each bot you run ([bots.md](bots.md)) |
| `github-hook.json` | `{ hookId, groupId, url, secret, repo }` of a GitHub hook ([integrations.md](integrations.md)); `scripts/github-webhook.sh` reads it |
| `invite-*.txt`, `bot-*.token`, `machine-*.token` | written by `admin.ts` |

## First setup of an env

```sh
cd server
bunx wrangler d1 create chat-<env> --location oc
# put the id into env.<env> of deploy/server.wrangler.jsonc in your ops checkout,
# and the env's base URL into deploy/servers.json
cd .. && scripts/deploy-server.sh <env> && cd server   # D1 migrations, deploy, writes wrangler.deploy.jsonc
d=~/.config/team-chat/<env>; mkdir -p -m 700 $d
for f in admin-secret hook-secret-key; do
  (umask 077; openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n' > $d/$f)
done
bunx wrangler -c wrangler.deploy.jsonc secret put ADMIN_SECRET --env <env> < $d/admin-secret
bunx wrangler -c wrangler.deploy.jsonc secret put HOOK_SECRET_KEY --env <env> < $d/hook-secret-key
cd .. && bun server/scripts/admin.ts bootstrap --env <env> --name <owner name>
```

Bootstrap works only while the env has no member. It writes the owner token to
`~/.config/team-chat/<env>/owner-token`.

Leave no test data in production: no test groups, topics, members or bots. Check a production
deploy with `/v1/health`, the read-only WebSocket check below and the monitor, never with
`tools/e2e`.

## Deploy

From a commit on `origin/main`:

```sh
cd server
bun run deploy:e2e        # D1 migrations, then wrangler deploy --env e2e
bun run deploy:staging    # the same for staging
```

The `deploy:*` scripts run `scripts/deploy-server.sh <env>`. It copies the Worker config from
the ops checkout (above) and passes `VERSION` with `wrangler deploy --var`: the release version
when HEAD has a release tag `v<semver>`, otherwise the 12-character commit (`-dirty` when
`server/`, `packages/` or `protocol/` have uncommitted changes). `DEPLOY_VERSION=<v>` overrides
it, `--print-version` shows it, `--dry-run` builds without uploading. The `VERSION` values in
`server/wrangler.jsonc` are only for `wrangler dev`.

It refuses a `0.0.0*` version and, after the deploy, fails unless `/v1/health` reports the
deployed version (`--check-health [--expect <v>]` runs only that check). Then run `tools/e2e`
against the env (below).

What is deployed and which migrations ran:

```sh
cd server
bunx wrangler -c wrangler.deploy.jsonc deployments list --env <env> | tail -n 4
bunx wrangler -c wrangler.deploy.jsonc d1 migrations list DB --env <env> --remote
```

Read-only WebSocket check (hello, welcome, a `fetch` of every group, ping; no writes):

```sh
bun scripts/ws-check.ts <base> ~/.config/team-chat/<env>/owner-token
# {"ok":true,"welcomeMs":...,"groups":[...],"pong":true,...}; exit 0 on success
```

## admin.ts

`bun server/scripts/admin.ts <command> --env <env>` from the repository root. The base URL comes
from `--url`, else from `deploy/servers.json` in the ops checkout. It prints only ids and file
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
cargo run -p e2e -- --base-url <e2e base> --secrets-dir ~/.config/team-chat/e2e
```

All scenarios except `soak` run by default (`--minutes <n>` adds soak). `pipeline` sends 50
writes on one socket without waiting for acks; it is the check for the subrequest depth limit
(see Logs), so run it on a deployed env after any change to how Durable Objects call each other.
Never run `tools/e2e` against production: it leaves its groups and members behind.

Use the e2e env for routine runs. Each run leaves its groups and members in place (only their
tokens are revoked), so run against staging only when a release needs it.

## Bots and the desktop app against a deployed env

```sh
# Runner-backed bot: the real daemon with the fake Claude Code CLI. Makes its own group,
# member, bot runner and machine, then removes the runner and the machine.
CARGO_TARGET_DIR=<your target dir> bun apps/daemon/e2e/local-e2e.ts \
  --base-url <e2e base> \
  --owner-token-file ~/.config/team-chat/e2e/owner-token

# Bot SDK echo example (make the bot with admin.ts bot, add it to a group, then mention it)
CHAT_URL=<e2e base> CHAT_TOKEN_FILE=<bot token file> bun examples/echo-bot.ts

# Desktop app from the DMG against staging, signed in with a test bot token (never the owner's)
Cahoots.app/Contents/MacOS/Cahoots --selftest-live <staging base> \
  --token-file ~/.config/team-chat/staging/<test bot>.token --report <json>
```

Keep a test bot and a test group in staging for this check only.

## Logs

```sh
cd server
bunx wrangler -c wrangler.deploy.jsonc tail --env e2e                          # live, pretty
bunx wrangler -c wrangler.deploy.jsonc tail --env staging --format json > ../.tmp/tail.log
bunx wrangler -c wrangler.deploy.jsonc tail --env staging --status error       # failed requests only
bunx wrangler -c wrangler.deploy.jsonc deployments list --env staging          # what is deployed
```

- Turn Workers Logs (observability) on for every env; older logs are in the dashboard under
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
