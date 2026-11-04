# Product brief

The short version of the design doc ([snapshot](design/design-doc.zh.md)) that code work needs.
When this file and the snapshot disagree, this file wins for V1.

## What it is

A macOS work chat for one small team: Joye, 4 to 10 interns, and about 5 bots. People and bots
are equal members. Development, product and other work are discussed, assigned and delivered
here, so the team stops using Feishu and Slack. Every message belongs to a topic, like a Feishu
topic group.

## Interface words

The interface is in Simplified Chinese. It uses only these four nouns for product concepts:

| UI word | Code name | Means | Merges these developer words |
| --- | --- | --- | --- |
| 成员 | `Member` | A person or a bot. Shows name, avatar and 岗位 (role). A bot's avatar is its jojo-friends character with its live status signal, and its 岗位 tag carries a small bot glyph; no other mark (Joye, 2026-11-03). | agent, bot, teammate |
| 群 | `Group` | Where members talk. A DM is a 群 of two people. | chat, channel, DM |
| 话题 | `Topic` | One piece of work: discussion, progress, questions and 产出 hang under it. | thread, task, ticket, session |
| 产出 | `Artifact`, `File` | What a topic produces: pages (HTML), files, decisions. | artifact, attachment |

- Topic status chips say who is waited on and for what:

  | `TopicStatus` | Chip when the viewer is in `waitingOn` | Chip otherwise |
  | --- | --- | --- |
  | `open` | (no chip) | (no chip) |
  | `in_progress` | 进行中 | 进行中 |
  | `waiting_answer` | 等你回答 | 等{name}回答, or 等回答 |
  | `waiting_review` | 等你审 | 等{name}审, or 等审 |
  | `blocked` | 卡住了 | 卡住了 |
  | `done` | 完成 | 完成 |

- Status words are a closed set, one meaning each (docs/DESIGN.md §9.1): 需要你拍板 (the viewer
  must decide or review), 等你 (its short form in lists), 等{name} (waiting on someone else),
  正在做 / 在做 (live work), 排队 (queued), 做完了 (a result arrived), 卡住了 (a problem), 完成 (the
  topic is closed). The `TopicStatus` chips above map onto them in the copy catalog.
- Other UI words allowed from 1.1.0: 单独问 (a private side chat with a member about a topic),
  实时查看 (watch a member's computer while it works), 接管 and 交还 (pause the member and take over,
  then hand back), 交代 (hand work to a member), 来龙去脉 (where a 产出 came from), 群里的决定 (the
  decisions recorded in a 群), 验收标准 (the checks a result must pass), 回放 (replay a run by step).
- Words for cards, the interaction loop and 产出 pages (1.1.0 part 2; DESIGN.md §7.13, §7.14): the
  card band words 等你, 等{name}, 处理中, 完成, 失败, 已作废, 已过期; the choice forms 单选, 多选, 排序,
  评分 (打分), 量表, 是或否, 图片选择, 矩阵, 时间段, 投票, 问卷, 对比; and 等 {name} 回应, {name} 还在处理,
  {name} 没有回应, 重试, 不等了, {name} 不在线，这一步没有发出, {name} 不在线，回来后会看到,
  {name} 现在接不了，稍后再试, 卡片刚更新了，请再看一眼, 内容变了, 用新的, {name} 更新了这张卡,
  刚刚, 按你点的「…」, 按你的选择, 按 {name} 的选择, {name} 刚加的, {name} 回了一条新消息, 已更新到 v{n}, 有 v{n}, 页面里的操作会发给 {name},
  回答会记在 {name} 的工作记录里, 回答会记进工作记录, 页面能做的：读这份数据.
- Words for 引用提问 and diagrams (DESIGN.md §7.15, §7.16); 图 (a diagram) is an allowed word, not a
  product noun: 问这里, 就这里提问, 单独问这里, 就这张卡提问, 最多引用 5 处, 已更新, 原消息已删除,
  已放进话题的输入框, 去提问, 整行, 整列, 全部, 第 {n} 步, 第 {n} 行, 第 {k} 段, 截图 {k}, 图片, 表格, 图表,
  指标, 清单, 计划, 改动, 证据, 图, 代码, 正在画图, 放大, 缩小, 适合窗口, 看原文, 复制原文, 复制图片, 展开,
  图没有画出来, 这种图还画不了, 图太大了，只显示原文, 图里有不支持的写法, 第 {n} 行看不懂, {n} 个节点, 流程图, 时序图, 状态图, 类图,
  关系图, 甘特图, 时间线, and the 产出 footer chip 图 n. Never `Mermaid`, `block`, `render` or `event`
  in UI text.
