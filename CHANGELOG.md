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
- Card v2.1 (`catalog: "tc.card/2.1"`, artboards CardsCompose and Interact): containers `stack`, `row`
  (weights, folds to a stack under 560 pt), `group`, `compare`, `tabs` and `questionnaire` (depth ≤ 2, 80
  blocks); the `decision` block gains `allowOther`, `minSelect` and `maxSelect`, and the new choice forms `rank`, `rating`, `scale`,
  `confirm`, `imageChoice`, `matrix`, `slots` and `poll`; `buttons`, input `slider`, table row select, `diagram`
  and `embed`. Every card has a status band (等你, 等{name}, 处理中, 完成, 失败, 已作废). Unknown blocks show a
  box with their `fallbackText`; the derived v1 card covers every 2.1 type.
- Interaction loop: one `card.event` frame for every pick, chart point, table row, input and button
  (`card.answer` stays as its short form); the author answers with `card.patch` (`message.updated` with
  `cause`), a new message, or `interaction.resolve`. The server owns `Message.cardState` (ask rounds, expiry,
  retract, poll tally). The app shows a pending band (等 {name} 回应, {name} 还在处理, {name} 没有回应 with
  重试 and 不等了), re-renders on a conflict with a toast. A rev with a `cause` gets the update band ({name} 更新了这张卡 · 刚刚 ·
  按你点的「…」 for the person whose event caused it, 按 {name} 的选择 for others); changed and inserted top-level
  blocks sit with no gap and a hairline between them; changed ones flash and settle on the card color, and
  inserted ones keep a light tint and {name} 刚加的 until the next rev. A pickable single-series bar chart is drawn as rows that keep the chart label, the summary
  and the pick chip (left out when that pick caused the rev). Two short single-select options sit side by side.
  The card header shows the creation time; table number columns are right-aligned mono at the row end with
  32 pt rows, and the table-row 问这里 follows the row name.
  New transients go only to clients that send `hello.features` `interact/1`.
- 产出 pages talk both ways: the bridge `window.teamchat` (`getData`, `onData`, `emit`, `ask`, `ref`); page events
  travel as `artifact.event` to the version's author and are audited, not stored; the author pushes
  `artifact.data` into open pages and answers with `artifact.reply`. Only versions made by a bot member may emit
  or ask, and only declared events. An open page reloads in place on a new version. Inline pages start at the top and fade at the cut edge; the
  footer keeps 页面里的操作会发给 {name} on one line (wrapping only before the name) and cuts the event labels
  with an ellipsis and a tooltip.
- Runner members get card and page events as `run.answer` / `run.interaction`, and post, patch and reply
  through `chat-run` on the run's PATH.
- 引用提问 (`refs/1`, artboard Refs): `PartRef` and `send.blockRefs` (≤ 5 per message) for a table cell or row, a
  chart point, a metric, a step, a diff line, a screenshot, a diagram node or a page element; 问这里 on hover,
  ⌥-click, a context menu and the keyboard; chips in the composer and on sent messages. 1.0.x clients get a text
  prefix in the body; `buildPrompt` passes the refs to runner members.
- Diagrams (`diagram/1`): Mermaid fences in Markdown and the `diagram` block, rendered on this computer with no
  network, with the app's colors in light and dark; an expand viewer with zoom; node refs. Error states keep
  the source and name the failed line or the node count; a too large diagram (over 150 nodes, 300 edges or 16 KiB
  of source) has its own quiet state. The renderer window sends whole SVGs past the 16 KiB page payload limit.
- Media 产出 (artboard Media): `MediaInfo` on `ArtifactVersion.media` and on file attachments, computed by the
  sender, with `artifact.previewReady` as the fallback. Inline and full views for live HTML, Markdown, images,
  video (poster, chapters, frames), audio (waveform, key points, transcript), PDF, code and CSV tables; upload
  and preview states.
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
