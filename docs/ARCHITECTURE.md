# Architecture

The design doc (product form and tech stack) is a private Claude Doc:
https://claude.ai/artifact/YSW3U93QALC7srXYMmAGZ3. This file is the short version that code
work needs. The wire contract is [protocol.md](protocol.md).

## Layout

| Path | What it holds |
| --- | --- |
| `product.json` | Product name, slug, bundle id, URL scheme, server URLs. The only place the product name lives. |
| `docs/protocol.md` | Wire protocol contract |
| `packages/protocol` | Zod schemas: the executable source of truth for the protocol |
| `protocol/fixtures` | Golden JSON frames that both implementations must parse and re-serialize |
| `server` | Cloudflare Worker in TypeScript: API Worker, Durable Objects, D1 migrations |
| `crates/chat-protocol` | Rust serde types for the protocol |
| `crates/chat-core` | Rust client core without UI: local SQLite cache, sync engine, WebSocket connection |
| `apps/desktop` | Rust GPUI macOS app |
| `apps/daemon` | Rust machine daemon that runs coding CLIs for bot members |
| `tools/e2e` | Headless clients for end-to-end and load tests (Rust, built on chat-core) |
| `crates/artifact-view` | WKWebView window for HTML artifacts (objc2) |
| `packages/search` | CJK bigram tokenizing for SQLite FTS5 |
| `packages/bot-sdk` | TypeScript SDK for external bots |
| `ops/monitor` | Monitor Worker |
| `scripts` | `build-dmg.sh`, release and admin helpers |
| `docs/PRODUCT.md` | Product brief: interface words, screens, V1 bar |

## Server (Cloudflare)

- **API Worker**: HTTP API, auth, file upload and download, webhooks, and routing of WebSocket
  upgrades to the member's gateway. A scheduled handler runs the daily archive.
- **MemberGateway** Durable Object, one per member: holds that member's WebSockets (several
  devices), forwards frames to group rooms, pushes events and directory updates, stores read
  cursors.
- **GroupRoom** Durable Object, one per group: the group's event log in its own SQLite (seq,
  dedupe, topics, messages), fan-out to member gateways, unread counts, typing and presence in
  memory only, delta coalescing for streaming bot replies. Feature modules (search, files,
  bots, notifications) plug in through hooks.
- **BotDesk** Durable Object, one per bot member: the front desk. It never executes commands.
- **MachineLink** Durable Object, one per machine: the daemon connection and task leases.
- **D1**: members, tokens, invites, groups, group membership, files and artifacts index, hooks,
  machines, audit log.
- **R2**: attachments, HTML artifact versions, raw sessions, releases (DMGs), diagnostics, and
  the daily archive of every group log and of D1.
- **Monitor Worker** (`ops/monitor`): cron health checks and a real message round trip; alerts
  go to an ops group.
- Every Durable Object is created with location hint `oc`. Queues are available but V1 uses
  `ctx.waitUntil` and Durable Object alarms instead.

Server code layout: `server/src/core/` (router, auth, gateway, rooms), and one folder per
feature: `files/`, `search/`, `bots/`, `integrations/`, `ops/`, `notify/`.

## Environments

| Env | Worker | D1 | R2 bucket | URL |
| --- | --- | --- | --- | --- |
| local | `wrangler dev` | local | local | `http://127.0.0.1:<port>` |
| e2e | `chat-api-e2e` | `chat-e2e` | `chat-files-e2e` | `https://chat-api-e2e.huangdeshiou.workers.dev` |
| staging | `chat-api-staging` | `chat-staging` | `chat-files-staging` | `https://chat-api-staging.huangdeshiou.workers.dev` |
| production | `chat-api-prod` | `chat-prod` | `chat-files-prod` | `https://chat-api-prod.huangdeshiou.workers.dev` |

The three R2 buckets exist (made through the Cloudflare connector; the wrangler login has no R2
scope, so do not run `wrangler r2` commands, but Worker R2 bindings work). Secret files live in
`~/.config/team-chat/<env>/` (mode 600). Only the agents whose task says so deploy.

## Client (macOS)

- GPUI plus gpui-component, built with GPUI's `runtime_shaders` feature, so a full Xcode is not
  needed to build.
- `third_party/gpui-pre` is gpui-pre 0.3.10 with two text fixes, used through `[patch.crates-io]`:
  CJK line breaking (no line starts with closing or ends with opening punctuation; paths and
  URLs break after `/`, never before it, in `line_wrapper.rs` and `line_layout.rs`) and an
  ellipsis that fits the shaped width and trims punctuation and short word cuts before `…`.
- chat-core runs networking on a background tokio runtime. The UI renders from the local SQLite
  cache and never waits for the network.

## Milestones

### V0: technical validation

Done when all five pass:

1. Two Macs log in with invite codes, see the same groups, start topics and chat in real time.
2. Cut the network on one Mac and send 50 messages from the other: after the reconnect, the
   first Mac shows all 50 in order with no duplicates.
3. A topic with 10,000 messages opens in under 100 ms from the cache and scrolls without
   visible stutter; Chinese IME input works in the composer.
4. The server runs on staging, and `tools/e2e` passes against staging.
5. `scripts/build-dmg.sh` makes a DMG that installs and runs on another Mac.

### V1: production use by 5 people and 5 bots

See the V1 bar in [PRODUCT.md](PRODUCT.md#v1-production-use-by-5-people-and-5-bots).