- The settings window may also use 岗位 (role and how the member works), 权限 (permissions),
  机器 (machines) and 成本 (cost).
- Developer words appear only in the developer view (开发者视图) and in logs: session, CLI,
  model names, router, worker, runner, trigger, token, agent, bot, chat, thread, channel, DM,
  task, ticket, artifact, attachment, and their Chinese forms 会话, 机器人, 智能体, 频道, 线程,
  任务, 工单, 附件, 令牌.
- Every user-visible string comes from one copy catalog in the desktop app. A test fails when a
  catalog entry outside the developer-view section contains a developer word. The product name
  comes from `PRODUCT_NAME` (product.json) and is never typed.

## Screens (desktop)

The layout and look of every screen are in [DESIGN.md](DESIGN.md) v3 and its artboards
([design/mockups-v3/](design/mockups-v3/)).

- **Shell**: one main window: a left rail, the page, and one right pane. Every page has a title
  bar with the place, its state, and buttons that open the right panes.
- **Left rail**: search (⌘K), the destinations 首页, 产出 and 成员, the list of work, the active
  members, and me. The list has three modes; each person picks one in 设置 → 外观 (侧栏样式) or in
  the rail's header menu, and the choice is kept on this computer:
  - **A 以工作为中心** (default): 话题 grouped by 群, then 私聊; each row shows one state (needs me,
    who is working, waiting on someone, done today). A filter shows only the topics I take part in.
  - **B 以「需要我」为中心**: sections 需要你, 在做, 等别人, 做完了 · 今天; 群 are filter chips.
  - **C 以成员为中心**: each member with the topics it is doing, then people.
- **首页**: Jojo's one-line priority for today (only when Jojo wrote one), 需要你拍板 (every card and
  topic that waits on me, answerable in place), 做完了 today with proof, and a column 正在做 with
  each active run, its plan progress and, when it uses its computer, a live thumbnail; 排队 when
  runs are queued.
- **话题**: the timeline (virtualized, anchored at the bottom, Markdown, images inline, cards
  rendered natively), the newest decision that waits on me pinned above the composer, and the
  composer (multi-line, Chinese IME, @mentions, quote a message, paste or drop images and files).
  On the right, the topic rail: 负责, 计划, 改动, 产出, 用到的决定, 电脑, 用时和成本; a card with no
  data is hidden.
- **单独问** (⌘;): a side chat with a member (Jojo by default) beside a topic. The member has read
  the topic up to now. Only I see the questions and answers. A suggested step goes into the topic
  only when I press its button.
- **实时查看 + 接管**: a member's computer beside the topic while it works: its terminal rebuilt from
  the run's steps, the plan, 验收标准, and 回放 by step. 接管 pauses the member so I can act; 交还 lets
  it continue from that step; 停止 cancels the run.
- **⌘K 交代或搜索**: one field to hand work to a member (交给 <member> starts a topic addressed to
  it in the chosen 群; ⌘↵ hands it to Jojo) or to find 话题, 产出 and 成员.
- **群**: a composer to start a topic, the topics 进行中 with live previews, 最近完成, and a rail with
  群里的决定 (recorded from answered decisions, or added with 记一条), 最近的产出 and the 电脑 in use.
- **成员**: every member with filters (全部, 在做事, 等人); a member's page shows 现在, 这周 (topics,
  产出, cost), 最近交付, 需要批准的事, 能用的工具, 电脑 and 最近的问题. Changing a member opens the settings
  forms.
- **产出 viewer**: a 产出 in the main window with its 来龙去脉 (the topic, who made it, who asked,
  versions, choices made in the page, decisions used, where it is cited). HTML pages stay isolated
  (one origin per 产出, strict CSP, no network); they also open in their own native window.
  Choices inside a page a member made go back to that member. Files open with the default macOS
  app.
- **Cards (Card v2)**: members send semantic cards and the app draws them the same way on 首页, in
  话题, 群 and notifications: choices, approvals with a confirm step, forms, drafts to confirm or
  edit, plans, numbers and charts, tables, diffs, evidence, and links. A card shows who must act;
  only they get live controls. After an answer the card becomes a one-line record of who answered
  and when.
- **Composed cards and choice forms (Card v2.1)**: a card combines blocks (上下叠, 并排, 分组, 对比,
  标签页, 问卷; 并排 becomes 上下叠 in a narrow window) and asks with more forms: 排序, 打分, 量表, 确认,
  图片选择, 矩阵, 时间段 and 投票. A status band at the top of every card says the state once (等你,
  等{name}, 处理中, 完成, 失败, 已作废). A question with two short options shows them side by side as
  two buttons; three or more are stacked. The card header shows when the card was sent, also after
  the member changes it. In card tables, number columns are right-aligned. Artboard: CardsCompose.
