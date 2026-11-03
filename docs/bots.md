# Bots

[中文](#机器人中文) below.

An external bot is a member with `kind: "bot"`. It uses the same API as the desktop app: a
WebSocket for events and writes, and HTTP for files, 产出 and one-shot posts. The TypeScript
SDK is `packages/bot-sdk` (`@chat/bot-sdk`); it runs on Bun and on Node 22+.

## 1. Create a bot member and get its token file

Only an owner can create a bot. The team's bots, their characters and the exact commands
that create them are in [ROSTER.md](ROSTER.md); `--character <id>` gives a bot its
jojo-friends look. With the owner token saved by `admin.ts bootstrap` (in
`~/.config/team-chat/<env>/owner-token`):

```sh
bun server/scripts/admin.ts bot --env local --name "Deploy" --role "deploys and CI"
# bot 01J... (Deploy)
# bot token: /Users/<you>/.config/team-chat/local/bot-deploy-01J....token
```

The script prints only the path. The token file has mode 600. The token (`tc_...`) does not
expire: keep it out of logs, chats and git. To revoke it:
`bun server/scripts/admin.ts revoke --env local --member <botId>`.

A new bot joins the default groups (the bootstrap group "全员"). Add it to other groups
with `POST /v1/groups/:id/members` `{ "memberId": "<botId>" }` (any member of that group), or
in the desktop settings. Its level is `member`.

## 2. Run a bot

```sh
bun install            # once, at the repo root
CHAT_URL=http://127.0.0.1:8789 \
CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-deploy-01J....token \
bun examples/echo-bot.ts          # or: node examples/echo-bot.ts
```

Mention the bot in any topic of a group it is in: it replies `echo: <text>`. Write
`@Deploy stream ...` to see a streaming reply.

A minimal bot:

```ts
import { Bot } from "@chat/bot-sdk";   // in this repo: packages/bot-sdk/src/index.ts

const bot = new Bot({
  url: process.env.CHAT_URL!,
  tokenFile: process.env.CHAT_TOKEN_FILE!,
  stateFile: "/var/lib/deploy-bot/state.json", // cursors and followed topics
});

bot.onMention(async (ctx) => {
  await ctx.updateTopic({ status: "in_progress", ownerId: bot.me.id });   // take the work
  const w = await ctx.stream("Working on it...\n");
  w.append("step 1 done\n");
  await w.finish();                                                       // final edit
  await ctx.updateTopic({ status: "waiting_review", waitingOn: [ctx.message.authorId] });
});

bot.onMessage(async (ctx) => { /* a message in a topic I follow */ });
bot.onCardAction(async (ctx) => {
  if (ctx.actionId === "approve") await ctx.updateCard({ v: 1, title: "Approved", blocks: [] });
});

await bot.start();
```

## 3. What the SDK does

- **Connection**: `hello` on every connection, ping every 30 s, reconnect with backoff (0.5 s to
  30 s, with jitter). A rejected token stops the bot with `ChatError` code `unauthorized`.
- **Ordered events**: events are applied per group strictly in `seq` order. A gap is filled
  with `fetch`. After a restart, the bot reads every event after its saved cursor
  (`stateFile`) and runs handlers with `ctx.catchUp = true`.
- **First start**: with no state file, the bot skips old history (`firstStart: "latest"`); pass
  `firstStart: "all"` to handle it. Groups the bot joins later are read from their start.
- **Cursors are saved after handlers finish**, so a crash runs a handler again. Replies from a
  handler context (`ctx.reply`, `ctx.stream`) use a clientMsgId made from the message id, so the
  server answers the retry with the first reply (`duplicate: true`) and nothing is posted twice.
  Effects outside the chat (deploys, API calls) must be idempotent too.
- **Handlers**: `onMention` (a message that mentions me), `onMessage` (any other message in a
  topic I follow), `onCardAction` (a press on one of my cards), `onCardAnswer` (1.1: an answer
  to one of my v2 cards, posted with `postCardV2`; close it with
  `ctx.resolve`), `onEvent` (every event),
  `onTransient` (typing, presence, deltas). A message runs at most one of `onMention` and
  `onMessage`. My own messages and system messages run neither. Messages of other bots run
  neither either, so two bots cannot answer each other without end; `allowBotAuthors: true`
  turns this off. Handlers run concurrently; errors go to `onError`.
- **Follow rules**: the bot follows a topic it created, posted in, was mentioned in, was added
  to `waitingOn` of, or became owner of, as the server does. `bot.setFollow(...)` changes it.
- **Writes** wait for the connection and are resent after a reconnect (`send` is idempotent
  by `clientMsgId`). Rate-limited writes are retried. A request fails with `timeout` after
  `requestTimeoutMs` (60 s). Frames are checked against the protocol schemas before sending.
- **Streaming**: `send` with `streaming: true`, `delta` frames batched every 100 ms, and a final
  `edit` with the full body and `streaming: false`. After a reconnect the writer sends the full
  text again so readers miss nothing.

| Method | Does |
| --- | --- |
| `send(target, input)` / `startTopic(groupId, input)` / `reply(message, input)` | post; no `topicId` starts a topic |
| `stream(target, input)` → `StreamWriter` (`append`, `finish`) | streaming reply |
| `edit`, `deleteMessage` | change my messages |
| `updateTopic(groupId, topicId, { status, waitingOn, ownerId, title })` | topic status and who it waits on |
| `postCard(target, card)` / `updateCard(groupId, messageId, card)` | cards (V1 extension) |
| `uploadFile({ groupId, name, mime, data })` → `FileRef` | for `attachments` (V1 extension) |
| `http.publishHtml(...)`, `http.createArtifact`, `http.addArtifactVersion` | 产出 (V1 extension) |
| `http.postMessage(groupId, input)` | one HTTP send (V1 extension) |
| `history`, `typing`, `setFollow`, `pressCard` | the other frames |
| `setPlan(target, plan)` / `ctx.setPlan(plan)` | the plan of my current run in a topic (below) |
| `endRun(target, status, summary?)` / `ctx.endRun(status, summary?)` | end my current run |
| `postCardV2(target, card)` / `updateCardV2(groupId, messageId, card)` / `ctx.ask(card)` | Card v2 (1.1, below) |
| `answerCard(groupId, { messageId, rev, actionId, inputs?, editedValues? })` | answer another member's v2 card |
| `ack(groupId, messageId, emoji?)` / `ctx.ack()` | react (default 👀) instead of an "on it" line |
| `done(target, card)` / `ctx.done(card)` | post the done card and end my run |
| `setAcceptance(target, items)` / `ctx.setAcceptance(items)` | the 验收标准 of my current run |
| `http.publishHtml({ ..., events, runId })` | an interactive 产出 whose page sends events back |

Cards, files, 产出, `topic.follow`, `waitingOn` and HTTP send are V1 protocol extensions
(docs/protocol.md). On a server build without them the call fails with the server's error.

### Show what the bot does next: the run plan

A run plan is the bot's ordered to-do list. Members see it at the top of the topic's side panel
(`{bot} 的进度`): done items with a check, the current one, items that wait for a member with
an eye, later ones with a dashed circle. `setPlan` starts a run of the bot in the topic when it
has none; every call replaces the whole plan (`[]` clears it). At most 30 items, each 1..200
characters, state `done`, `current`, `waiting` or `pending`.

```ts
bot.onMention(async (ctx) => {
  await ctx.setPlan([
    { text: "写标题和正文", state: "current" },
    { text: "生成 3 张配图", state: "pending" },
    { text: "等 Joye 确认", state: "pending" },
    { text: "周四 20:00 发出", state: "pending" },
  ]);
  // ... write the draft ...
  await ctx.setPlan([
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "done" },
    { text: "等 Joye 确认", state: "waiting" },
    { text: "周四 20:00 发出", state: "pending" },
  ]);
  await ctx.updateTopic({ status: "waiting_review", waitingOn: [ctx.message.authorId], ownerId: bot.me.id });
});
// Later, when the post is out:
await bot.endRun({ groupId, topicId }, "done", "已发出");
```

Runner-backed bots get `conflict` from `setPlan`: their runner sets the plan from the coding
CLI's own to-do list (Claude Code's TodoWrite, Codex plan updates).

