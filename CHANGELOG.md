# Changelog

Each release is tagged `v<version>` and has a GitHub Release with the DMG, the machine daemon
and their `.sha256` files. Release notes for the team are in Chinese in `docs/releases/`.

## 1.2.5 (2026-11-08)

Notes: [docs/releases/1.2.5.md](docs/releases/1.2.5.md). No D1 migration, no protocol change, no
server redeploy. 1.2.4 clients and daemons keep working.

- Product updates: a sidebar update card that follows download and install, the 更新内容 sheet,
  已更新到 on the first start of a new version, and 设置 › 通用 › 更新记录 from bundled notes.
- Sidebar rows carry one state mark and a Semibold title for unread; 新建 and the filter sit in
  the traffic-light row.
- Labeled toolbar items, the 群 as a path before the 话题 title, and a settings-style 详情
  inspector with a 提醒 select.
- 登录时启动 registers through SMAppService, 菜单栏里显示 hides the status item, 声音 controls
  the notification sound, the Dock badge shows unread, and read groups' notifications are
  withdrawn.
- 文件 and 帮助 menus, 查找 and 表情与符号 in 编辑, 服务 in the app menu, a registered 窗口
  menu and a Dock menu.
- The main window reopens at its saved frame (in `layout.json`).
- ⌘F / ⌘G / ⇧⌘G find in a 话题 or chat.
- Quick Look on Space, 共享…, 存储为… and drag-out for files and 产出.
- The pointing hand is kept for links and mentions only.

## 1.2.4 (2026-11-07)

Notes: [docs/releases/1.2.4.md](docs/releases/1.2.4.md). No D1 migration, no protocol change, no
server redeploy. 1.2.3 clients and daemons keep working.

- Card 产出 rows and the card 停止 button work again: they find the main view inside the window's
  root wrapper.
- The 产出 pane is 520 pt wide by default and its left edge drags it from 400 to 960 pt
  (double-click resets; saved in `layout.json`).
- Done cards lead with their 产出 as full-width rows and fold passed checks into 已检查 N 项.
- Working cards with no plan step name the member's live action from the process log.
- The daemon's agent note: pages open about 400 pt wide, use `window.teamchat` for state, and the
  final message is one or two sentences.
- Flush sidebar with one separator, traffic lights and sidebar toggle centered on the title line,
  the toggle pinned after the traffic lights when the sidebar is hidden, slower sidebar slide,
  calmer account footer hover and a narrower account menu.

## 1.2.3 (2026-11-06)

Notes: [docs/releases/1.2.3.md](docs/releases/1.2.3.md). No D1 migration. Protocol: wording only
in `docs/protocol.md` (a done card is posted only with proof). The server is redeployed: a run
result with no tests, acceptance, pull requests or 产出 is posted as a text message, not a done
card. 1.2.2 clients keep working.

- 设置 › 快捷键 records and saves custom shortcuts.
- Quoting a message mentions its sender.
- A 产出 opened from a message opens beside the 话题, and the 产出 page goes back to it; HTML
  产出 render live in the side pane and are hidden after leaving the 产出 page.
- Card text renders Markdown; the done-card evidence nag is gone.
- 设置登录邮箱 is a two-step sheet (email, then code) with a send spinner and animated steps.
- Smoother settings window resize, keyboard-only focus rings, switch and pane motion, no
  ellipsis on buttons, quieter select chevrons and borderless setting groups.

## 1.2.2 (2026-11-06)

Notes: [docs/releases/1.2.2.md](docs/releases/1.2.2.md).

D1 migration `0010_runner_machine` (`bot_runners.machine_id`, nullable; 1.2.1 reads the table
unchanged). Protocol, additive: `BotRunner.machineId?` and `SetBotRunnerRequest.machineId?`
(absent keeps, `null` clears). 1.2.1 clients and daemons keep working; a 1.2.1 client that saves
a runner keeps its machine.

- An owner picks the computer an AI member works on (`machineId`): 设置 › 成员 › 编辑 › 运行设置 ›
  在哪台电脑干活 (任意一台空闲电脑, or one computer; this Mac is marked 这台电脑). A pinned runner
  runs only on that machine, whatever its labels; revoking the machine unpins it.
- 设置 › 机器 marks this Mac's row 这台电脑, and its ⋯ menu has 重新启动 (restarts the daemon
  service, so a daemon from before 1.2.1 runs the app's current one).
- Checked end to end on staging with a real daemon whose `daemon.toml` still said
  `acceptEdits`: a runner pinned to it with a label no machine has got the run there, and the run
  published an HTML 产出 with `chat-run` without asking.

## 1.2.1 (2026-11-06)

Notes: [docs/releases/1.2.1.md](docs/releases/1.2.1.md). No server or protocol change; the
server is redeployed with `DEPLOY_VERSION=1.2.1` so `/v1/health` matches the release.

- AI members always have every permission: Claude Code runs with `bypassPermissions` and the
  shell, edit, web, todo, task and `chat-run` tools; Codex with sandbox `danger-full-access` and
  approval `never`. `permission_mode`, `allowed_tools`, `sandbox` and `approval_policy` in
  `daemon.toml` still parse but are ignored (a 1.1 config with `acceptEdits` no longer blocks
  `chat-run`). Asides stay read-only.
