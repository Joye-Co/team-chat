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
it as failed):

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
  await ctx.resolve();                           // edit the card: rev + 1, no buttons
});
```

The values come from the client: check them. `ctx.card` is the answered card when this process
saw it (sent or caught up); `resolve(change, base)` needs it or `base`. The server takes only
the first answer to a rev of an ask and only from members that `ask.from` names.

**Side chats (单独问).** A member can ask a bot privately about a topic: the question goes to
their DM with the bot, in a topic whose `contextOf` names the source topic and the seq they
read up to. `onAside` gets these messages, @ or not (without an `onAside` handler they go to
`onMention`). `ctx.aside` is the context and `ctx.source()` reads the source topic up to
`uptoSeq`, oldest first. Answer with `ctx.reply`: it stays in the DM. Never post into the source
topic from a side chat; the member does that with the suggestion button.

**Page events.** A bot's 产出 page may send events back when its version declares them:
`http.publishHtml({ ..., events: [{ name: "pick", label: "选这个" }] })`. `onArtifactEvent`
gets each `artifact.evented` on my 产出 (`ctx.name`, `ctx.payload`, `ctx.actorId`). A page event
never approves anything: ask with a Card v2 when you need a decision.

**Acceptance criteria (验收标准).** `ctx.setAcceptance([{ text, state: "pending" | "pass" |
"fail", evidence? }])` replaces the list of the current run (at most 20) and starts a run when
there is none. Members see it in the live view. It uses `PUT /v1/runs/:id/acceptance`.

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
inputs, a v1 press on a v2 card, `resolve`, the done card, side chats and page events.
`bun test/node-smoke.ts` runs the echo example under Node.

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
  `done`)并结束这次运行,`failed: true` 则以失败结束。
- **卡片构造函数**:`askCard`、`decisionCard`、`approvalCard`、`draftCard`、`formCard`、`doneCard`
  生成合法的 v2 卡片,不合协议时抛出 `ChatError`(`invalid`)。`from` 指定谁能回答(成员 id、
  `"requester"`、`"owners"`),`record: true` 表示答案是群里的决定,`irreversible` 说明不可撤销的
  后果。固定 id 见 `CARD_IDS`:选择块 `choice`、草稿块 `draft`、按钮 `submit`/`approve`/`deny`/
  `cancel`;纯文字选项的 id 是 `o1`、`o2`……
- **回答**:`onCardAnswer` 收到我的 v2 卡片的每个回答(重启后也能收到:内存里没有的卡片,SDK 会在处理函数运行前从历史消息加载,
  所以 `ctx.card`、`ctx.draft()`、`ctx.resolve()` 照常可用),旧客户端按 v2 卡片的
  按钮也会到这里(没有输入)。用 `ctx.choice()`、`ctx.number(id)`、`ctx.text(id)`、`ctx.bool(id)`、
  `ctx.draft()` 读取输入,`ctx.cancelled` 表示取消,`ctx.resolve()` 把卡片改成已回答(rev 加 1,
  去掉按钮)。输入来自客户端,使用前要检查。
- **单独问**:成员可以在和机器人的私聊里单独问某个话题,这个私聊话题的 `contextOf` 指向原话题和
  读到的位置。`onAside` 收到这些消息(不用 @;没有 `onAside` 时交给 `onMention`),`ctx.source()`
  读取原话题到 `uptoSeq` 为止的消息。用 `ctx.reply` 回答,只留在私聊里;不要往原话题发消息。
- **页面事件**:`http.publishHtml({ ..., events: [{ name, label }] })` 声明产出页面可以发回的事件,
  `onArtifactEvent` 收到它们。页面事件不能当作批准:需要决定时发 Card v2 提问。
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
不重复发消息,以及 1.1 的卡片回答、交付卡片、单独问和页面事件。`bun test/node-smoke.ts` 用 Node 运行 echo 示例。
