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
| 成员 | `Member` | A person or a bot. Shows name, avatar and 岗位 (role). A bot's avatar has a small mark. | agent, bot, teammate |
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

- **Sidebar**: 待办 at the top (topics waiting on me and unread mentions), then one list of 群
  sorted by last activity with unread and mention badges. DMs show the other member's avatar
  and presence dot. A search field and a quick switcher (Cmd+K).
- **Topic list** of the selected 群: title, status chip, last activity, unread count per topic.
  A button starts a new topic.
- **Topic view**: the timeline (virtualized, anchored at the bottom, Markdown, images inline,
  cards rendered natively, streaming bot replies) and the composer (multi-line, Chinese IME,
  @mentions, quote a message, paste or drop images and files). A side panel shows the topic's
  产出 and the process view of a bot's work.
- **产出 window**: HTML pages open in a separate native window (WKWebView, one origin per 产出,
  strict CSP, no network) with a version list. Files open with the default macOS app.
- **Settings**: my profile (name, 岗位, avatar), 成员 (invite, change level, deactivate, add a
  bot member and its 岗位, 机器 and credential), 机器 (connected machines and their health),
  通知 (mute a 群, follow topics), diagnostics, sign out, and a developer-view switch.
- **Menu bar item**: the app can stay in the menu bar and keep its connection.

## Behavior

- Starting to write in a 群 without picking a topic starts a new topic; its title defaults to
  the first line of the message.
- Long content goes into 产出; the topic keeps the conclusion and a link.
- Notifications are quiet by default: only when I am @mentioned, when a topic waits on me, or
  when a topic I follow gets a result (status 完成 or a new 产出).
- Presence, typing, read positions and unread counts sync in real time. Read positions are per
  topic: reading one topic does not mark the other topics of the 群 read.
- @mentioning a bot asks it to take the work. It acknowledges once, works quietly, and comes
  back only with a result, a decision it needs, or when it is stuck. One responsible member per
  topic. Everyone sees the same streaming reply.
- Cards are declarative JSON (status, fields, table, progress, list, code, image, actions). Bots
  and integrations post them; buttons send the action back to the card's author.
- Permissions: levels `owner` (Joye and the bots at Joye's level) and `member` (interns and
  their bots). Clients and bots use the same API. Every write is audited with who asked, who
  did it, which credential, and what changed.
- Sign-in for V1 is an invite code that becomes a token in the macOS Keychain. Email or OAuth
  sign-in is later.
- Distribution: an unsigned DMG. GitHub Releases keeps every build; the server also serves the
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