- The daemon restarts itself on a new version: when its program file is replaced (the app
  updated itself, or a new binary was copied over it) and no run is active, it exits and launchd
  starts the new one (`apps/daemon/src/selfupdate.rs`).
- 设置 › 机器 › 用这台电脑: an owner makes this Mac one of the team's computers with no
  terminal. The app registers the machine (named after the Mac, label `mac`), writes the config
  and token (mode 600), and runs `install-service` from the daemon inside the app with the login
  shell's `PATH`. A Mac whose service runs a daemon copied by hand gets 改用应用里的版本, which
  keeps its config and moves the service to the app's daemon.
- The app no longer sends requests with an empty token while it reads the saved token at launch
  (a burst of 401s in the log after each update).

## 1.2.0 (2026-11-05)

Notes: [docs/releases/1.2.0.md](docs/releases/1.2.0.md). The server deploys first, from the
commit that will be tagged, with `DEPLOY_VERSION=1.2.0` (`bun run deploy:e2e`, `deploy:staging`,
`deploy:production`), then `scripts/release.sh` makes the app, the machine daemon and the tag.
1.1.0 clients and daemons keep working against the 1.2.0 server.

D1 migrations, both new tables only: `0009_email_sign_in` (`member_emails`, `email_codes`,
`email_outbox`) and `0009_ai_service` (`ai_service`). Room Durable Objects add the named
migration `messaging-2` (`topic_states.muted`, old rows read as not muted) on first access. No
Durable Object class change. New Worker config in staging and production: the `send_email`
binding `EMAIL` and the var `EMAIL_FROM` (a sender on the operator's mail domain, Cloudflare
Email Service).
No new Worker secret: the AI 服务 key is encrypted with `HOOK_SECRET_KEY`; the
`ANTHROPIC_API_KEY` secret from #54 is no longer read.

Operators after the release: install the 1.2.0 daemon on the machine that runs the bot (new runner
permission defaults; see [docs/bots.md](docs/bots.md)), and set the AI 服务 in 设置 › 通用 (without
it, a new 话题 keeps its first line as the title).

