# Changelog

Each release is tagged `v<version>` and has a GitHub Release with the DMG, the machine daemon
and their `.sha256` files. Release notes for the team are in Chinese in `docs/releases/`.

## 1.1.0 (unreleased)

Notes: [docs/releases/1.1.0.md](docs/releases/1.1.0.md). Draft: the date, the final scope and the
deploy facts are filled in at release time. The server deploys first (e2e, staging, production),
then the app and the machine daemon.

- Design v3 ([docs/DESIGN.md](docs/DESIGN.md), artboards in `docs/design/mockups-v3/`): HELMET2
  ink-on-paper tokens, ink primary buttons, grey selection, state colors amber (you), blue (live)
  and red (problem), mono numerals, member tiles with status rings, 52 pt title bars, motion with
  reduce-motion support, light, dark and Increase contrast.
- Shell: left rail · page · right pane; the topic-list column is removed. The rail has three
  modes (work, attention, members), chosen in the new 设置 → 外观 page with the theme.
- New pages: 首页 (需要你拍板, 做完了, 正在做, 排队), 群, 成员 and the 产出 viewer in the main window.
- Topic page: title bar with pane buttons, the pinned decision card above the composer, and the
  topic rail (负责, 计划, 改动, 产出, 用到的决定, 电脑, 用时和成本).
- 单独问 (⌘;): a private side chat with a member that reads the topic; a suggestion posts into the
  topic only when pressed.
- 实时查看: a member's terminal rebuilt from the run's tool events, 回放 by step, 验收标准, and
  接管 / 交还 (`run.pause` / `run.resume`: the daemon stops and continues the run's process group;
  the server suspends stall deadlines; `BotRun.pausedBy`).
- ⌘K 交代或搜索: hand work to a member (a new topic addressed to it) or search.
- Card v2 (`Message.cardV2`, additive): intents, typed blocks (decision, approval with confirm,
  form, draft, plan, metric, chart, table, diff, evidence, refs) and states; answers travel as
  `card.answer` / `card.answered` to the card's author. The server derives a v1 `card` for 1.0.x
  clients.
- Runs: members acknowledge with a reaction, post no progress messages, and end with a done card
  with proof; `BotRun.usage` and `durationMs` are stored.
- chat-core: a cache migration re-fetches group logs once, so events that 1.0.x skipped are read.
- Protocol: additive only (new optional fields and new frame and event types); closed enums are
  unchanged. See docs/protocol.md and docs/machine-protocol.md.

## 1.0.1 (2026-11-03)

Notes: [docs/releases/1.0.1.md](docs/releases/1.0.1.md). No server change.

- Bot runs: 停止 on the pending row (for the person who asked) and in the 过程 header (for
  everyone in the 群). chat-core has `Client::cancel_run` (`POST /v1/runs/:id/cancel`); a run
  that has already ended is taken quietly. The local daemon e2e stops a run through it.
- Updater: `--selftest-update <url> --token-file <file> --install-dir <dir>` updates the real
  app bundle in a folder (check, download, SHA-256, designated requirement, install, read the
  new version, remove the old bundle) without a window. `apps/desktop/src/updater/e2e-install-dir.sh`
  uses it against `wrangler dev`: a released app installs a test build signed with the same
  certificate and refuses an ad-hoc signed one. `build-dmg.sh` and `test-dmg.sh` take
  `APP_VERSION` for such local test builds.

## 1.0.0 (2026-11-03)

The first production release, for daily use by 5 people and 5 bots. Notes:
[docs/releases/1.0.0.md](docs/releases/1.0.0.md).

- Chat: groups, DMs, topics with status and `waitingOn`, @mentions, quotes, edit and delete,
  per-topic read positions, presence, typing, 待办, quiet notifications, group rename.
- Bots: bot tokens and the TypeScript bot SDK; runner-backed bots through the machine daemon,
  with streaming replies, captured runs and the process view.
- 产出: files and images up to 100 MB, HTML pages in an isolated window with versions.
- Search with Chinese two-character words, in one group and across all groups.
- Integrations: incoming webhooks and a GitHub adapter that posts PR, review and CI cards.
- Operations: separate staging and production, a monitor with alerts into 运维, a daily R2
  archive and a tested restore tool.
- Distribution: DMG on GitHub Releases and on the server, invite download links, in-app update.
- Signing: the app, its bundled daemon and the DMG are signed with one self-signed certificate,
  so the designated requirement (`identifier "co.joye.teamchat" and certificate leaf = H"…"`)
  stays the same across builds and the Keychain keeps trusting the app after an update. The
  updater refuses an update whose signature does not meet the running app's designated
  requirement (an ad-hoc running app, such as a 0.x build, accepts any valid update).
- `/v1/health` reports the release version.
