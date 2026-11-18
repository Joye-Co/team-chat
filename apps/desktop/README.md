# desktop

The macOS and Windows app: Rust, GPUI and gpui-component.

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
(each worktree sets its own `CARGO_TARGET_DIR`; `<worktree>/.tmp/target` is the default of
`bun run dev:desktop`).

## Run

| Command | What it does |
| --- | --- |
| `desktop` | Opens the real app (sign in to a server). |
| `desktop --sandbox <dir>` | The real app on the local sandbox server whose `server-url` and `owner-token` are in `<dir>`, signed in as the sample owner; its files and token stay under `<dir>/app` (no Keychain, no Application Support). `bun run dev:desktop` starts the server (`scripts/sandbox/up.ts`), seeds it once (`scripts/sandbox/seed.ts`) and opens the app this way, again on every save. |
| `desktop --fake` | The in-process samples of the screenshot self-tests (`src/fake/v2.rs`); not kept in step with every server feature. Use the sandbox to look at UI work. |
| `desktop --signed-out` | Starts at the login view. Any code signs in except `0000-0000` (and the legacy `WRONG-CODE-0000`). |
| `desktop --screenshot <dir> [--theme light\|dark\|both] [--scenes a,b] [--worst-case] [--size WxH] [--settle-ms <n>] [--lang en]` | Captures each scene window in each theme to `<dir>/<scene>-<theme>[-worst].png` plus `screenshots.json`, without focus, and exits (0 = every PNG written and every scene check passed). `--settle-ms` sets how long a scene waits before its capture. `--lang en` draws the scenes in English from the table `src/selftest/english.txt` (the app's copy stays Chinese); text with no entry is listed in `<dir>/untranslated.txt`. `desktop --list-scenes` lists scenes; add one in `src/selftest/scenes.rs` or in the feature folder's `scenes.rs` (`src/views/<feature>/scenes.rs`). |
| `desktop --selftest-live <url> --token-file <path> --report <path> [--read-only]` | Signs in to a real server with the token, starts a topic and sends a message, then restores the session. `--read-only` only connects and catches up every group (no window, no writes): safe against production. With `--interact <dir>` it runs the live check of the card interaction loop and 引用提问 instead and saves `interact-rev2.png` and `interact-ref.png` to `<dir>`; `scripts/live-interact-check.ts` prepares the server side first. |
| `desktop --selftest-update <url> --token-file <path> --install-dir <dir> --report <path>` | Updates the real `<name>.app` in `<dir>` (a copy of a released app, never `/Applications`) the way the app updates itself: check as that bundle's version, download, SHA-256, install only when the new bundle satisfies the old designated requirement, read the new version, remove the old bundle. No window, no relaunch. With `--workdir <dir>` and no `--install-dir` it runs the older fake-bundle path (`src/updater/e2e.sh`). `src/updater/e2e-install-dir.sh <old.dmg>` proves a signed update and an ad-hoc refusal against `wrangler dev` on port 8816. |
| `desktop --selftest-smoke --report <path> [--hold-ms <n>]` | Opens the window on the 10,000-message topic without focus or a Dock icon, writes JSON (`ok`, `first_frame_ms`, counts) and exits (0 = ok). |
| `desktop --selftest-timeline <n> --report <path>` | Opens a topic with `<n>` generated messages (10000 is the usual count), scrolls from the newest message to the oldest and back by code, and writes JSON (`first_frame_ms`, `frames`, `p50_ms`, `p95_ms`, `max_ms`). |
| `desktop --selftest-cards --report <path> [--shots <dir>]` | Draws every sample card with the fake provider in light and then dark mode, presses buttons through the provider, checks what was drawn and writes JSON (0 = every check passed). `--shots` also saves `cards-light.png` and `cards-dark.png`. |
| `desktop --selftest-nav --report <path>` | Opens the main window with fake data without focus, drives the navigation actions and dialogs, and writes JSON. |
| `desktop --selftest-settings --report <path> [--live <url> --token-file <path>]` | Opens the settings window without focus, walks every page and drives its forms (profile, invite, AI member, machine, levels, diagnostics), and writes JSON (never a secret). Without `--live` it uses the fake provider; with it, use a local `wrangler dev`: the test writes members, invites and machines. |

## Module map

| Path | Owner track | Holds |
| --- | --- | --- |
| `build.rs` | skeleton | `PRODUCT_NAME`, `PRODUCT_SLUG`, `PRODUCT_BUNDLE_ID`, `PRODUCT_URL_SCHEME`, `PRODUCT_STAGING_URL`, `PRODUCT_PRODUCTION_URL` from `product.json` |
| `src/main.rs` | skeleton | Arguments and self-tests |
| `src/app.rs` | skeleton, navigation | `Services`, `ChangeHub`/`Changes`, `Navigation`/`Place`, `RootView`, windows, menus, actions |
| `src/theme.rs` | skeleton | Follows system light or dark and loads the tokens into gpui-component's theme; `theme::colors(cx)`, `theme::motion` (durations) |
| `src/ui/native/` | all | The native component kit. `tokens.rs` is the one token module (colors, heights, radii, spacing, text sizes, shadows; docs/design/COMPLETENESS.md §1); the other files are the building blocks of every screen (toolbar, sidebar, buttons, forms, fields, menus, sheets, pages, `window_chrome.rs` for the Windows title bar) |
| `src/ui/` | all | Shared parts on top of the kit (avatars, chips, rows, icons, SF Symbols, clipboard, motion) |
| `src/data.rs`, `src/data/` | chat-core provider | `ChatProvider` trait and plain data types |
| `src/provider/` | chat-core provider | The real provider: `ChatProvider` on `chat_core::Client` (session, cache reads, commands), the sandbox sign-in (`sandbox.rs`), the system glue of each OS (`macos.rs`, `windows.rs`) and the live self-tests |
| `src/fake.rs`, `src/fake_extra.rs`, `src/fake/` | all | `FakeProvider` with the sample data that `--fake` and the screenshot scenes draw; its bots are sample members |
| `src/selftest/` | all | The screenshot self-test: the runner (`mod.rs`), the scenes (`scenes.rs`, which chains each feature's `src/views/<feature>/scenes.rs`), worst-case data, window capture per OS, the `--lang en` table |
| `src/updater/` | update | In-app update: check, download, verify, install and relaunch; its self-test and the two `e2e*.sh` scripts |
| `src/paths.rs`, `src/win32.rs` | skeleton | Where the app keeps its files on each system; Win32 helpers of the Windows integration |
| `src/characters/` | bot identity | Member characters: `image(character, status, size)`, the bot status rule and `StatusBoard`, the 16 emotions and the look rule, the vendored Jojo & Friends avatar pack (`assets/characters/`, embedded by `build.rs`) |
| `src/copy/` | all | The copy catalog, one file per area; `dev.rs` is the developer view |
| `src/views/sidebar.rs`, `topic_list.rs` | navigation | 待办, 群, 私聊; topic list |
| `src/views/timeline.rs` | timeline | Topic header and message list |
| `src/views/composer.rs` | composer | Message field |
| `src/views/login.rs`, `settings/`, `side_panel.rs` | skeleton for now | Login, settings window (one file per page), right panel |
| `src/views/<feature>/` | each feature | The other screens, one folder each (`desk/`, `topic_page/`, `group_page/`, `members_page/`, `cards/`, `admin/`, ...), with the feature's `scenes.rs` beside its views |

## Rules for views

- Read snapshots from `services.provider` (synchronous, never waits). Start work with its
  commands; they return at once.
- Subscribe to changes: `cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| ...)`.
  Re-read only what a `Change` names, then `cx.notify()`.
- Navigation: `cx.observe(&services.nav, ...)` and `services.nav.update(cx, |nav, cx| ...)`.
- Every user-visible string comes from `copy::*`; placeholders with `copy::fill`. Developer words
  only in `copy::dev`. `cargo test -p desktop` checks this against `docs/PRODUCT.md`.
- Colors only from `theme::colors(cx)` or `cx.theme()`; sizes, radii, spacing and text sizes only
  from `ui::native::tokens`; durations from `theme::motion`. No literal in a view.