- Email code sign-in (#52, #56): `POST /v1/auth/email/code`, `POST /v1/auth/email/sign-in` and
  `/v1/me/email`; 6-digit codes stored as SHA-256, single use, 10 minutes, 5 tries, rate limited.
  The first sign-in is still an invite code. Debug builds use their own data folder and Keychain
  service (`<bundle id>.dev`) and leave the installed app's sign-in alone; one session per server
  and a 服务器 menu.
- Several signed-in accounts with an account menu in the sidebar footer (#53).
- AI 服务 (#55): owner-only `GET/PUT/DELETE /v1/settings/ai` and `POST /v1/settings/ai/test`
  (DeepSeek, Anthropic, OpenAI-compatible); AI 话题 titles (#54, `server/src/autotitle/`) use it.
- 群聊 (#54): every 群 and DM has a 群聊 with the fixed id `main-<groupId>` (`Topic.main`, new
  optional field); 开个话题 on a message quotes it. The new-话题 sheets have no title field.
- Runners (#54): Claude Code runs with `bypassPermissions` and always allows `Bash(chat-run:*)`;
  Codex runs with `danger-full-access` and no approvals; asides stay read-only. The server strips
  the `acceptance` block from a posted result.
- Notification settings live on the 群 and 话题 pages (#50); new frame `topic.mute`,
  `TopicState.muted`; a muted 话题 notifies only on mentions.
- First run for a new member (#51): every 群 shows in the sidebar, 首页 shows 从这里开始, the
  first invite redemption makes a DM with Jojo (or the oldest bot), a 新建 menu, and 交给 in the
  new-话题 sheet.
- Message actions (#45): avatar click quotes and @s, 回复 works, R and double-click reply, 撤回
  own messages within 24 hours with 重新编辑, frequent emoji first; `bun run dev:desktop`.
- Shell (#46, #47, #48): one-line headers in a 64 pt band, fewer borders, a quiet 新话题 button,
  hide, resize and slide the sidebar, the settings gear in the sidebar footer, the profile avatar
  shows, no member 暂停 and no hand-set 话题 status.
- CI and the release workflow run on GitHub-hosted runners (#49); the staging smoke warms up and
  keeps every sample (#44).

## 1.1.0 (2026-11-05)

Notes: [docs/releases/1.1.0.md](docs/releases/1.1.0.md). `scripts/release.sh` writes the date in
the release commit. The server deploys first, from the commit that will be tagged, with
`DEPLOY_VERSION=1.1.0` (`bun run deploy:e2e`, `deploy:staging`, `deploy:production`; each one
applies the D1 migrations, then fails unless `/v1/health` reports `1.1.0`), then
`scripts/release.sh` makes the app, the machine daemon and the tag. 1.0.x clients and 1.0.x
daemons keep working against the 1.1.0 server.

D1 migrations, all additive (new nullable or defaulted columns and new tables; 1.0.x reads the
data unchanged): `0007_file_parts` (`files.upload_id`, table `file_parts`), `0007_group_purpose`
(`groups.purpose`, `groups.update_seq`), `0007_settings` (table `approval_policies`),
`0008_member_plan` (`members.reports_to`, `members.weekly_budget_cents`). The member gateway
Durable Object adds the named migration `settings-1` (notify settings, notify log, digest state)
on first access. No Durable Object class change.

Deferred to 1.2 (not in 1.1.0): real screen frames in 实时查看 (it shows the terminal rebuilt from
tool events), scheduled jobs, typed asks from CLIs through a daemon MCP tool server, secret inputs,
cloud runners, enforcement of the weekly limit, and the landing page.

Operators after the release: install the 1.1.0 daemon on the machine that runs the bot and enable
the bot's runner (`admin.ts runner`); see the notes and [docs/bots.md](docs/bots.md).

- Native Mac client (#6, [docs/design/NATIVE.md](docs/design/NATIVE.md)): the shell, sidebar,
  话题, 首页, 群, 成员, 产出, cards, sheets, alerts, settings window and login are rebuilt on a
  native kit (`apps/desktop/src/ui/native/`) over gpui-component 0.8.1 (vendored in
  `third_party/gpui-component`, Apache-2.0): toolbar, push buttons, switches, segmented controls,
  pop-up menus, sheets, alerts, SF Symbols rasterized by AppKit, key, main and inactive window
  states.
- Completeness (#7, #9–#11, #13–#41, [docs/design/COMPLETENESS.md](docs/design/COMPLETENESS.md)):
  one token module (`ui/native/tokens.rs`) with one space, size, radius, type, icon and shadow
  scale; a black and white palette like shadcn, with state colors only on badges, dots and alerts
  next to text; every page and pane has loading (skeleton), failed (alert with 重试), empty and
  worst-case states; keyboard focus rings and Tab order through the kit; one topic state and one
  set of counts shared by the sidebar, 话题 header, inspector, 首页, 群 and 成员; light, dark and
  Increase contrast parity.
- Settings window (⌘,, #6, #15, #20, #29, #40): panes 通用, 账号, 通知, 成员, 机器, 外观, 快捷键.
  Server settings routes (`server/src/settings/`, protocol `NotifySettings`, `ApprovalPolicy`,
  `Device`, the `digest` transient event and the `blocked` notify event): `GET`/`PATCH
  /v1/me/notify-settings` (events, quiet hours, per-group levels, daily digest at a local time,
  filtered in the member gateway), `GET /v1/me/devices` and `DELETE /v1/me/devices/:id` (signs a
  device out), `GET`/`PATCH /v1/settings/approvals` (the team's approval rules for AI members;
  they go into the run prompt).
- Backend gap fixes (#5, #8): anonymous card answers stay hidden from runner bots; idempotent
  `chat-run` outputs (`output.unconfirmed` on a lost ack); multipart uploads of files over 100 MB
  up to 2 GB (`PUT /v1/files/:id/parts/:n`, `POST /v1/files/:id/complete`); `POST
  /v1/groups/:id/decisions` (记一条); `PUT /v1/desk-note` (bot SDK `setDeskNote`, `chat-run desk
  note`); `POST /v1/runs/:id/retry`; session resume in follow-up runs (daemon feature `resume`);
  `Member.reportsTo` and `Member.weeklyBudgetCents` (owner-set with `PATCH /v1/members/:id`,
  stored and shown, not enforced); Card v2 member directory cache per isolate; a repeated
  `run.pause` is reported again; `POST /v1/admin/import/groups/:id/copy-files` has an executed
  test.
- Desktop wiring of backend data (#12): 成员 需要你批准的事 from the approval rules, 汇报和限额
  (owner sheet), queue times on 首页 and 成员, 修改群说明…, 记一条 with its errors, the page
  response log of a 产出, and 重试 of a member's failed run.
- Login: the 8-character invite code in 8 separate boxes; a pasted code with dashes or spaces
  fills them.
- Design v3 ([docs/DESIGN.md](docs/DESIGN.md)) as the first 1.1.0 layout; the native client and
  the completeness work above replace its colors, sizes and controls. DESIGN.md still owns Card
  v2, status words and content.
- Shell: sidebar · page · right pane; the topic-list column is removed. The sidebar has three
  styles (以工作为中心, 以「需要我」为中心, 以成员为中心), chosen in the 侧栏选项 menu above the list.
- New pages: 首页 (需要你拍板, 今天交付, 正在做, 排队), 群, 成员 and the 产出 viewer in the main window.
- Topic page: title bar with pane buttons, the pinned decision card above the composer, and the
  topic rail (负责, 计划, 改动, 产出, 用到的决定, 在哪干活, 用时和成本).
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
- Release: `scripts/deploy-server.sh` refuses a `0.0.0*` version and checks `/v1/health` after the
  deploy (`--check-health [--expect <v>]` checks it alone); `scripts/release-check.sh <v>` checks
  the workspace version, Cargo.lock, this file and the notes (`--tag`, `--env <env>`).

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
