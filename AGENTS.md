# Repository rules

Read [docs/PRODUCT.md](docs/PRODUCT.md) (what we build, interface words, V1 bar),
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (layout, components, environments) and
[docs/protocol.md](docs/protocol.md) (wire contract) before you change code.

- **Product name**: only in `product.json`. The server imports it, `apps/desktop/build.rs`
  exports it as compile-time constants, scripts read it with bun or jq. Never type the product
  name in code or UI strings. Infra ids (Worker, D1, crate and package names) use neutral names
  and do not change when the product is renamed.
- **Interface text**: Simplified Chinese, only from the desktop copy catalog, only the four
  product nouns 成员, 群, 话题, 产出 (see PRODUCT.md). Developer words stay in the developer view
  and in logs. The catalog test enforces this.
- **Design bar**: first-class visual design is a V1 requirement, not polish for later. Build
  every UI element from gpui-component (the first-class GPUI component library: button, input,
  list, popover, menu, dialog, tooltip, badge, avatar, tabs, switch, notification and the rest),
  styled through the theme; never hand-roll a control that gpui-component has. Follow
  [docs/DESIGN.md](docs/DESIGN.md) once it is on main; until then keep every color, size and
  duration in theme tokens so the design pass can restyle them. Look at your screens: capture
  only the app's own window (`screencapture -l <window id> -o -x <png>`, or the app's
  screenshot self-test when it exists), never the full screen, in light and dark mode, and read
  the PNGs. Before merging UI work, check it with the vendored skills in
  `docs/design/skills/emilkowalski/`: `emil-design-eng` (polish), `review-animations` (motion),
  `break-ui` (worst-case data) and `apple-design` (macOS feel). Web pages, if any, use the
  libraries in `pick-ui-library`.
- **Protocol changes**: update `docs/protocol.md`, `packages/protocol`, `crates/chat-protocol`
  and `protocol/fixtures` in one commit. Both fixture tests must pass. Additive changes only:
  new optional fields, new frame and event types. Never change the meaning of a released field.
- **Server**: TypeScript on Cloudflare Workers. Validate every client frame and HTTP body with
  the Zod schemas at the boundary. Durable Objects use location hint `oc`. Each Durable Object
  migrates its own SQLite on first access with named migrations (a `migrations` table keyed by
  name such as `core-1`, `search-1`), so feature modules add steps without number clashes; new
  code must read old data. D1 migrations are `server/migrations/NNNN_<feature>.sql`; two
  independent features may share a number. Feature code lives in its own folder
  (`server/src/<feature>/`) and plugs into the core through the hooks the core exposes.
- **Secrets**: never print, log or commit tokens, invite codes or secrets. Use
  `wrangler secret put` for Worker secrets, files under `~/.config/team-chat/<env>/` (mode 600)
  for local copies, and the macOS Keychain on the client. Redact them on every log path.
- **Rust builds**: every worktree uses its own `CARGO_TARGET_DIR` and shares compiled
  dependencies through sccache: `export RUSTC_WRAPPER=sccache CARGO_BUILD_JOBS=6`. Run
  `. "$HOME/.cargo/env"` first.
- **Tooling**: bun for JS packages (`bun install`, `bun run`), cargo for Rust, and wrangler from
  the server package (`bunx wrangler` inside `server/`).
- **Tests**: server with vitest and `@cloudflare/vitest-pool-workers` (`bun run test` in
  `server/`); Rust with `cargo test`; end-to-end with `tools/e2e` against `wrangler dev`, the
  e2e environment or staging. A change is done when its tests pass and you ran them.
- **Commits**: a one-line conventional commit subject, no body. Work on your own branch; push to
  `main` only when your task says so. Never force-push `main`.
