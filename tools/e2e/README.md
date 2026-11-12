# tools/e2e

Headless end-to-end runner built on `chat-core`. It makes its own members through the HTTP API,
runs scenarios against a server, and writes JSON reports to `tools/e2e/reports/` (gitignored):
one `<run>-<scenario>.json` per scenario and a `<run>-summary.json` file. The exit code is 0 when
every scenario passes and 1 when a scenario fails.

## Run against local wrangler dev

```sh
. "$HOME/.cargo/env"; export RUSTC_WRAPPER=sccache CARGO_BUILD_JOBS=6
# one-time: an admin secret for this local stack (never printed)
mkdir -p -m 700 ~/.config/team-chat/local-e2e
(umask 077; openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n' > ~/.config/team-chat/local-e2e/admin-secret)
(umask 077; printf 'ADMIN_SECRET=%s\n' "$(cat ~/.config/team-chat/local-e2e/admin-secret)" > server/.dev.vars)
mkdir -p .tmp
(cd server && bunx wrangler d1 migrations apply DB --local)
(cd server && "$PWD/node_modules/.bin/wrangler" dev --port 8790 --inspector-port 9790 --ip 127.0.0.1 \
  > ../.tmp/wrangler.log 2>&1 & echo $! > ../.tmp/wrangler.pid)

cargo run -p e2e -- --base-url http://127.0.0.1:8790 --secrets-dir ~/.config/team-chat/local-e2e
cargo run -p e2e -- --scenario chat,gap
cargo run -p e2e -- --minutes 10             # all scenarios plus a 10-minute soak
cargo run -p e2e -- --list

# stop wrangler: check that `ps -p "$(cat .tmp/wrangler.pid)" -o command=` shows this worktree
kill "$(cat .tmp/wrangler.pid)"
```

Client caches go to `.tmp/e2e-<pid>/` and are removed at the end of the run.

On the first run against an empty server the runner bootstraps an owner with `admin-secret` and
saves the token to `<secrets-dir>/owner-token` (mode 600). Later runs use `owner-token`. Against a
deployed environment, pass `--base-url` and a secrets dir that has `owner-token`.

Each run makes new members named `e2e-<run>-<label>`. They leave the default groups at once, and
their tokens are revoked at the end of the run.

## Scenarios

| Name | Checks |
| --- | --- |
| `chat` | A starts a topic; B gets `topic.created` then `message.created` (raw wire order); B replies; A gets it; A renames the group, B gets `group.updated` and its cache follows; the same name again adds no event |
| `catchup` | B is offline; A sends 50; B restarts with its cache and has all 50 in order, no duplicates |
| `idempotent` | The same `clientMsgId` twice (also after a reconnect) gives one message and `duplicate: true` |
| `gap` | B's test hook drops one live event; the next event shows the gap and B fetches it |
| `streaming` | A bot streams with deltas and a final edit; humans see growing text and the final body; one `message.updated` stored |
| `permissions` | A non-member can neither read nor write (WS frames and HTTP); bad and revoked tokens get 401 |
| `reactions` | A and B add and remove reactions; caches follow live, a repeated add stores nothing, a fresh client rebuilds the list from the log |
| `plan` | A bot starts a run with a plan; a member's cache gets it; an update replaces the whole plan, `[]` clears it; a member gets 403 and a 201-character item 400; after the end a late member catches up to the final plan |
| `runs-v11` | 1.1 runs on a raw machine socket with features `pause`, `answer` and `aside`: a mention gets a 👀 reaction (no text ack); the requester pauses (409 on a second pause) and resumes with a note, and the machine gets `run.pause` and `run.resume`; the bot posts a Card v2 ask, the requester's answer reaches the run as `run.input.answer` and a second answer is refused; `run.done` posts one done card with test proof and stores `resultMessageId`, `usage` and `durationMs`; `GET /v1/groups/:id/decisions` lists the answer; a side chat runs as `kind: aside` in the asker's DM and posts nothing in the source group |
| `acp-resume` | An AI member on an ACP agent, raw machine socket with feature `resume` and per-CLI `CliInfo.resume`: a follow-up in the topic of an agent that loads sessions gets `resumeSessionId` and only what is new; with an agent that cannot, no session and the whole topic |
| `relay` | A relay (按顺序做) on a raw machine socket: `@dev 写 → @qa 审 → @dev 改` in a topic starts only dev; dev's end wakes qa with dev's result and code location; qa fails, the relay pauses (`step_failed`) and the topic is blocked; a retry runs qa's step again; dev's last step is stopped with `POST …/relays/:id/stop`, which cancels its run |
| `load` | 5 humans + 5 bots in 3 groups send 100 messages each at once; all caches equal the server log; p50/p95 latency |
| `soak` | `--minutes N` of low-rate chat with network blips and restarts (sends queued while stopped); consistency at the end |

`soak` runs with `--scenario all` (the default) only when `--minutes` is given.

## Add a scenario

Add `src/scenarios/<name>.rs` with a `pub const SCENARIO: Scenario` and add it to `registry()`
in `src/scenarios/mod.rs`. Use `Harness` (`human`, `bot`, `group`, `client`, `api`) for setup,
`TestClient::wait` to wait on the cache, `RawWs` to read the server log or send raw frames, and
`soak::consistent` to compare caches with the server log. Put numbers in `ctx.metric` and facts
in `ctx.note`. Never print tokens or invite codes.
