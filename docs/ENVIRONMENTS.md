# Environments

Deployed server environments, how to deploy them, and where their secrets are. The procedures
for incidents, rollback, rotation and restore are in [RUNBOOK.md](RUNBOOK.md).

| Env | URL | Worker | D1 (id) | R2 bucket | Use |
| --- | --- | --- | --- | --- | --- |
| local | `http://127.0.0.1:<port>` | `wrangler dev` | local | local | development |
| e2e | `https://chat-api-e2e.huangdeshiou.workers.dev` | `chat-api-e2e` | `chat-e2e` (`67fb03a3-eabb-40d9-b446-808f301c21f3`) | `chat-files-e2e` | `tools/e2e` runs; data is disposable |
| staging | `https://chat-api-staging.huangdeshiou.workers.dev` | `chat-api-staging` | `chat-staging` (`95271ff5-2e69-4ce7-ba26-bbbdfe29f760`) | `chat-files-staging` | team trial before production |
| production | not deployed yet | `chat-api-prod` | `chat-prod` (not made yet) | `chat-files-prod` | daily use |

- The D1 databases are in location `oc`, next to the Durable Objects (location hint `oc`).
- The D1 ids are in `server/wrangler.jsonc` under `env.<env>`. The staging URL is in
  `product.json` `servers.staging`; the desktop app and `server/scripts/admin.ts` read it there.
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
| `owner-token` | token of the first owner (`Joye` on staging, `e2e-owner` on e2e); `admin.ts` and `tools/e2e` read this file |
| `owner.token` | staging only: a copy of `owner-token` |
| `invite-*.txt`, `bot-*.token`, `machine-*.token` | written by `admin.ts` |

## First setup of an env (done for e2e and staging)

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

Then check `/v1/health` and run `tools/e2e` against the env (below).

State on 2026-11-03: e2e and staging run `origin/main` at `d2edf55` (D1 migrations through
`0004_characters`). No new Worker secret since the first setup.

## admin.ts

`bun server/scripts/admin.ts <command> --env <env>` from the repository root. The base URL comes
from `--url`, else from `product.json` `servers.<env>`. For e2e there is no `product.json` entry,
so pass `--url https://chat-api-e2e.huangdeshiou.workers.dev`. It prints only ids and file
paths.

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
Last run on e2e (`e2wgg0`): all 8 pass; load delivery p50 71 ms, p95 153 ms.

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
