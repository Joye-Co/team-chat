# desktop

The macOS app: Rust, GPUI and gpui-component.

## Versions

| Crate (Cargo name) | Package and version | Why |
| --- | --- | --- |
| `gpui` | `gpui-pre =0.3.10` | The GPUI snapshot that gpui-component 0.8.1 pins. The crates.io `gpui 0.2.2` is a year old and no current gpui-component uses it. |
| `gpui_platform` | `gpui-pre-platform =0.3.10`, feature `runtime_shaders` | The platform crate owns the shader build. `runtime_shaders` compiles the Metal shaders when the app starts, so only the CommandLineTools are needed (no `metal` compiler, no full Xcode). |
| `gpui-kit` | `=0.8.1` | `gpui_kit::init`, `gpui_kit::open_window` (mounts the Root that dialogs, popovers, notifications and inputs need) and the Lucide icon assets. |
| `gpui-component` | `=0.8.1` | The components (button, input, textarea, list, markdown, switch, ...). 0.8.1 was a week old when chosen; 0.9.0 was one day old. |

All are pinned with `=`. To upgrade, move `gpui-kit` and `gpui-component` together and set
`gpui`/`gpui_platform` to the `gpui-pre` version that `gpui-component` pins (see its
`Cargo.toml`), then run the build, the tests and the smoke self-test.

Build: `. "$HOME/.cargo/env"; export RUSTC_WRAPPER=sccache CARGO_BUILD_JOBS=6; cargo build -p desktop`
(each worktree sets its own `CARGO_TARGET_DIR`).

## Run

| Command | What it does |
| --- | --- |
| `desktop` | Opens the real app (sign in to a server). |
| `desktop --sandbox <dir>` | The real app on the local sandbox server whose `server-url` and `owner-token` are in `<dir>`, signed in as the sample owner; its files and token stay under `<dir>/app` (no Keychain, no Application Support). `bun run dev:desktop` starts the server (`scripts/sandbox/up.ts`), seeds it once (`scripts/sandbox/seed.ts`) and opens the app this way, again on every save. |
| `desktop --fake` | The in-process samples of the screenshot self-tests (`src/fake/v2.rs`); not kept in step with every server feature. Use the sandbox to look at UI work. |
| `desktop --signed-out` | Starts at the login view. Any code signs in except `0000-0000` (and the legacy `WRONG-CODE-0000`). |
| `desktop --screenshot <dir> [--theme light\|dark\|both] [--scenes a,b] [--worst-case] [--size WxH]` | Captures each scene window in each theme to `<dir>/<scene>-<theme>[-worst].png` plus `screenshots.json`, without focus, and exits. `desktop --list-scenes` lists scenes; add one in `src/selftest/scenes.rs`. |
| `desktop --selftest-live <url> --token-file <path> --report <path> [--read-only]` | Signs in to a real server with the token, starts a topic and sends a message, then restores the session. `--read-only` only connects and catches up every group (no window, no writes): safe against production. |
| `desktop --selftest-update <url> --token-file <path> --install-dir <dir> --report <path>` | Updates the real `<name>.app` in `<dir>` (a copy of a released app, never `/Applications`) the way the app updates itself: check as that bundle's version, download, SHA-256, install only when the new bundle satisfies the old designated requirement, read the new version, remove the old bundle. No window, no relaunch. With `--workdir <dir>` and no `--install-dir` it runs the older fake-bundle path (`src/updater/e2e.sh`). `src/updater/e2e-install-dir.sh <old.dmg>` proves a signed update and an ad-hoc refusal against `wrangler dev` on port 8816. |
| `desktop --selftest-smoke --report <path> [--hold-ms <n>]` | Opens the window on the 10,000-message topic without focus or a Dock icon, writes JSON (`ok`, `first_frame_ms`, counts) and exits (0 = ok). |

## Module map

| Path | Owner track | Holds |
| --- | --- | --- |
| `build.rs` | skeleton | `PRODUCT_NAME`, `PRODUCT_SLUG`, `PRODUCT_BUNDLE_ID`, `PRODUCT_URL_SCHEME`, `PRODUCT_STAGING_URL`, `PRODUCT_PRODUCTION_URL` from `product.json` |
| `src/main.rs` | skeleton | Arguments and self-tests |
| `src/app.rs` | skeleton, navigation | `Services`, `ChangeHub`/`Changes`, `Navigation`/`Place`, `RootView`, windows, menus, actions |
| `src/theme.rs` | skeleton | Follows system light or dark; `theme::colors(cx)`, status chip colors, layout sizes |
| `src/data.rs` | chat-core provider | `ChatProvider` trait and plain data types |
| `src/fake.rs` | all | `FakeProvider` with sample data; its bots are sample members |
| `src/characters/` | bot identity | Member characters: `image(character, status, size)`, the bot status rule and `StatusBoard`, the optional character pack (from `TC_OPS_DIR`, embedded by `build.rs`) |
| `src/copy/` | all | The copy catalog, one file per area; `dev.rs` is the developer view |
| `src/views/sidebar.rs`, `topic_list.rs` | navigation | 待办, 群, 私聊; topic list |
| `src/views/timeline.rs` | timeline | Topic header and message list |
| `src/views/composer.rs` | composer | Message field |
| `src/views/login.rs`, `settings.rs`, `side_panel.rs` | skeleton for now | Login, settings window, right panel |

## Rules for views

- Read snapshots from `services.provider` (synchronous, never waits). Start work with its
  commands; they return at once.
- Subscribe to changes: `cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| ...)`.
  Re-read only what a `Change` names, then `cx.notify()`.
- Navigation: `cx.observe(&services.nav, ...)` and `services.nav.update(cx, |nav, cx| ...)`.
- Every user-visible string comes from `copy::*`; placeholders with `copy::fill`. Developer words
  only in `copy::dev`. `cargo test -p desktop` checks this against `docs/PRODUCT.md`.
- Colors only from `theme::colors(cx)` or `cx.theme()`.