- **The interaction loop**: every pick, chart point, table row, input and button in a card goes
  back to the member that made it; the member changes the card in place. The card's band then says
  {name} 更新了这张卡 with 刚刚 · 按你点的「B 站」 for the person whose pick caused it and 按 {name} 的选择
  for everyone else; blocks the member added say {name} 刚加的 and keep a light tint until the next
  change. A bar chart you can pick from reads as rows, one per bar, and the picked bar is dark. While the member has not answered, the band says 等 {name} 回应, then
  {name} 还在处理, then {name} 没有回应 with 重试 and 不等了. 产出 pages talk both ways: page buttons go to
  the member, the member pushes new data into the open page or a new version, and the page can ask
  the member and show the answer. Under the page, 页面里的操作会发给 {name} says who gets its
  actions, followed by what the page can send. Artboard: Interact.
- **引用提问**: point at one part (a table cell or row, a chart point, a metric, a plan step, a diff
  line, a screenshot, a diagram node, an element of a 产出 page) with 问这里, ⌥-click or the context
  menu; it becomes a chip in the composer (at most 5) and the member gets the question with the part
  and the value that was seen. 单独问这里 puts it into 单独问. Artboard: Refs.
- **Diagrams (图)**: flowcharts, sequence, state, class, ER, gantt and timeline diagrams in messages,
  cards and Markdown 产出, drawn on this computer in the app's colors; a viewer with zoom; when one
  cannot be drawn, the source shows with the reason: the line that could not be read
  (第 {n} 行看不懂), or for a diagram past 150 nodes 图太大了，只显示原文 with its node count.
  Artboard: Refs.
- **产出 media**: inline in the 话题 and full in the 产出 pane: live HTML pages, Markdown, images,
  video (poster, chapters, frames to send into the 话题), audio (waveform, key points, transcript),
  PDF, code and tables; upload and preview states. Previews are made by the sender's computer.
  An inline page starts at its top and fades out at the bottom edge when more lies below; 展开
  opens all of it. Artboard: Media.
- **Settings**: 外观 (theme: follow the system, light or dark; 侧栏样式), my profile (name, 岗位,
  avatar), 成员 (invite, change level, deactivate, add a bot member and its 岗位, 机器 and
  credential), 机器 (connected machines and their health), 通知 (mute a 群, follow topics),
  diagnostics, sign out, and a developer-view switch.
- **Menu bar item**: the app can stay in the menu bar and keep its connection.

## Behavior

- Starting to write in a 群 without picking a topic starts a new topic; its title defaults to
  the first line of the message.
- Long content goes into 产出; the topic keeps the conclusion and a link.
- Notifications are quiet by default: only when I am @mentioned, when a topic waits on me
  (需要你拍板), or when a topic I follow gets a result (做完了, status 完成 or a new 产出).
- Presence, typing, read positions and unread counts sync in real time. Read positions are per
  topic: reading one topic does not mark the other topics of the 群 read.
- @mentioning a bot asks it to take the work. It acknowledges with a reaction, works quietly
  (progress shows in its plan and in 实时查看, not as messages in the topic), and comes back only
  with a result card with proof, a decision it needs, or when it is stuck. One responsible member
  per topic.
- Cards are declarative JSON. Card v1 (status, fields, table, progress, list, code, image,
  actions) still works. Card v2 adds intents (ask, done, working, info), typed blocks and states
  (pending, waiting on someone, sent, answered, executing, done, failed, expired, superseded).
  Card v2.1 adds containers, choice forms, `buttons`, `diagram` and `embed`. Bots and integrations
  post them; every interaction (not only answers) goes back to the card's author, which may change
  the card in place. Only people send interactions; a bot's message never starts a run. Clients
  before 1.1.0 see a v1 card that the server derives and refreshes on every change.
- Permissions: levels `owner` (Joye and the bots at Joye's level) and `member` (interns and
  their bots). Clients and bots use the same API. Every write is audited with who asked, who
  did it, which credential, and what changed.
- Sign-in for V1 is an invite code that becomes a token in the macOS Keychain. Email or OAuth
  sign-in is later.
- Distribution: a DMG signed with the team's own certificate (not notarized). GitHub Releases keeps every build; the server also serves the
  latest DMG to holders of an invite code and to members, and the app updates itself from it.

## Bot members (first roster)

