# Repository rules

Read [docs/PRODUCT.md](docs/PRODUCT.md) (what we build, interface words, V1 bar),
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (layout, components, environments) and
[docs/protocol.md](docs/protocol.md) (wire contract) before you change code.

- **Public repo**: this repo is public open source (AGPL-3.0). Never commit an operator's
  deployment details (server URLs, D1/Worker ids, member/group/bot ids, who runs which bot on
  which machine, rosters, private assets, local paths). They live in the private ops checkout
  (`TC_OPS_DIR`, see [docs/ENVIRONMENTS.md](docs/ENVIRONMENTS.md)); docs use placeholders.
- **Product name**: only in `product.json`. The server imports it, `apps/desktop/build.rs`
  exports it as compile-time constants, scripts read it with bun or jq. Never type the product
  name in code or UI strings. Infra ids (Worker, D1, crate and package names) use neutral names
  and do not change when the product is renamed. After a rename, add the old name to
  `formerNames` in `product.json`: the Windows installer keeps it as a second name of the app's
  program, because an installed app starts `<old name>.exe` after it updates itself. Server URLs are not in `product.json`: the app and the
  scripts read them from `deploy/servers.json` in the ops checkout.
- **Interface text**: Simplified Chinese, only from the desktop copy catalog, only the four
  product nouns 成员, 群, 话题, 产出 (see PRODUCT.md). Developer words stay in the developer view
  and in logs. The catalog test enforces this.
- **Design bar**: first-class visual design is a V1 requirement, not polish for later. Build
  every UI element from gpui-component (the first-class GPUI component library: button, input,
  list, popover, menu, dialog, tooltip, badge, avatar, tabs, switch, notification and the rest),
  styled through the theme; never hand-roll a control that gpui-component has. The binding
  finish spec is [docs/design/COMPLETENESS.md](docs/design/COMPLETENESS.md): pure black and white
  like shadcn, one token module (`apps/desktop/src/ui/native/tokens.rs`), one scale, every state
  drawn. It wins over NATIVE.md and DESIGN.md where they differ, and a screen is done only when it
  passes its §4 checklist. The app is a
  Mac client, not a web page: follow [docs/design/NATIVE.md](docs/design/NATIVE.md) (window,
  toolbar, sidebar, controls, SF Symbols, sheets, settings window, behavior), which wins over
  [docs/DESIGN.md](docs/DESIGN.md) where they differ; DESIGN.md still owns Card v2, status words
  and content. The Windows app is the same app: [docs/design/WINDOWS.md](docs/design/WINDOWS.md) lists
  the only differences from NATIVE.md there (title bar and window buttons, in-window menus and
  tray, Ctrl shortcuts, fonts, the SVG icon set, scrollbars, dialogs), and COMPLETENESS.md binds
  both systems. Keep every color, size and duration in theme tokens. Look at your screens: capture
  only the app's own window (`screencapture -l <window id> -o -x <png>`, or the app's
  screenshot self-test when it exists), never the full screen, in light and dark mode, and read
  the PNGs. Before merging UI work, check it with the vendored skills in
  `docs/design/skills/emilkowalski/`: `emil-design-eng` (polish), `review-animations` (motion),
  `break-ui` (worst-case data) and `apple-design` (macOS feel). Web pages, if any, use the
  libraries in `pick-ui-library`.
- **Protocol changes**: update `docs/protocol.md`, `packages/protocol`, `crates/chat-protocol`
  and `protocol/fixtures` in one commit; write fixtures in `packages/protocol/scripts` and run
  `bun run gen:protocol`, never edit `protocol/fixtures` or `protocol/schema` by hand. Both
  fixture tests must pass. Additive changes only:
  new optional fields, new frame and event types. Never change the meaning of a released field.
- **Server**: TypeScript on Cloudflare Workers. Validate every client frame and HTTP body with
  the Zod schemas at the boundary. Durable Objects use location hint `oc`. Each Durable Object
  migrates its own SQLite on first access with named migrations (a `migrations` table keyed by
  name such as `core-1`, `search-1`), so feature modules add steps without number clashes; new
  code must read old data. D1 migrations are `server/migrations/NNNN_<feature>.sql`; two
  independent features may share a number. Feature code lives in its own folder
  (`server/src/<feature>/`) and plugs into the core through the hooks the core exposes.
- **Secrets**: never print, log or commit tokens, invite codes or secrets. Use
  `wrangler secret put` for Worker secrets (`-c wrangler.deploy.jsonc --env <env>` on a deployed
  env), files under `~/.config/team-chat/<env>/` (mode 600)
  for local copies, and the macOS Keychain on the client. Redact them on every log path.
- **Rust builds**: every worktree uses its own `CARGO_TARGET_DIR` and shares compiled
  dependencies through sccache: `export RUSTC_WRAPPER=sccache CARGO_BUILD_JOBS=6`. Run
  `. "$HOME/.cargo/env"` first.
- **Disk space**: a desktop debug target takes 10–30 GB, and parallel worktrees filled the
  whole disk once. For agent and check builds also set `CARGO_PROFILE_DEV_DEBUG=0
  CARGO_INCREMENTAL=0`. Before you start builds, check `df -h ~`: stop and report when less
  than 60 GB is free. When a branch is merged or abandoned, delete its worktree's
  `CARGO_TARGET_DIR`; delete only target folders whose builds have stopped.
- **Local processes**: keep PID files, logs and scratch files inside your own worktree under
  `.tmp/` (git ignores it), never in a shared folder such as a scratchpad: other agents run at
  the same time. Stop only processes you started, after checking that `ps -p <pid> -o command=`
  shows your worktree path.
- **Tooling**: bun for JS packages (`bun install`, `bun run`), cargo for Rust, and wrangler from
  the server package (`bunx wrangler` inside `server/`). `bun install` turns on the
  `.githooks/pre-commit` hook, which runs rustfmt on the staged Rust files; never skip it with
  `--no-verify`.
- **Look at UI work in the sandbox**: `bun run dev:desktop` opens the real app (the real provider,
  nothing faked) on this worktree's local server with the sample team of
  `scripts/sandbox/seed.ts`, signed in, and reopens it on every save. When a feature needs data
  the samples lack, add it to `seed.ts` through the public API in the same change; CI seeds a
  fresh server on every PR. Do not add features to the in-process fake (`src/fake/`): it only
  feeds the screenshot self-tests.
- **Tests**: server with vitest and `@cloudflare/vitest-pool-workers` (`bun run test` in
  `server/`); Rust with `cargo test`; end-to-end with `tools/e2e` against `wrangler dev`, the
  e2e environment or staging. A change is done when its tests pass and you ran them.
- **Commits**: a one-line conventional commit subject, no body. Work on your own branch; push to
  `main` only when your task says so. Never force-push `main`.