## 3a. 1.1: Card v2, answers, side chats, page events

These need a 1.1 server. `examples/ask-bot.ts` uses all of them.

**The output contract.** Say you saw a request with a reaction, not a text line
(`ctx.ack()`, 👀). Keep progress in the plan (`ctx.setPlan`), not in a streaming message:
`stream()` still works but is deprecated for work reports. End with one result card,
`ctx.done(...)`, which posts a Card v2 with intent `done` and ends the run (`failed: true` ends
it as failed; the card then starts with a `fail` evidence block `failed`, label `failedLabel`,
default 没有完成, and its v1 card for 1.0.x clients has tone `danger`):

```ts
await ctx.done({
  title: "做完了",
  summary: "登录页改好了,测试通过",            // text block, fallback and run summary
  evidence: [{ label: "测试 128/128 通过", state: "pass" }, { label: "截图", fileId }],
  refs: [{ label: "PR #42", target: { kind: "pr", url: "https://github.com/o/r/pull/42", number: 42 } }],
  metrics: [{ label: "首屏", value: 1.2, unit: "s", delta: -0.4, better: "down" }],
});
```

**Builders.** `askCard`, `decisionCard`, `approvalCard`, `draftCard`, `formCard` and `doneCard`
make valid v2 cards and throw `ChatError` `invalid` when a card does not fit the protocol
(docs/protocol.md, "Cards v2"). Ask builders take `from` (member ids, `"requester"`, `"owners"`:
who may answer), `expiresAt`, `record` (the answer is a group decision, 群里的决定) and
`irreversible` (`{ verb, target, consequence, typeToConfirm? }`). Their ids are fixed, in
`CARD_IDS`: the decision block is `choice`, the draft block `draft`, the actions `submit`,
`approve`, `deny` and `cancel`. Free-text options and input choices get ids `o1`, `o2`, ...
The server derives a v1 card for 1.0.x clients; an approval (no inputs) keeps its buttons there.