Every bot's look is a character from Joye's jojo-friends family (his private personal IP,
`/Users/joye/Documents/code/jojo-friends`; use it only inside this private product, never publish
the source or the art elsewhere). A bot's avatar shows its live state with the character's
status signal: idle, working, needs-input, success, error, offline. The main bot is Jojo. The
other bots and their duties are copied from the team's Notion roster (团队名册) for now; Joye
will adjust them. The roster lives in [`docs/ROSTER.md`](ROSTER.md) and `assets/bots/roster.json` (named
ROSTER, not BOTS, because macOS file names ignore case and `docs/bots.md` exists).

| Bot | Character | 岗位 | Level | Reports to | Duties (from Notion) |
| --- | --- | --- | --- | --- | --- |
| Jojo | `jojo` Jojo | 主助手 | owner | Joye | The team's main bot: the default entry point, routes work to the right member, does general work; runs on the Mac mini |
| Tata | `o03` 塔塔 | 产品负责人 | member | Joye | qqbot lead and product; jiajia.fun and share.jiajia.fun; the growth group (data flywheel); R2 buckets |
| Pocket | `o05` 兜兜 | 幕僚长 | member | Joye | Joye's single entry point (mail, calendar, todos, reminders, morning brief), routes to the right bot; keeps the roster; decision and merge cards; blog, GitHub and personal brand |
| Forge | `o08` 薇薇 | 开发 | member | Tata | qqbot development: issues, coding runs, PR review, test debt, CI failures |
| Watch | `o11` 蒂奇 | 运维 | member | Tata | qqbot operations: alerts, staging checks, incident reviews, cloud moves, secret and permission checks; asks Joye before production |
| Scout | `o07` 点点 | 测试验收 | member | Tata | qqbot QA: a daily read-only patrol of staging; checks each batch on staging before production |
| Ledger | `h02` 便当 | 数据 | member | Tata | qqbot data: reports, tracking, the knowledge base and review queue, evals, cost |
| Taro | `h03` 芋圆 | 社媒发布 | member | Pocket | Social posts: rewrites Joye's blog, projects and views and schedules them on X, the blog, Bilibili, Xiaohongshu and Douyin; Joye confirms each post first |

## V1: production use by 5 people and 5 bots

V1 is not an MVP. The team works in it every day, stably. Done when all of these hold:

1. **Chat**: groups, DMs, topics with status and `waitingOn`, @mentions, quotes, edit and
   delete, per-topic unread and read positions, presence, typing, 待办, quiet notifications.
   No lost or duplicated messages across reconnects, sleep and network changes.
2. **Performance** (same region): p95 delivery under 300 ms; a 10,000-message topic opens in
   under 100 ms from the cache and scrolls at 60 fps; cold start to usable under 1 s; catch-up
   after a day offline under 2 s; search under 300 ms.
3. **Bots**: external bots join with a bot token and the TypeScript bot SDK. Runner-backed bots
   run coding CLIs (Codex, Claude Code) through the machine daemon on the Mac mini: they
   acknowledge once, stream progress, post results, take follow-ups in the topic, and stalls are
   detected. Every bot run is captured (normalized events plus the CLI's raw session in R2) and
   visible in the process view.
4. **产出**: files and images up to 100 MB, inline image previews, HTML pages in the isolated
   window with versions.
5. **Search**: Chinese two-character words work, inside one 群 and across all of mine.
6. **Integrations**: an incoming webhook API and a GitHub adapter that turns PR, review and CI
   events into cards in a topic.
7. **Operations**: separate staging and production environments; Workers logs with secrets
   redacted; a monitor that checks health and a real message round trip every 5 minutes and
   posts alerts into an ops 群; a daily R2 archive of every group log and of D1; a restore tool
   and runbook that were tested.
8. **Distribution**: a DMG on GitHub Releases and on the server; in-app update; Chinese install
   and onboarding guides for interns.
9. **Security**: hashed tokens and invite codes, authorization on every frame and route,
   sandboxed 产出, rate limits, audited admin actions, webhook signatures checked.
10. **Design**: first-class visual design, on par with the best macOS apps, in light and dark
    mode: every control from gpui-component, styled per docs/DESIGN.md, and checked with Emil
    Kowalski's skills (docs/design/skills/emilkowalski): emil-design-eng, review-animations,
    break-ui and apple-design.

Later, not V1: decisions as first-class objects and typed backlinks, catch-up summaries, task
graphs, semantic search, cost tracking (the 成本 page may say 即将推出), Cloudflare Containers
runners, email or OAuth sign-in, mobile, Windows and Linux.
