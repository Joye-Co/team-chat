# Product brief

The short version of the design doc ([snapshot](design/design-doc.zh.md)) that code work needs.
When this file and the snapshot disagree, this file wins for V1.

## What it is

A work chat for one person or one small team (an owner, up to about 10 people, and a few AI
members), with one implementation for both. People and AI members (bots) are equal members.
Development, product and other work are discussed, assigned and delivered here, so the team
stops using Feishu and Slack. Each 群 is a chat first (its 群聊); a topic opens from it when a
piece of work needs its own thread, like a Feishu topic in a group chat.

- **Clients**: Mac and Windows, the same app with the same features ([design/WINDOWS.md](design/WINDOWS.md)).
- **Server**: every team runs its own (Cloudflare Workers), also one person alone; there is no
  hosted service ([SELF-HOSTING.md](SELF-HOSTING.md)). The person who sets it up is its first 所有者.
- **Computers** (电脑, where AI members work): Mac, Windows and Linux; a Linux host can run each
  run in its own VM. One person uses the computer the app runs on (用这台电脑) or adds another;
  a team uses the team's computers, for example one Linux host, and anyone may add their own.
- **How AI members work**: see "AI members at work" below. The plan that finishes this
  (the usable milestone) is [plans/usable.zh.md](plans/usable.zh.md).

## Interface words

The interface is in Simplified Chinese. It uses only these four nouns for product concepts:

| UI word | Code name | Means | Merges these developer words |
| --- | --- | --- | --- |
| 成员 | `Member` | A person or a bot. Shows name, avatar and 岗位 (role). A bot's avatar is its character (or a monogram) with its live status signal, and its 岗位 tag carries a small bot glyph; no other mark. | agent, bot, teammate |
| 群 | `Group` | Where members talk. A DM is a 群 of two people. | chat, channel, DM |
| 话题 | `Topic` | One piece of work: discussion, progress, questions and 产出 hang under it. | thread, task, ticket, session |
| 产出 | `Artifact`, `File` | What a topic produces: pages (HTML), files, decisions. | artifact, attachment |

