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
| `apps/daemon` | Rust machine daemon that runs coding CLIs for bot members (V1) |
| `tools/e2e` | Headless clients for end-to-end and load tests (Rust, built on chat-core) |
| `scripts` | `build-dmg.sh` and admin helpers |

## Server (Cloudflare)

- **API Worker**: HTTP API, auth, and routing of WebSocket upgrades to the member's gateway.
- **MemberGateway** Durable Object, one per member: holds that member's WebSockets (several
  devices), forwards frames to group rooms, pushes events and directory updates, stores read
  cursors.
- **GroupRoom** Durable Object, one per group: the group's event log in its own SQLite (seq,
  dedupe, topics, messages), fan-out to member gateways, unread counts, typing and presence in
  memory only, delta coalescing for streaming bot replies.
- **BotDesk** Durable Object, one per bot member (V1): the front desk loop.
- **MachineLink** Durable Object, one per machine (V1): the daemon connection and task leases.
- **D1**: members, tokens, invites, groups, group membership, audit log.
- **R2** (V1): attachments, HTML artifacts, raw sessions, daily archives of every group log.
- Every Durable Object is created with location hint `oc`.

## Client (macOS)

- GPUI plus gpui-component, built with GPUI's `runtime_shaders` feature, so a full Xcode is not
  needed to build.
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

### V1: daily use by 5 people and 5 bots

Topics with status, DMs, @mentions, unread counts and notifications, edit and delete,
attachments, Chinese search, presence and typing, bot members on bot tokens (same WebSocket
protocol), the machine daemon running coding CLIs as bot members, session capture, HTML
artifacts in a WKWebView window, a DMG on GitHub Releases, and a load test with 10 concurrent
members that passes.
