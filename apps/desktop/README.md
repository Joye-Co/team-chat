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
| `desktop` | Opens the app with the fake provider (until the chat-core provider lands). |
| `desktop --signed-out` | Starts at the login view. Any code signs in except `WRONG-CODE-0000`. |
| `desktop --screenshot <dir> [--theme light\|dark\|both] [--scenes a,b] [--worst-case] [--size WxH]` | Captures each scene window in each theme to `<dir>/<scene>-<theme>[-worst].png` plus `screenshots.json`, without focus, and exits. `desktop --list-scenes` lists scenes; add one in `src/selftest/scenes.rs`. |
| `desktop --selftest-smoke --report <path> [--hold-ms <n>]` | Opens the window on the 10,000-message topic without focus or a Dock icon, writes JSON (`ok`, `first_frame_ms`, counts) and exits (0 = ok). |

## Module map

| Path | Owner track | Holds |
| --- | --- | --- |
| `build.rs` | skeleton | `PRODUCT_NAME`, `PRODUCT_SLUG`, `PRODUCT_BUNDLE_ID`, `PRODUCT_URL_SCHEME`, `PRODUCT_STAGING_URL`, `PRODUCT_PRODUCTION_URL` from `product.json` |
| `src/main.rs` | skeleton | Arguments and self-tests |
| `src/app.rs` | skeleton, navigation | `Services`, `ChangeHub`/`Changes`, `Navigation`/`Place`, `RootView`, windows, menus, actions |
| `src/theme.rs` | skeleton | Follows system light or dark; `theme::colors(cx)`, status chip colors, layout sizes |
| `src/data.rs` | chat-core provider | `ChatProvider` trait and plain data types |
| `src/fake.rs` | all | `FakeProvider` with sample data |
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
