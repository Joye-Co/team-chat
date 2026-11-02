# Repository rules

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (layout, components, milestones) and
[docs/protocol.md](docs/protocol.md) (wire contract) before you change code.

- **Product name**: only in `product.json`. The server imports it, `apps/desktop/build.rs`
  exports it as compile-time constants, scripts read it with bun or jq. Never type the product
  name in code or UI strings. Infra ids (Worker, D1, crate and package names) use neutral names
  and do not change when the product is renamed.
- **Protocol changes**: update `docs/protocol.md`, `packages/protocol`, `crates/chat-protocol`
  and `protocol/fixtures` in one commit. Both fixture tests must pass.
- **Server**: TypeScript on Cloudflare Workers. Validate every client frame and HTTP body with
  the Zod schemas at the boundary. Durable Objects use location hint `oc`. Each Durable Object
  keeps a `schema_version` in its own SQLite and migrates itself on first access; new code must
  read old data. D1 schema changes go in `server/migrations`.
- **Secrets**: never print, log or commit tokens, invite codes or secrets. Use
  `wrangler secret put` for Worker secrets and the macOS Keychain on the client.
- **Rust builds**: share one target dir so every worktree reuses the GPUI build:
  `export CARGO_TARGET_DIR=$HOME/.cache/team-chat-target`. Do not start a second heavy cargo
  build while one is running.
- **Tooling**: bun for JS packages (`bun install`, `bun run`), cargo for Rust, and wrangler from
  the server package (`bunx wrangler` inside `server/`).
- **Tests**: server with vitest and `@cloudflare/vitest-pool-workers` (`bun run test` in
  `server/`); Rust with `cargo test`; end-to-end with `tools/e2e` against `wrangler dev` or
  staging. A change is done when its tests pass and you ran them.
- **Commits**: a one-line conventional commit subject, no body. Work on your own branch; push to
  `main` only when your task says so.