- Topic status, `waitingOn` and the members working on it follow the work (runs, cards, results);
  nobody sets them by hand. The topic menu only has 重命名 and 关注. Bots are
  members: there is no 暂停 for a member; you tell it in the topic, 暂停 holds one run, and 停止 ends one run.
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
- Words for memory (沉淀, docs/design/agent-harness.zh.md §7.7 and §13.5): 记得的事 (an AI member's),
  团队约定, 群里的约定, 我的偏好, and 我的约定 (团队约定 and 我的偏好 on one board when one person uses
  the server); the kinds 约定, 事实, 决定, 偏好, 经验, 做法; 记忆更新, 记一条, 明天生效, 停用, 恢复, 撤回,
  可能过时; and AI 服务 (the server's own model, set in 团队管理).
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
- Words for several AI members in one 话题 and for work that goes from member to member
  (docs/plans/usable/design-relay.zh.md §3.6). Never 接力 as a noun: an order a person writes
  (`@A → @B → @C`) is 按顺序做, and its steps use the closed set of status words above plus 停止:
  按顺序做：{names}, 第 {k}/{n} 步, 停在 {name}：{reason}, 都做完了, {name} 停止了, 停止了, 重试, 停止,
  没能停止, 没能重试; {a} 把这件事转交给 {b}, {a} 把一部分分给 {b}, 全部停止, 停止这件事里所有在做的？,
  做完的标准, {b} 做完了，结果已交回 {a}, {b} 没做完：{reason}, 被 {name} 停止; the reasons
  {name} 现在接不了这件事, 等 {name} 手上的做完, 交出次数到上限了, {name} 的代码还没推上去,
  发起的人已经不在这个群, 没有交到 {name} 手上, 停住了; the composer's 想让他们按顺序做？用 → 连起来,
  按顺序做要在话题里用, 这个话题已经在按顺序做，先停止它, 按顺序做要写 2 到 6 步，同一个成员不能挨着;
  and in 详情 → 成员 在做 and 参与过. Several members can work in a 话题 at once or one after
  another; no member is the one responsible for a 话题.
- The settings window may also use 岗位 (role and how the member works), 权限 (permissions),
  机器 (machines) and 成本 (cost).
- Words for each person's GitHub (docs/plans/usable/design-github-identity.zh.md §3.7): GitHub is
  a proper name, not a product noun; 连接 GitHub, 连接…, 重新连接…, 断开…, 已连接为 @{login}, 还没连接,
  需要重新连接, 验证码 (GitHub's own word), 复制验证码, 打开 GitHub, 授权, 组织, 仓库, PR, 以后再说,
  去 GitHub 撤销, and {name} 在等你连接 GitHub.
- Words for a run's state and 接管 (docs/plans/usable/design-takeover.zh.md §3.9): 暂停, 继续, 已暂停,
  已停止, 没做完, 在等电脑, 排队第 {n} 个, 电脑不在线, 正在准备, 等 {who} 回复, 等 {who} 批准,
  卡住了，需要你看一下, {who} 暂停了 {member}, 你在操作 {computer}, {who} 在操作 {computer}, 已停下,
  替 {who} 交还, 去回复, 运行, 中止, 运行中, 成功, 失败, 已中止, 工作目录, 在访达中打开 / 在资源管理器中打开,
  留句话给 {member}, {who} 接管了 {member}, {who} 交还了 {member}, 跑了 {k} 条命令, 改了 {files}, and the
  reminders {member} 还停着，等你交还 and {who} 接管 {member} 已经 {h} 小时. 暂停 stops one run for a
  while (继续 lets it go on); it is not a setting of the member. 暂停, 等你 and 完成 never share words.
- The three 身份 of a team (design-roles): 所有者, 管理员 and 普通成员, with 身份 itself and the menu
  items 设为所有者, 设为管理员, 设为普通成员. 负责人 stays the member a 话题 is assigned to and is
  never a 身份.
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
- **首页**: the main bot's one-line priority for today (only when it wrote one), 需要你拍板 (every card and
  topic that waits on me, answerable in place), 做完了 today with proof, and a column 正在做 with
  each active run, its plan progress and, when it uses its computer, a live thumbnail; 排队 when
  runs are queued.
- **话题**: the timeline (virtualized, anchored at the bottom, Markdown, images inline, cards
  rendered natively), the newest decision that waits on me pinned above the composer, and the
  composer (multi-line, Chinese IME, @mentions, quote a message, paste or drop images and files).
  On the right, the topic rail (详情): 成员 (everyone in the 群, who all see the 话题, with the AI
  members at work marked 在做, those that took part 参与过, and the one who 提出 it), 计划, 改动, 产出, 用到的决定, 电脑, 用时和成本; a card
  with no data is hidden.
- **单独问** (⌘;): a side chat with a member (the main bot by default) beside a topic. The member has read
  the topic up to now. Only I see the questions and answers. A suggested step goes into the topic
  only when I press its button.
- **实时查看 + 接管**: a member's computer beside the topic while it works: its terminal rebuilt from
  the run's steps, the plan, 验收标准, and 回放 by step. Each run shows one state (see "AI members at
  work"). 暂停 holds the run and 继续 lets it go on; 接管 holds it for me and opens a command box in
  the run's 工作目录 (when its computer supports take-over); 交还 lets it continue from that step
  with a summary of what I did; 停止 cancels the run.
- **⌘K 交代或搜索**: one field to hand work to a member (交给 <member> starts a topic addressed to
  it in the chosen 群; ⌘↵ hands it to the main bot) or to find 话题, 产出 and 成员.
- **群**: opening a 群 (or a 私聊) shows its 群聊: one chat timeline with the composer at the
  bottom, where plain messages stay and @mentioning a member hands it work whose result comes back
  there. The title bar has 详情 and 话题 (Joye, 2026-11-08); a message's menu has 开个话题, which
  starts a 话题 whose first message quotes it. 详情 opens 群资料 beside the chat: 成员 first (who is
  in this 群, 添加成员…), then 通知, 群里的决定 (recorded from answered decisions, or added with
  记一条), 最近的产出 and the 电脑 in use. 话题 opens the 群 page, which shows only 话题: the
  topics 进行中 with live previews, 最近完成, 新话题 and 回到群里聊.
- **成员**: every member with filters (全部, 在做事, 等人); a member's page shows 现在, 这周 (topics,
  产出, cost), 最近交付, 需要批准的事, 能用的工具, 电脑 and 最近的问题. Changing a member opens the settings
  forms.
- **常用设置**: an admin changes what changes most about an AI member, which computer it works on
  (one of the team's or any free one), without opening settings: from the menu of its messages
  (also a right-click on its avatar), from 在哪干活 › 更换电脑… on its page and from ⋯ in 团队管理
  (which also has 编辑…). A pick saves at once; 更多设置… opens the full edit sheet. Models and
  思考强度 are set once for the team, not here.
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
  avatar), 成员, 电脑 (我的电脑, 团队的电脑 and 同事的电脑: 用这台电脑, 添加电脑, rename, remove,
  不再用这台电脑, and on my own computer which AI members do my work there), 通知 (mute a 群,
  follow topics), diagnostics, sign out, and a developer-view switch.
- **团队管理** (a window for admins, from Settings or the app menu): members (invite, 身份,
  deactivate, 添加 AI 成员 and its 岗位, 运行方式 and computer), usage, activity, the team's 电脑
  (health, and the VMs of a Linux host), 账号与密钥 (subscription sign-ins of each computer and API
  keys), and 团队设置: AI 服务, the team's models, 默认规则 (approvals and spend) and 团队记忆.
  Members cannot open it; they read the team settings they need where they use them.
- **Menu bar item**: the app can stay in the menu bar and keep its connection.

## Behavior

- Writing in a 群 (or a 私聊) posts into its 群聊 and does not start a topic. A topic starts only
  on purpose: 新话题, or 开个话题 on a 群聊 message (its first message quotes that message); its
  title defaults to the first line. The 群聊 has no status, owner or 关注; an @mentioned member
  works and answers in it, each mention to the member it names. The sidebar's 群 header opens the
  群聊 (bold with a dot while it has unread messages) and its 话题 list under it.
- Long content goes into 产出; the topic keeps the conclusion and a link.
- Notifications are quiet by default: only when I am @mentioned, when a topic waits on me
  (需要你拍板), or when a topic I follow gets a result (做完了, status 完成 or a new 产出).
- Presence, typing, read positions and unread counts sync in real time. Read positions are per
  topic: reading one topic does not mark the other topics of the 群 read.
- @mentioning a bot asks it to take the work. It acknowledges with a reaction, works quietly
  (progress shows in its plan and in 实时查看, not as messages in the topic), and comes back only
  with a result card with proof, a decision it needs, or when it is stuck. Several members can
  work in one topic, at once or one after the other: each @mention goes to the member it names,
  and a follow-up without an @ goes to the member that most recently started work there.
- Cards are declarative JSON. Card v1 (status, fields, table, progress, list, code, image,
  actions) still works. Card v2 adds intents (ask, done, working, info), typed blocks and states
  (pending, waiting on someone, sent, answered, executing, done, failed, expired, superseded).
  Card v2.1 adds containers, choice forms, `buttons`, `diagram` and `embed`. Bots and integrations
  post them; every interaction (not only answers) goes back to the card's author, which may change
  the card in place. Only people send interactions; a bot's message never starts a run. Clients
  before 1.1.0 see a v1 card that the server derives and refreshes on every change.
- Permissions: three roles. 所有者 (team owner: `level` `owner` with `teamOwner`, people only),
  管理员 (admin: `level` `owner`, people and the AI members at that level) and 普通成员 (member:
  `level` `member`, everyone else and their AI members). A 所有者 is also an admin. Admins manage
  the team: the AI service key and model, the team's models, approval rules, invites, AI members,
  team machines, usage and activity; members read the team settings. Only a 所有者 changes
  anyone's role and deactivates, revokes or edits an admin or a 所有者. Invited people arrive as
  members and get a role afterwards. The team keeps at least one active 所有者; the person who
  sets up a server is its first 所有者, so someone using it alone notices no roles at all
  (see [plans/usable/design-roles.zh.md](plans/usable/design-roles.zh.md)). Clients and bots use
  the same API. Every write is audited with who asked, who did it, which credential, and what
  changed.
- Sign-in for V1 is an invite code (the first 所有者 gets one from the setup script) that becomes
  a token in the macOS Keychain or the Windows Credential Manager. After that a member can add a
  sign-in email in 账号 and sign in on any computer with a 6-digit code sent there.
  The app keeps one session per server (正式服务器, 测试服务器 or another), so switching servers
  from the app menu does not sign out. OAuth sign-in is later.
- Distribution: a DMG signed with the team's own certificate (not notarized) and a Windows
  installer. GitHub Releases keeps every build; the server also serves the latest one to holders
  of an invite code and to members, and the app updates itself from it.

## AI members at work

What is built on main; [plans/usable.zh.md](plans/usable.zh.md) tracks what is left and how it is
checked on real computers, and [design/agent-harness.zh.md](design/agent-harness.zh.md) is the
design it came from.

- **Front desk and executor**: an @mention, a DM or a follow-up first reaches the AI member's
  front desk on the server. With an AI 服务 (a model key the admins set in 团队管理) it makes one of
  four decisions: answer at once, hand the work to the executor with a written brief, ask one
  question (a card; the topic waits on the answer), or stay silent. Without one, every request
  goes to the executor, and a plain 停 from the requester stops the run. The executor is a
  coding CLI (Claude Code, Codex, or an agent over ACP) that the daemon starts on a computer. It reads the
  topic, 产出, cards, decisions, members and memory, and posts cards, 产出 and reports, with the
  `chat-run` command the daemon puts on its PATH (not an MCP server). The CLI runs with full
  permissions; only the gated actions below stop it.
- **Continuing**: a member's runs in one topic share one CLI session, one working folder and one
  branch, so a follow-up continues where the last run stopped; a DM continues its last session
  until the person says 新任务 (or the front desk finds the request is new work). A run that
  starts again (a restart, a daemon restart, a move to another computer) gets a recovery note
  with what it already did outside the computer.
- **Watching the work** (盯活) is done by code, not by a model: one nudge after 5 silent minutes, a
  reminder for a step repeated 4 times, a retry in the same session, another computer last; at
  most 3 fixes, then one note in the topic. A computer that drops off is waited for (30 minutes
  before another one takes over; a team with one computer waits on). At the team's time limit
  (default 2 hours) the run saves its state and goes on in a new turn; two turns in a row without
  progress stop it. A run that hits a subscription limit waits until the limit resets.
- **Results are checked**: a run that reports blocked or waiting does not mark the 话题 完成; the
  result card's numbers come from the run log; a failure says in one sentence why (not signed in,
  limit reached, model not available, network, CLI not installed) and what to do.