**Answers.** `onCardAnswer` gets every `card.answered` on my v2 cards, also after a restart
(the SDK remembers its card messages and loads a card it no longer has in memory from the
history before the handler runs, so `ctx.card`, `ctx.draft()` and `ctx.resolve()` work). A press on a v2 card by an old client arrives here too,
with no inputs. Type the inputs with a generic and read them with the helpers:

```ts
bot.onCardAnswer<{ choice: string; count: number }>(async (ctx) => {
  if (ctx.cancelled) return;                     // a cancel action ("deny", "cancel")
  ctx.choice();                                  // first chosen option of block "choice"
  ctx.number("count"); ctx.text("note"); ctx.bool("notify"); ctx.draft();   // inputs by block id
  await ctx.markAnswered();                      // edit the card: rev + 1, no buttons
});
```

The values come from the client: check them. `ctx.card` is the newest rev the SDK knows (after a
restart it reads the card from the topic history). The helpers read 2.0 values (`"o2"`) and 2.1
values (`{value}`, `{values}`, `{value, other}`). The server takes only the first answer of an
ask round and only from members that `ask.from` names.

**Side chats (单独问).** A member can ask a bot privately about a topic: the question goes to
their DM with the bot, in a topic whose `contextOf` names the source topic and the seq they
read up to. `onAside` gets these messages, @ or not (without an `onAside` handler they go to
`onMention`). `ctx.aside` is the context and `ctx.source()` reads the source topic up to
`uptoSeq`, oldest first. Answer with `ctx.reply`: it stays in the DM. Never post into the source
topic from a side chat; the member does that with the suggestion button.

