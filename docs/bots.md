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

A new bot joins the default groups (the bootstrap group "General"). Add it to other groups
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
  topic I follow), `onCardAction` (a press on one of my cards), `onEvent` (every event),
  `onTransient` (typing, presence, deltas). A message runs at most one of `onMention` and
  `onMessage`. My own messages and system messages run neither. Handlers run concurrently;
  errors go to `onError`.
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
duplicates, and run plans (set, replaced, ended). `bun test/node-smoke.ts` runs the echo example under Node.

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

新机器人会自动加入默认群(bootstrap 时建的 "General")。加入其他群:由该群任一成员调用
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
  `onCardAction`(有人按了我的卡片按钮)、`onEvent`(所有事件)、`onTransient`(输入中、在线、
  流式增量)。一条消息最多触发 `onMention` 和 `onMessage` 中的一个;我自己的消息和系统消息
  都不触发。处理函数并发运行,出错交给 `onError`。
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
检查 @ 提及、回复、关注、流式回复、话题状态、服务器重启后重连、机器人重启后补收,以及处理函数重跑
不重复发消息。`bun test/node-smoke.ts` 用 Node 运行 echo 示例。