- **Approvals** (要批准的动作): the team checks a few kinds of actions in 默认规则 (merging to main,
  deploying to production, changing production config, deleting data, publishing). The daemon
  stops them at the step that would run them (Claude Code's hook, Codex's and ACP's permission
  requests, and shims for `git`, `gh` and other tools first on the run's PATH) and the approver
  gets a card; 这件事里都同意 covers the rest of that piece of work. This guards against mistakes;
  branch protection and production approvals stay the real limit.
- **Several AI members**: each @mention starts its own run; an AI member may hand the whole piece
  of work to another (转交) or give a part to another in its own 话题 (分出去), whose result comes
  back; a person may write an order, `@A → @B → @C` (按顺序做), where each step starts from the
  results of the step before. 全部停止 stops every run of that piece of work. No member is
  responsible for a 话题 alone.
- **Run states and 接管**: every run shows one state: 正在做, waiting for a person (a question, an
  approval, a reply), 在等电脑 (queued, offline, starting, a limit), 已暂停, taken over, 做完了,
  失败 or 已停止. The server works it out for every run (`BotRun.phase`, with `holdKind`,
  `takeover` and `lastHandback`); the app works it out from `status` and `pausedBy` only for an
  older server. 暂停, 继续, 接管 and 交还 move between them (`POST /v1/runs/:id/pause`, `/resume`,
  `/takeover`; the commands of 接管 go through `/v1/runs/:id/commands`); who may do which is in
  [plans/usable/design-takeover.zh.md](plans/usable/design-takeover.zh.md). The command box of 接管
  needs the computer's daemon to support it (feature `takeover`).
- **Memory** (沉淀): four layers, the team's, a 群's, an AI member's and a person's own. What was
  learned in a 群 is read only by people in that 群; a person's own only by that person. AI members
  note what they learn during the day; once a night in the team's time zone the AI 服务 sorts the
  day's notes, with no card to confirm, and 记忆更新 lists each change with 撤回. Without an AI 服务
  there is no sorting and no lessons after a run; the memory screens say so.
- **Computers**: each computer is the team's or one person's. Admins add and manage the team's;
  anyone adds their own (用这台电脑 makes this computer one's own), and a personal computer only
  does the work its owner asks for. CLI sign-ins stay on the computer, and the app seals each
  API key for the computer that uses it, so the server never sees it in clear text.

## Bot members

Bots are members like people: same API, same 群 and 话题, a name, avatar and 岗位. Which bots a
team runs, and what each one does, is the operator's choice, not part of the product; add them
as in [bots.md](bots.md). The main bot is the active bot named Jojo, else the oldest active bot
(see [protocol.md](protocol.md)): it gets the first DM of a new member and is the default for
单独问 and ⌘↵.

A bot's avatar is its character from Jojo & Friends (26 characters, art CC BY 4.0, vendored in
`apps/desktop/assets/characters/`); a new bot gets one no other member uses. The avatar shows
its live state in two channels: the status (idle, working, needs-input, success, error,
offline) and the face, one of 16 emotions that tells the case apart (thinking before it
writes, focused while it writes, shy when its work waits for review, celebrating a finished
topic, aggrieved when stuck in several places). A bot whose character is cleared shows a
monogram.

## V1: production use by 5 people and 5 bots

V1 is not an MVP. The team works in it every day, stably. Done when all of these hold:

1. **Chat**: groups, DMs, topics with status and `waitingOn`, @mentions, quotes, edit and
   delete, per-topic unread and read positions, presence, typing, 待办, quiet notifications.
   No lost or duplicated messages across reconnects, sleep and network changes.
2. **Performance** (same region): p95 delivery under 300 ms; a 10,000-message topic opens in
   under 100 ms from the cache and scrolls at 60 fps; cold start to usable under 1 s; catch-up
   after a day offline under 2 s; search under 300 ms. `scripts/staging-smoke.ts` measures
   delivery, search, catch-up, reconnects, mention notifications and the 100 MB upload; the
   rest is not measured yet (see docs/RUNBOOK.md, Quality smoke).
3. **Bots**: external bots join with a bot token and the TypeScript bot SDK. Runner-backed bots
   run coding CLIs (Codex, Claude Code, ACP agents) through the machine daemon on a computer: they
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
   and onboarding guides for new members.
9. **Security**: hashed tokens and invite codes, authorization on every frame and route,
   sandboxed 产出, rate limits, audited admin actions, webhook signatures checked.
10. **Design**: first-class visual design, on par with the best macOS apps, in light and dark
    mode: every control from gpui-component, styled per docs/DESIGN.md, and checked with Emil
    Kowalski's skills (docs/design/skills/emilkowalski): emil-design-eng, review-animations,
    break-ui and apple-design.

Later, not V1: decisions as first-class objects and typed backlinks, catch-up summaries, task
graphs, semantic search, cost tracking (the 成本 page may say 即将推出), Cloudflare Containers
runners, OAuth sign-in, mobile, a Linux app.