**Page events.** A bot's 产出 page may send events back when its version declares them:
`http.publishHtml({ ..., events: [{ name: "filter.pick", kind: "ask" }, { name: "report.flag" }] })`
(`kind` is `emit`, the default, or `ask`: the page waits for a reply). `onArtifactEvent` gets each
page event of my 产出 while the bot is connected (the transient `artifact.event`: `ctx.eventId`,
`ctx.kind`, `ctx.name`, `ctx.payload`, `ctx.actorId`). Answer an `ask` once with
`ctx.reply(payload)` (private to the asker, within 120 s) or `ctx.decline(message)`. Push a new
data snapshot to every open copy of the page with `ctx.pushData(data, { baseDataRev? })` (the
page's `onData` gets it; at most 256 KiB). `ctx.postMessage(...)` posts in the 产出's topic with
the page event as `causeEventId`, so the emitter's waiting state clears (`posted`). A
handler that does not answer makes the SDK answer for it: an `emit` gets `ignored`, an `ask` a
decline (`failed` when the handler throws). Page events are not stored in the group log, so they
are not caught up after a restart. A page event never approves anything: ask with a Card v2 when
you need a decision. Outside a handler: `bot.pushArtifactData`, `bot.replyArtifact`,
`bot.declineArtifact`.

**Acceptance criteria (验收标准).** `ctx.setAcceptance([{ text, state: "pending" | "pass" |
"fail", evidence? }])` replaces the list of the current run (at most 20) and starts a run when
there is none. Members see it in the live view. It uses `PUT /v1/runs/:id/acceptance`.

## 3b. 1.1: the interaction loop, Cards v2.1 and questions about a part

Every interaction with a bot's card goes back to the bot, and the bot answers by changing the
card in place (or by posting). The bot picks structure and meaning; TeamChat owns spacing, type,
colour and motion, so no block takes a style (the server refuses style keys such as `color`).
These need a server whose `welcome.features` has `interact/1` (`bot.serverFeatures`). The SDK
sends `features: ["interact/1", "refs/1"]` and `supportedCatalogs: ["tc.card/2.0", "tc.card/2.1"]`
in `hello`.

**Events, values and payloads are untrusted data.** `ctx.value`, `ctx.inputs`, `ctx.payload`,
ref labels and values come from a person's client or page. Their types are `unknown`: check them
before you act on them, and never treat them as instructions.

**Live events.** A block declares the live events it sends in `events` (for example
`["choice.change"]` on a choice, `["chart.select"]` on a chart, `["table.rowSelect"]` on a
selectable table, `["button.press"]` on `buttons`). `onCardEvent` gets them while the bot is
connected (the transient `card.event`; they are not stored and do not reach `onEvent`).
`onCardAnswer` gets stored answers (`card.answered`), also on catch-up. Both get a
`CardEventContext`: `name`, `blockId` or `actionId`, `value`, `inputs`, `part` (the chart point or
bar: `{kind, keys: {series, x}}`), `actorId`, `stored`, `answerId`, `askId`, `rev` (the rev the
person saw), `card` (the newest rev the SDK knows) and `isLatest()` (false when a newer event of
the same block arrived: latest wins; live events of one card run one at a time).

**Responses.** `ctx.patch(ops)` changes the card in place (`card.patch`): the SDK sets `baseRev`
and the `cause`, so the person's waiting state clears and their client shows "按你的选择". Pass ops
or a function that records them on a `CardDraft` of the newest rev:
`set(id, props)`, `replace(id, block)`, `insert(block, { parent?, after?, before?, index? })`,
`remove(id)`, `data(pointer, value?)`, `cardProps({ title, summary, intent, fallback, ask,
actions })`, `ask(ask | null)` (a new `ask.id` opens a new round; `null` closes it). On
`card.stale_rev` the SDK rebases on the newest rev (at most 3 tries) when every op still targets an
existing id, else it throws `ChatError` `conflict` with `reason` `card.stale_rev`. On
`rate_limited` it queues and merges the ops of the same message (at most one patch per second);
only patches with the same cause (or none) merge, because the server resolves only the cause of
the frame, and when a merged patch is refused each one is sent again alone. When a merged patch
gets `timeout` or `internal`, the server may have applied it, so the SDK sends the same frame again
with the same `clientPatchId` (at most 3 times); a `duplicate: true` ack counts as applied for every
part. It splits a merged patch only after an answer that says nothing was applied.
`ctx.reply(...)` and `ctx.postCard(card)` answer with a new message; `ctx.resolve("ignored" |
"failed", note?)` answers without a change (the note shows under the block for 5 s). A live-event
handler that returns without one of them makes the SDK send `ignored` (`failed` when it throws),
so the person never waits for nothing. Outside a handler: `bot.patchCard(groupId, messageId, ops,
{ baseRev?, cause?, clientPatchId? })` (with `baseRev` it never rebases), `bot.resolveEvent`,
`bot.updateCardV2(..., { baseRev, causeEventId })`, and `send({ causeEventId })`.

The server takes a cause only from the last 30 minutes (`interact.bad_cause` otherwise). An answer
read on catch-up after a longer time has no live waiting state, so the context's `patch`,
`reply`, `postCard`, `markAnswered` and `updateCard` then go without a cause, and `ctx.resolve`
does nothing. When the server refuses a cause the SDK added (clock skew), it sends the write again
without one. A cause you pass yourself is never dropped.

**Cards v2.1 builders.** `composeCard({ intent, title, summary?, fallback?, ask?, data?, blocks,
actions? })` makes a card from top-level blocks, flattens the children of containers into the
card's flat block list, sets `catalog: "tc.card/2.1"` when a 2.1 type is used, and checks every
write rule (depth, counts, one decision surface, one primary action, style keys, diagrams).
`blocks.*` has one builder per type:

- containers, whose children are blocks or block ids: `stack(id, children)`, `row(id, children,
  { weights })` (2–3 columns; folds to a stack when narrow), `group({ id, title, summary?,
  collapsible?, children })`, `compare({ id, question?, options: [{ id, label, pros?, cons?,
  recommended?, children? }], note? })`, `tabs({ id, tabs: [{ id, label, children }] })`,
  `questionnaire({ id, children, submitLabel? })`. Containers nest to depth 3.
- choices: `decision` (also exported as `choice`: single, or `multiple` with `minSelect`,
  `maxSelect`, `allowOther`, `recommended` options), `rank`, `rating`, `scale`, `confirm`,
  `imageChoice`, `matrix`, `slots`, `poll`, and the input kind `slider`.
- `buttons` (1–3 inline buttons that send `button.press`; never an answer), `diagram` (Mermaid;
  `fallbackText` defaults to the title or `图：<first node labels>`), `embed` (a live 产出 version
  inline; its `events` must be declared on the version), and every 2.0 block.

2.1 leaves get a default `fallbackText` (old clients show it). `mermaid(source)` makes a fenced
diagram for a message body; `checkDiagram(source)` runs the server's static checks.
`parts.cell(row, col)`, `parts.point(series, x)`, `parts.node(id)`, ... make `PartRef`s.

**Example: a data report with a line chart and "这个数据对不对？"** (V11-INTERACT 7.1). The metric
row, then the chart (2/3) beside the question (1/3); the question is a non-blocking ask of the
requester, so the card stays a report:

```ts
import { blocks, composeCard } from "@chat/bot-sdk";

const card = composeCard({
  intent: "done",
  title: "10 月数据月报",
  summary: "新增群 38 个，比 9 月多 12%；日活 2,340。",
  fallback: "Ledger 发了 10 月数据月报：新增群 38，日活 2,340。请看一眼数据对不对。",
  ask: { id: "verify", from: ["requester"], blocking: false, promise: "你确认之前，我不会把月报发到全员群。" },
  blocks: [
    blocks.row("kpis", [
      blocks.metric({ id: "m_groups", label: "新增群", value: 38, delta: 4, better: "up", note: "比 9 月" }),
      blocks.metric({ id: "m_dau", label: "日活", value: 2340, delta: 90, better: "up", note: "比 9 月" }),
      blocks.metric({ id: "m_reply", label: "平均回复", value: 1.2, unit: "秒", delta: -0.3, better: "down", note: "比 9 月" }),
    ]),
    blocks.row("main", [
      blocks.chart({
        id: "trend", kind: "line", label: "日活 · 近 4 周", emphasis: "high",
        x: ["10-06", "10-13", "10-20", "10-27"],
        series: [
          { id: "oct", name: "10 月", values: [2210, 2268, 2301, 2340] },
          { id: "sep", name: "9 月", values: [2150, 2190, 2236, 2250] },
        ],
        summary: "10 月日活从 2,210 涨到 2,340，每周都比 9 月高。",
        events: ["chart.select"],
      }),
      blocks.stack("check_col", [
        blocks.decision({
          id: "check", label: "这个数据对不对？", required: true, deliver: "onSubmit", events: ["choice.change"],
          options: [{ id: "ok", label: "对" }, { id: "wrong", label: "有问题" }],
        }),
        blocks.input({ id: "note", kind: "textarea", label: "补充一句（可选）", maxLength: 300 }),
      ]),
    ], { weights: [2, 1] }),
  ],
  actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "note"] }],
});
await bot.postCardV2({ groupId, topicId }, card);

// "有问题" picked (live, before the submit): add the channel breakdown under the row.
bot.onCardEvent(async (ctx) => {
  if (ctx.blockId === "check" && (ctx.value as { value?: string }).value === "wrong") {
    const rows = await channelRows();
    await ctx.patch((d) => {
      d.insert(blocks.group({ id: "drill", title: "按渠道拆开 · 新增群", collapsible: false, children: [
        blocks.table({ id: "by_channel", columns: ["渠道", "新增群", "占比"], rows,
          rowKeys: ["xhs", "invite", "bili", "douyin"], select: "multiple", events: ["table.rowSelect"] }),
        blocks.input({ id: "what", kind: "textarea", label: "哪一项不对？后台看到的是多少？" }),
      ] }), { after: "main" });
      d.remove("note");
      d.actions([{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "by_channel", "what"] }]);
    });
  }                                               // anything else: the SDK resolves "ignored"
});

// The submit (stored answer): fix the numbers in place.
bot.onCardAnswer(async (ctx) => {
  if (ctx.askId === "verify" && ctx.choice("check") === "wrong") {
    await ctx.patch([{ op: "set", id: "m_groups", props: { value: 40, delta: 6 } }, { op: "card", props: { actions: [] } }]);
  }
});
```

**Questions about a part (引用提问).** A person can point at one part of a message or a 产出 (a
table cell, a chart point, a diagram node, ...) and ask about it. The message then has `blockRefs`,
and the server writes one quote line per ref in front of the body (`> ↳ <label>`) for old
readers. In `onMention` and `onMessage`, `ctx.refs` is the list (`[]` when none) and
`ctx.question` is the body without those quote lines. `ctx.resolveRef(ref)` (or
`bot.resolveRef(ref, groupId)`) returns `{ message?, artifact?, block?, part?, current }`: the
source message (from the SDK's cache, else the topic history) or 产出 version, the block and the
part the ref names. `current` is false when the source was deleted, the card has a newer rev than
the one the person saw (`cardRev`), or the block or part is gone. Markdown blocks (`md:<n>`) are
counted best effort (`markdownBlocks`); the ref's `value` is always the sender's snapshot. Bots on
an older SDK see the quote lines in `body`, which still read as a quote.

## 4. One-shot scripts (HTTP)

`examples/notify.sh` posts one message with curl:

```sh
CHAT_URL=https://... CHAT_TOKEN_FILE=~/.config/team-chat/production/bot-ci-01J....token \
TITLE="Nightly build" examples/notify.sh <groupId> "build 142 passed"
```

Set `TOPIC_ID` to post in a topic, and `CLIENT_MSG_ID` to make a retried run post only once.
From TypeScript: `new HttpClient({ url, token }).postMessage(groupId, { body, clientMsgId })`.

## 5. Tests

`cd packages/bot-sdk && bun test` starts its own `wrangler dev` of `server/` on port 8795 (with
an empty database in a temp folder), and checks mention, reply, follow, streaming, topic status,
reconnect after a server restart, catch-up after a bot restart, handler re-runs without
duplicates, run plans (set, replaced, ended), and the 1.1 features: v2 card answers with
inputs, a v1 press on a v2 card, `markAnswered`, the done card, side chats and page events.
The 1.1 interaction loop runs there too (live events, patches with a rebase, the automatic
resolve, an answer caught up after a restart, page replies and data pushes, refs to parts): each
test that needs a server part the server does not have yet (`welcome.features`, or a frame that
gets "unknown frame type") is skipped, and the run prints why. `bun test test/unit.test.ts` checks
the same SDK logic against a fake server, without wrangler. `bun test/node-smoke.ts` runs the
echo example under Node.

---

# 机器人(中文)

外部机器人是一个 `kind: "bot"` 的成员。它和桌面 App 用同一套 API:WebSocket 收事件、发消息,
HTTP 传文件、产出和一次性发消息。TypeScript SDK 在 `packages/bot-sdk`(`@chat/bot-sdk`),
Bun 和 Node 22 以上都能运行。

## 1. 创建机器人成员,拿到令牌文件

只有 owner 能创建机器人。先用 `admin.ts bootstrap` 保存 owner 令牌(在
`~/.config/team-chat/<env>/owner-token`),然后:

```sh
bun server/scripts/admin.ts bot --env local --name "Deploy" --role "部署和 CI"
# bot 01J... (Deploy)
# bot token: /Users/<你>/.config/team-chat/local/bot-deploy-01J....token
```

脚本只打印文件路径,文件权限是 600。令牌(`tc_...`)不会过期:不要放进日志、聊天或 git。
吊销:`bun server/scripts/admin.ts revoke --env local --member <botId>`。

新机器人会自动加入默认群(bootstrap 时建的 "全员")。加入其他群:由该群任一成员调用
`POST /v1/groups/:id/members` `{ "memberId": "<botId>" }`,或者在桌面 App 设置里加。
机器人的级别是 `member`。

## 2. 运行机器人

```sh
bun install            # 只需一次,在仓库根目录
CHAT_URL=http://127.0.0.1:8789 \
CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-deploy-01J....token \
bun examples/echo-bot.ts          # 或者:node examples/echo-bot.ts
```

在机器人所在群的任意话题里 @ 它,它会回复 `echo: <内容>`。发 `@Deploy stream ...` 可以看到
流式回复。最小示例见上面英文部分的代码,API 相同。

## 3. SDK 做了什么

- **连接**:每次连接先发 `hello`,每 30 秒 ping 一次,断线后退避重连(0.5 秒到 30 秒,带抖动)。
  令牌被拒绝时停止,报 `ChatError`,code 为 `unauthorized`。
- **事件有序**:每个群按 `seq` 严格顺序处理,有缺口就用 `fetch` 补齐。重启后从保存的游标
  (`stateFile`)之后读取所有事件,这时处理函数里 `ctx.catchUp = true`。
- **第一次启动**:没有状态文件时跳过旧历史(`firstStart: "latest"`);传 `firstStart: "all"`
  则处理全部历史。之后新加入的群从头读取。
- **处理函数结束后才保存游标**,所以进程崩溃后处理函数会再跑一次。处理函数里的
  `ctx.reply`、`ctx.stream` 用由消息 id 生成的 clientMsgId,服务器对重试返回第一次的回复
  (`duplicate: true`),不会重复发。聊天以外的副作用(部署、调用 API)也要自己做成幂等。
- **处理函数**:`onMention`(@ 我的消息)、`onMessage`(我关注的话题里的其他消息)、
  `onCardAction`(有人按了我的卡片按钮)、`onCardAnswer`(1.1:有人回答了我用 `postCardV2` 发的
  v2 卡片;用 `ctx.resolve` 关闭它)、`onEvent`(所有事件)、`onTransient`(输入中、在线、
  流式增量)。一条消息最多触发 `onMention` 和 `onMessage` 中的一个;我自己的消息和系统消息
  都不触发。其他机器人的消息默认也不触发,避免两个机器人互相回复停不下来;传
  `allowBotAuthors: true` 可关闭这条规则。处理函数并发运行,出错交给 `onError`。
- **关注规则**:和服务器一致:我建的话题、我发过言的、@ 过我的、`waitingOn` 里加了我的、我是
  owner 的,都会自动关注。用 `bot.setFollow(...)` 修改。
- **写操作**会等连接可用,重连后重发(`send` 按 `clientMsgId` 幂等);被限流的写操作会重试;
  超过 `requestTimeoutMs`(60 秒)报 `timeout`。发送前先用协议 schema 校验。
- **流式回复**:`send` 带 `streaming: true`,每 100 毫秒合并发一次 `delta`,最后 `edit` 发完整
  正文并设 `streaming: false`。重连后会补发一次完整文本,读者不会漏内容。

方法列表见英文部分的表格。

**进度(运行计划)**:`ctx.setPlan([...])` 设置机器人在这个话题里当前一次运行的计划,成员在话题侧栏
「{bot} 的进度」里看到:完成的打勾、正在做的、等成员的(眼睛)、之后的(虚线圆)。没有运行时会先
开始一次运行;每次调用替换整个计划,`[]` 清空。最多 30 项,每项 1..200 字,状态 `done`、`current`、
`waiting`、`pending`。完成后用 `ctx.endRun("done", "已发出")` 结束。示例见英文部分。有执行器的
机器人调用会得到 `conflict`,它们的计划由执行器从 CLI 自己的待办列表(Claude Code 的 TodoWrite、
Codex 的计划更新)生成。

**1.1:卡片 v2、回答、单独问、页面事件**(需要 1.1 服务器,示例见 `examples/ask-bot.ts`):

- **交付方式**:收到请求先用表情回应(`ctx.ack()`,👀),不要发「收到」;进度放在计划里
  (`ctx.setPlan`),不用流式消息(`stream()` 仍可用,但不再推荐用来汇报工作);最后用
  `ctx.done({ title, summary, evidence, refs, metrics })` 发一张「做完了」卡片(Card v2,intent
  `done`)并结束这次运行,`failed: true` 则以失败结束:卡片开头加一个 `fail` 证据块 `failed`
  (文字为 `failedLabel`,默认「没有完成」),给 1.0.x 客户端的 v1 卡片用 `danger` 色调。
- **卡片构造函数**:`askCard`、`decisionCard`、`approvalCard`、`draftCard`、`formCard`、`doneCard`
  生成合法的 v2 卡片,不合协议时抛出 `ChatError`(`invalid`)。`from` 指定谁能回答(成员 id、
  `"requester"`、`"owners"`),`record: true` 表示答案是群里的决定,`irreversible` 说明不可撤销的
  后果。固定 id 见 `CARD_IDS`:选择块 `choice`、草稿块 `draft`、按钮 `submit`/`approve`/`deny`/
  `cancel`;纯文字选项的 id 是 `o1`、`o2`……
- **回答**:`onCardAnswer` 收到我的 v2 卡片的每个回答(重启后也能收到:内存里没有的卡片,SDK 会在处理函数运行前从历史消息加载,
  所以 `ctx.card`、`ctx.draft()`、`ctx.resolve()` 照常可用),旧客户端按 v2 卡片的
  按钮也会到这里(没有输入)。用 `ctx.choice()`、`ctx.number(id)`、`ctx.text(id)`、`ctx.bool(id)`、
  `ctx.draft()` 读取输入,`ctx.cancelled` 表示取消,`ctx.markAnswered()` 把卡片改成已回答(rev 加 1,
  去掉按钮)。输入来自客户端,使用前要检查。
- **单独问**:成员可以在和机器人的私聊里单独问某个话题,这个私聊话题的 `contextOf` 指向原话题和
  读到的位置。`onAside` 收到这些消息(不用 @;没有 `onAside` 时交给 `onMention`),`ctx.source()`
  读取原话题到 `uptoSeq` 为止的消息。用 `ctx.reply` 回答,只留在私聊里;不要往原话题发消息。
- **页面事件**:`http.publishHtml({ ..., events: [{ name, label, kind }] })` 声明产出页面可以发回的事件
  (`kind` 为 `emit` 或 `ask`),`onArtifactEvent` 收到它们。`ask` 用 `ctx.reply(payload)` 回答一次
  (只给提问的人)或 `ctx.decline(message)`;`ctx.pushData(data)` 把新数据推给所有打开这个页面的人。
  `ctx.postMessage` 发消息时把页面事件作为 `causeEventId`。
  处理函数没有回应时 SDK 自动回应(`emit` 回 `ignored`,`ask` 回拒绝)。页面事件不能当作批准:
  需要决定时发 Card v2 提问。
- **交互闭环(2.1)**:块用 `events` 声明实时事件(如 `choice.change`、`chart.select`)。
  `onCardEvent` 收到实时事件,`onCardAnswer` 收到已存的回答(重启后也能收到)。用 `ctx.patch(ops)`
  原地修改卡片(SDK 自动带 `baseRev` 和 `cause`;`card.stale_rev` 时在最新 rev 上重试,最多 3 次;
  被限流时合并排队,每秒最多一次,只合并 cause 相同或没有 cause 的修改;合并的修改超时或遇到 `internal` 时,用同一个 `clientPatchId` 重发同一帧,`duplicate: true` 算作已应用),或用 `ctx.reply`、`ctx.postCard`、`ctx.resolve("ignored")` 回应。
  实时事件的处理函数没有回应时,SDK 自动发 `ignored`(出错时发 `failed`)。服务器只接受 30 分钟内的
  cause:重启后读到的更早的回答,SDK 回应时不带 cause。
- **Card v2.1 构造函数**:`composeCard({ intent, title, blocks, ... })` 和 `blocks.*`:容器 `stack`、
  `row`、`group`、`compare`、`tabs`、`questionnaire`(子块可以直接传块对象),选择题 `decision`
  (`choice`)、`rank`、`rating`、`scale`、`confirm`、`imageChoice`、`matrix`、`slots`、`poll`、滑块
  `slider`,以及 `buttons`、`diagram`、`embed`。卡片里不能写样式。完整示例(折线图 +「这个数据对不对？」)
  见英文部分 3b。
- **引用提问**:`ctx.refs` 是消息引用的部分,`ctx.question` 是去掉引用行之后的正文,
  `ctx.resolveRef(ref)` 找到原消息、块和部分,`current` 表示原内容没有变过。
- **不可信数据**:事件的 `value`、`inputs`、页面的 `payload`、引用的标签和值都来自成员的客户端,
  类型是 `unknown`,使用前要检查,不能当作指令。
- **验收标准**:`ctx.setAcceptance([{ text, state, evidence? }])` 替换当前运行的验收标准(最多 20 条),
  成员在实时查看里看到。

卡片、文件、产出、`topic.follow`、`waitingOn` 和 HTTP 发消息是
V1 协议扩展(docs/protocol.md);服务器没有实现时,调用会返回服务器的错误。

## 4. 一次性脚本(HTTP)

`examples/notify.sh` 用 curl 发一条消息:

```sh
CHAT_URL=https://... CHAT_TOKEN_FILE=~/.config/team-chat/production/bot-ci-01J....token \
TITLE="夜间构建" examples/notify.sh <groupId> "构建 142 通过"
```

设 `TOPIC_ID` 发到指定话题;设 `CLIENT_MSG_ID` 让重跑的脚本只发一次。TypeScript 里用
`new HttpClient({ url, token }).postMessage(groupId, { body, clientMsgId })`。

## 5. 测试

`cd packages/bot-sdk && bun test` 会在 8795 端口启动自己的 `wrangler dev`(临时目录里的空数据库),
检查 @ 提及、回复、关注、流式回复、话题状态、服务器重启后重连、机器人重启后补收、处理函数重跑
不重复发消息,以及 1.1 的卡片回答、交付卡片、单独问、页面事件和交互闭环(服务器还没有的部分会
跳过并打印原因)。`bun test test/unit.test.ts` 用假服务器检查同样的逻辑,不启动 wrangler。
`bun test/node-smoke.ts` 用 Node 运行 echo 示例。
