# Bots

[中文](#机器人中文) below.

An external bot is a member with `kind: "bot"`. It uses the same API as the desktop app: a
WebSocket for events and writes, and HTTP for files, 产出 and one-shot posts. The TypeScript
SDK is `packages/bot-sdk` (`@chat/bot-sdk`); it is TypeScript source and runs on Bun and on
Node 22.18+ (Node 22.6 to 22.17 need `--experimental-strip-types`).

## 1. Create a bot member and get its token file

Only an owner can create a bot. `--character <id>` gives a bot one of the 26 Jojo & Friends
characters (the desktop app picks an unused one when you add a bot there). Keep your own list of bots and the
commands that made them in your ops checkout ([ENVIRONMENTS.md](ENVIRONMENTS.md#ops-checkout)).
With the owner token saved by `admin.ts bootstrap` (in
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
bun examples/echo-bot.ts          # or, with Node 22.18+: node examples/echo-bot.ts
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

`tokenFile` and `stateFile` read a leading `~` as the home folder, so a path from a `.env` file
or a service unit works as written. Bad options (no token, a missing token file, a damaged state
file, a URL that is not `http(s)://host`) make `new Bot(...)` throw `ChatError` `invalid` with
the file or the option in the message.

## 3. What the SDK does

- **Connection**: `hello` on every connection, ping every 30 s, reconnect with backoff (0.5 s to
  30 s, with jitter). A rejected token stops the bot with `ChatError` code `unauthorized`.
  `url` is the server's base URL (a trailing `/v1` is dropped). `start()` resolves on the first
  welcome; while the server cannot be reached it keeps trying, and each failed try logs a
  warning `cannot reach <ws url> (<why>), retrying in <n> s`.
- **Ordered events**: events are applied per group strictly in `seq` order. A gap is filled
  with `fetch`. After a restart, the bot reads every event after its saved cursor
  (`stateFile`) and runs handlers with `ctx.catchUp = true`.
- **First start**: with no state file, the bot skips old history (`firstStart: "latest"`); pass
  `firstStart: "all"` to handle it. Groups the bot joins later are read from their start.
- **Cursors are saved after handlers finish**, so a crash runs a handler again. Replies from a
  handler context (`ctx.reply`, `ctx.stream`, `ctx.ask`, `ctx.postCard`, `ctx.done`) use a
  clientMsgId made from the message (or card event) id, so the
  server answers the retry with the first reply (`duplicate: true`) and nothing is posted twice.
  `bot.send`, `bot.done` and the other `bot.*` posts make a random id unless you pass `clientMsgId`.
  Effects outside the chat (deploys, API calls) must be idempotent too.
- **Handlers**: `onMention` (a message that mentions me), `onMessage` (any other message in a
  topic I follow), `onCardAction` (a press on one of my cards), `onCardAnswer` (1.1: an answer
  to one of my v2 cards, posted with `postCardV2`; close it with
  `ctx.markAnswered()`), `onCardEvent` (1.1: a live event of one of my v2 cards, section 3b),
  `onArtifactEvent` (1.1: an event from the page of one of my 产出, section 3a), `onAside`
  (1.1: a side chat), `onEvent` (every event),
  `onTransient` (typing, presence, deltas). A message runs at most one of `onMention` and
  `onMessage`. My own messages and system messages run neither. Messages of other bots run
  neither either, so two bots cannot answer each other without end; `allowBotAuthors: true`
  turns this off. Handlers run concurrently; errors go to `onError`.
- **Follow rules**: the bot follows a topic it created, posted in, was mentioned in, was added
  to `waitingOn` of, or became owner of, as the server does. `bot.setFollow(...)` changes it.
- **Writes** wait for the connection and are resent after a reconnect (`send` is idempotent
  by `clientMsgId`). Writes are paced on the client at 18 per second with bursts of 18
  (`writeRate`; the server allows about 20 per member), so a burst of 50 sends lands in order.
  Rate-limited writes are retried after the server's `retryAfterMs`. A request fails with `timeout` after
  `requestTimeoutMs` (60 s). Frames are checked against the protocol schemas before sending.
- **Errors**: a failed call throws `ChatError` with the server's `code` and, when the server
  sends them, `reason`, `retryAfterMs` (how long to wait after `rate_limited`) and `memberIds`
  (the members a reason such as `relay.member_unavailable` is about). HTTP calls also set
  `status`. `isRetryable(err)` is true for `network`, `timeout`, `internal` and `rate_limited`.
- **Streaming**: `send` with `streaming: true`, `delta` frames batched every 100 ms, and a final
  `edit` with the full body and `streaming: false`. After a reconnect the writer sends the full
  text again so readers miss nothing. A message stays "streaming" until that final edit, so
  the SDK finishes a `ctx.stream()` writer that its handler left open (it returned or threw
  before `finish()`), and `stop()` finishes every writer that is still open.

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
| `postCardV2(target, card)` / `updateCardV2(groupId, messageId, card)` / `ctx.ask(card)` / `ctx.postCard(card)` | Card v2 (1.1, below); `ctx.postCard` is `ctx.ask` for a card that asks nothing |
| `patchCard(groupId, messageId, ops, opts?)` / `ctx.patch(ops)` | change one of my v2 cards in place (3b) |
| `resolveEvent(groupId, eventId, result, note?)` / `ctx.resolve(result, note?)` | answer a live event without a change (3b) |
| `answerCard(groupId, { messageId, rev, actionId, inputs?, editedValues? })` | answer another member's v2 card |
| `ack(groupId, messageId, emoji?)` / `ctx.ack()` | react (default 👀) instead of an "on it" line |
| `done(target, card)` / `ctx.done(card)` | post the done card and end my run |
| `setAcceptance(target, items)` / `ctx.setAcceptance(items)` | the 验收标准 of my current run |
| `publishPage(target, { title, html, events?, data?, preview? })` / `ctx.publishPage(...)` / `http.publishHtml({ ..., events, data, preview, runId })` | an interactive 产出 whose page sends events back (3a) |
| `pushArtifactData(groupId, artifactId, version, data, opts?)` / `ctx.pushData(data, opts?)` | a new data snapshot for every open copy of a page |
| `replyArtifact(groupId, eventId, payload)` / `declineArtifact(groupId, eventId, message)` / `ctx.reply`, `ctx.decline` | answer a page's `ask` |
| `http.getArtifactData(artifactId, version)` / `ctx.data()` / `http.putArtifactData(...)` | read (or write over HTTP) a version's data snapshot |
| `http.artifactEvents(artifactId, { before?, limit? })` | the stored page events of a 产出, newest first |
| `resolveRef(ref, groupId?)` / `ctx.resolveRef(ref)` | what a 引用提问 ref points at (3b) |

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
    { text: "等负责人确认", state: "pending" },
    { text: "周四 20:00 发出", state: "pending" },
  ]);
  // ... write the draft ...
  await ctx.setPlan([
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "done" },
    { text: "等负责人确认", state: "waiting" },
    { text: "周四 20:00 发出", state: "pending" },
  ]);
  await ctx.updateTopic({ status: "waiting_review", waitingOn: [ctx.message.authorId], ownerId: bot.me.id });
});
// Later, when the post is out:
await bot.endRun({ groupId, topicId }, "done", "已发出");
```

Runner-backed bots get `conflict` from `setPlan`: their runner sets the plan from the coding
CLI's own to-do list (Claude Code's TodoWrite, Codex plan updates).

### The bot's line on the 首页 desk (1.1)

`bot.http.setDeskNote(groupId, text)` sets the bot's one-line priority for the group, which
members see at the top of 首页 ("今天先把 1.1 发出去"). At most 140 characters after trimming;
`""` clears it, and the same text again stores nothing (`seq: null`). `deskNotes(groupId)` lists
the notes of the group's bots.

```ts
await bot.http.setDeskNote(groupId, "今天先把 1.1 发出去");
```

## 3a. 1.1: Card v2, answers, side chats, page events

These need a 1.1 server. Three examples run as they are: `examples/ask-bot.ts` shows the output
contract, an ask, its answer and a side chat; `examples/page-bot.ts` (with its page,
`examples/page-bot.html`) shows an interactive page; `examples/report-bot.ts` shows a 2.1 card
that changes in place (section 3b).

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
history before the handler runs, so `ctx.card`, `ctx.draft()` and `ctx.markAnswered()` work). A press on a v2 card by an old client arrives here too,
with no inputs. Type the inputs with a generic and read them with the helpers:

```ts
bot.onCardAnswer<{ choice: string; count: number }>(async (ctx) => {
  if (ctx.cancelled) return;                     // a cancel action ("deny", "cancel")
  ctx.choice();                                  // first chosen option of block "choice"
  ctx.number("count"); ctx.text("note"); ctx.bool("notify"); ctx.draft();   // inputs by block id
  await ctx.markAnswered();                      // edit the card: rev + 1, no buttons
});
```

The values come from the client: check them. On a private card (a non-public poll, an
anonymous questionnaire) `ctx.anonymous` is true and `ctx.actorId` is `"anonymous"`
(`ANONYMOUS_ACTOR`), not a member id: do not mention it or put it in `from` or `waitingOn`. A run
such an answer starts has no `requestedBy` in the SDK's events. `ctx.card` is the newest rev the SDK knows (after a
restart it reads the card from the topic history). The helpers read 2.0 values (`"o2"`) and 2.1
values (`{value}`, `{values}`, `{value, other}`). The server takes only the first answer of an
ask round and only from members that `ask.from` names.

**Side chats (单独问).** A member can ask a bot privately about a topic: the question goes to
their DM with the bot, in a topic whose `contextOf` names the source topic and the seq they
read up to. `onAside` gets these messages, @ or not (without an `onAside` handler they go to
`onMention`). `ctx.aside` is the context and `ctx.source()` reads the source topic up to
`uptoSeq`, oldest first. Answer with `ctx.reply`: it stays in the DM. Never post into the source
topic from a side chat; the member does that with the suggestion button. A side chat opened before the bot's
cursor (a first start that skipped history, or state of an older SDK) is found too: the SDK reads
each DM group once from its start for side chat topics (`asideScan` in the state file), without
running handlers, before it handles that group's newer events.

**Page events: a two-way page.** A bot's 产出 page can talk to its bot. The version declares the
events the page may send, the page calls `window.teamchat` (next heading), and the bot answers in
`onArtifactEvent`. `bot.publishPage(target, input)` (in a handler: `ctx.publishPage(input)`)
uploads one HTML page as a 产出 of the topic, made by the bot's current run there; it is
`http.publishHtml` with the topic and `runId` filled in. Its input:

- `title`, `html`, and `artifactId` to add a version to a 产出 instead of making a new one.
- `events: [{ name, kind?, label?, schema? }]`: what the page may send. `kind` is `emit` (the
  default: fire and forget) or `ask` (the page waits for a reply). `schema` gives the payload a
  shape (TcSchema: `type`, `properties`, `required`, `enum`, `items`, `maxItems`, `minimum`,
  `maximum`, `maxLength`, `pattern`); the server refuses a payload that does not fit.
- `data`: the first data snapshot (JSON, at most 256 KiB), so the page never opens with `null`.
- `preview: { title?, summary?, metrics? }`: the text and up to 6 figures (`{ label, value,
  delta? }`) of the version's preview card.

Show the page in the conversation with a card that has `blocks.embed({ id, artifactId, version,
height?, events? })` (160–480 pt high; `events` names the declared events the embedded copy may
send), or with `artifactRefs` on a message. `examples/page-bot.ts` is a whole program; the loop
in short (`PAGE` is the HTML after it):

```ts
import { blocks, composeCard } from "@chat/bot-sdk";

const DISHES = ["面", "饭", "饺子"];
const page = await bot.publishPage({ groupId, topicId }, {
  title: "午饭吃什么",
  html: PAGE,
  events: [
    { name: "lunch.vote", label: "投票", schema: { type: "object", properties: { dish: { type: "string", enum: DISHES } }, required: ["dish"] } },
    { name: "lunch.mine", kind: "ask", label: "问自己投了什么" },
  ],
  data: { picks: {} },                              // the first snapshot: member id -> dish
  preview: { summary: "点一下投票，结果大家都看得到。" },
});
await bot.postCardV2({ groupId, topicId }, composeCard({
  intent: "info",
  title: "午饭吃什么",
  blocks: [blocks.embed({ id: "page", artifactId: page.artifact.id, version: page.version.version, height: 200, events: ["lunch.vote", "lunch.mine"] })],
}));

type Picks = { picks: Record<string, string> };
// An emit. The payload comes from the page: check it. `actorId` comes from the server.
bot.onArtifactEvent("lunch.vote", async (ctx) => {
  const dish = (ctx.payload as { dish?: unknown } | null)?.dish;
  if (typeof dish !== "string" || !DISHES.includes(dish)) return;    // the SDK resolves "ignored"
  const { data, dataRev } = await ctx.data();                         // the state lives on the server
  const picks = { ...(data as Picks | null)?.picks, [ctx.actorId]: dish };
  await ctx.pushData({ picks }, { baseDataRev: dataRev });            // every open copy redraws
});
// An ask: only the member who asked gets the reply.
bot.onArtifactEvent("lunch.mine", async (ctx) => {
  const { data } = await ctx.data();
  await ctx.reply({ dish: (data as Picks | null)?.picks?.[ctx.actorId] ?? null });
});
```

```html
<div id="dishes"></div>
<button id="mine">我投了什么？</button>
<p id="status"></p>
<script>
  const tc = window.teamchat;                       // absent in a plain browser
  const say = (text) => (document.getElementById("status").textContent = text);
  function render(data) {
    const picks = Object.values(data?.picks ?? {});
    document.getElementById("dishes").replaceChildren(
      ...["面", "饭", "饺子"].map((dish) => {
        const b = document.createElement("button");
        b.textContent = `${dish} · ${picks.filter((p) => p === dish).length}`;
        b.disabled = !tc?.can.emit;
        b.onclick = () => tc.emit("lunch.vote", { dish }).catch((e) => say(e.code));
        return b;
      }),
    );
  }
  if (tc) tc.onData(render);                        // now, and after every push of the bot
  else render({ picks: { a: "面" } });
  document.getElementById("mine").disabled = !tc?.can.ask;
  document.getElementById("mine").onclick = async () => {
    try {
      const { dish } = await tc.ask("lunch.mine");
      say(dish ? `你投了${dish}。` : "你还没投。");
    } catch (e) {
      say(e.code);                                  // "timeout", "declined", "offline", ...
    }
  };
</script>
```

In the handler, `onArtifactEvent(handler)` gets every page event of my 产出 and
`onArtifactEvent(name, handler)` only the ones with that name, while the bot is connected (the
transient `artifact.event`). The context has `eventId`, `kind`, `name`, `payload`, `actorId`,
`artifactId` and `version`, and:

- `ctx.reply(payload)` answers an `ask` once (private to the asker, within 120 s);
  `ctx.decline(message)` refuses it.
- `ctx.data()` reads the version's current snapshot from the server: `{ data, dataRev }` (`null`
  and 0 when it has none). Keep a page's state there, not in the process: it survives a restart.
- `ctx.pushData(data, { baseDataRev? })` replaces the snapshot and sends it to every open copy of
  the page (its `onData`; at most 256 KiB, about 2 pushes per second per version). With
  `baseDataRev` the server refuses the push with `conflict` when another push came in between:
  read again and retry (`examples/page-bot.ts` does).
- `ctx.postMessage(...)` posts in the 产出's topic with the page event as `causeEventId`, so the
  emitter's waiting state clears (`posted`).

A handler that does not answer makes the SDK answer for it: an `emit` gets `ignored`, an `ask` a
decline (`failed` when the handler throws). A page event never approves anything: ask with a
Card v2 when you need a decision. Outside a handler: `bot.pushArtifactData`, `bot.replyArtifact`,
`bot.declineArtifact`, and over HTTP for a script without a connection `http.getArtifactData`
and `http.putArtifactData(artifactId, version, data, { baseDataRev?, cause? })`.

Page events are not in the group log, so the bot does not get the ones it missed while it was
down. After a restart read them from the page log, `http.artifactEvents(artifactId, { before?,
limit? })` (newest first, kept 180 days; `replied` is true on an ask that was answered), and the
state from `http.getArtifactData(artifactId, version)`.

The server checks a `pattern` of a `schema` in linear time, so it refuses a backreference (`\1`,
`\k<…>`), lookaround (`(?=`, `(?!`, `(?<=`, `(?<!`) or a pattern of more than 1000 states, and a
string longer than 4096 characters (`MAX_PATTERN_INPUT`) does not match. `publishPage`,
`publishHtml`, `createArtifact` and `addArtifactVersion` check the declarations with the same
rule (`checkArtifactEvents`, `patternProblem`) and the size of `data` (`checkArtifactData`), and
throw `ChatError` (`invalid` with reason `artifact.bad_schema`, or `too_large`) before anything
is uploaded.

### The page side: `window.teamchat`

TeamChat puts `window.teamchat` into the page before its own scripts run. It is absent in a
plain browser, so guard it (`window.teamchat?.`) and the page still opens from a file. The page
reaches its bot only through these calls.

| Member | What it is |
| --- | --- |
| `teamchat.version` | `1`. |
| `teamchat.viewer` | `{ memberId, name }` of the member who looks at the page, or `null`. |
| `teamchat.can` | `{ emit, ask, events: [{ name, kind }] }`: whether this copy may send an `emit` or an `ask` (false when the version declares none of that kind, was made by a person, or its bot left the team or was deactivated), and the declared events. |
| `teamchat.getData()` | `Promise<data>`: the version's current snapshot (`null` when it has none). |
| `teamchat.onData((data, meta) => …)` | Calls back with the current snapshot, then again on every push. `meta` is `{ rev, cause? }`; `cause` is `{ eventId, actorId }` when a page event caused the push. Returns a function that stops it. |
| `teamchat.emit(name, payload?)` | Sends a declared `emit` event to the bot. `Promise<{ eventId }>`: resolved when the server took it, not when the bot answered. |
| `teamchat.ask(name, payload?, { timeoutMs? })` | Sends a declared `ask` and resolves with the bot's reply payload. Default 30 s, at most 120 s; at most 4 open asks per page. |
| `teamchat.ref({ label, value?, selector? })` | Puts a part of the page into the viewer's composer as a 引用提问 (`label` 1–80 characters, `value` at most 2048 bytes of JSON, at most 2 per second). |

- **A call follows the viewer's own input.** `emit` and `ask` go through only within 2 s of a
  click, drag or key press of the viewer in the page; one without rejects with `no_user_action`. Call
  them from the event handler, not from a timer or on load.
- **Payloads** are JSON of at most 16 KiB. Only declared names of the declared kind are sent. When
  a page sends faster than the app's pace, a later `emit` of the same name replaces the waiting
  one, which rejects with `rate_limited`.
- **Errors.** A rejected call gives an `Error` with a `code`: `not_allowed` (not declared, or no
  capability), `no_user_action` (no recent input of the viewer), `bad_payload` (the payload does
  not fit the event's schema; `message` says where), `bad_request`, `too_large`, `rate_limited`,
  `offline`, `timeout` (no reply to an `ask` in time), `declined` (the bot's `ctx.decline`; `message`
  is its text), `failed` (the
  bot's handler threw, or the server refused the event), `reloaded` (the page was reloaded while
  the call was open) and `unknown_method`.
- **Parts to ask about.** An element with `data-ref-label="…"` can be pointed at: ⌥-click (Alt on
  Windows) or its context menu puts it into the viewer's composer. `data-ref-value` is its value
  (JSON, else the text; without it the element's text, cut to 200 characters).
- **Old pages.** `window.artifact` (`getData`, `emitEvent(name, payload)`, `ref`, `call`) is the
  1.0 name of the bridge and still works; `emitEvent` resolves `true` at once. New pages use
  `teamchat.emit`.

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

`onCardEvent(handler)` gets every live event; `onCardEvent("button.press", handler)` only the
events with that name, and `onCardEvent({ name?, block? }, handler)` only the ones of that name
and block id. An event no handler answers is resolved `ignored`.

What each event carries, and the reader of the context that checks it. A reader returns
`undefined` (or `[]`) when the value has another shape, so no handler has to cast `ctx.value`:

| Event | Sent by | `ctx.value` | Reader |
| --- | --- | --- | --- |
| `choice.change`, `choice.submit` | `decision`, `imageChoice`, `slots`, `poll` | `{ value: optionId }`; a multiple choice `{ values: [optionId] }`; the "other" option `{ value: "other", other: text }` | `ctx.choice()`, `ctx.choices()` |
| `choice.change`, `choice.submit` | `confirm` | `{ value: true \| false }` | read `ctx.value` |
| `compare.pick` | `compare` | `{ value: optionId, note? }` | `ctx.choice()` |
| `input.change`, `input.submit` | `input` (`input.submit`: kinds `text` and `number` only) | `{ value }` (text, number, boolean or option id by kind); kind `chips` `{ values }` | read `ctx.value` |
| `input.change` | `rank`, `matrix`, `rating`, `scale` | `{ order: [itemId] }`, `{ rows: { rowId: columnId } }`, `{ value: 1..5 }`, `{ value: number }` | read `ctx.value` |
| `chart.select` | `chart` | `{ series, x, y, index }` (`series` is the series id, or its name without an id); `ctx.part` is the point or bar | `ctx.point()` |
| `chart.clear` | `chart` | none | |
| `metric.press` | `metric` | none; `ctx.part` is `{ kind: "metric" }` | |
| `table.rowSelect` | `table` | `{ values: [rowKey] }` (`rowKeys`, else the row index as text) | `ctx.rows()` |
| `table.sort` | `table` | `{ column, dir: "asc" \| "desc" }` | `ctx.sort()` |
| `button.press` | `buttons` | `{ buttonId }` | `ctx.button()` |
| `tab.open` | `tabs` | `{ tabId }` | `ctx.tab()` |
| `draft.edit` | `draft` | `{ fieldId, value }` | read `ctx.value` |

A block sends only the events of its row, and only the ones it declares in `events` (the commit
of a `deliver: "immediate"` block, a `confirm`, a `compare` and a `poll` needs no declaration: it
is an answer). The server refuses any other press, so `composeCard` throws `ChatError` `invalid`
when a block declares an event its type cannot send (`block "check" (decision) cannot send
chart.select; it can send: choice.change, choice.submit`), and `blocks.buttons` declares
`button.press` for you. `blockEvents(block)` is the list. On a stored answer (`onCardAnswer`)
`ctx.choice(blockId)` and `ctx.choices(blockId)` read `inputs` instead, by block id.

**Responses.** `ctx.patch(ops)` changes the card in place (`card.patch`): the SDK sets `baseRev`
and the `cause`, so the person's waiting state clears and their client shows "按你的选择". Pass ops
or a function that records them on a `CardDraft` of the newest rev:
`set(id, props)`, `replace(id, block)`, `insert(block, { parent?, after?, before?, index? })`,
`remove(id)`, `data(pointer, value?)`, `cardProps({ title, summary, intent, fallback, ask,
actions })`, `ask(ask | null)` (a new `ask.id` opens a new round; `null` closes it),
`actions(list)` (replaces the card's actions; `[]` removes them). The draft
knows the card: `set`, `replace`, `remove` and the `parent`, `after` or `before` of an `insert`
throw `ChatError` `invalid` for an id the card does not have (`set: no block "m_dua" in rev 3;
blocks: kpis, m_dau, …`), counting what the draft inserted and removed so far, and nothing is
sent. `d.set<"metric">("m_dau", { value: 2341 })` checks the fields against the block type;
`d.block(id)` reads a block. A raw op list is sent as it is. On
`card.stale_rev` the SDK rebases on the newest rev (at most 3 tries) when every op still targets an
existing id, else it throws `ChatError` `conflict` with `reason` `card.stale_rev`. On
`rate_limited` it queues and merges the ops of the same message (at most one patch per second);
only patches with the same cause (or none) merge, because the server resolves only the cause of
the frame, and when a merged patch is refused each one is sent again alone. When a merged patch
gets `timeout` or `internal`, the server may have applied it, so the SDK sends the same frame again
with the same `clientPatchId` (at most 3 times); a `duplicate: true` ack counts as applied for every
part. It splits a merged patch only after an answer that says nothing was applied.
`ctx.reply(...)` and `ctx.postCard(card)` answer with a new message; `ctx.resolve("ignored" |
"failed", note?)` answers without a change (the note shows under the block for 5 s).
`ctx.done(card)` posts the done card as the answer and ends the run; `ctx.setPlan`, `ctx.endRun`
and `ctx.setAcceptance` work in the card's topic as they do in a message handler. A live-event
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
write rule (depth, counts, one decision surface, one primary action, style keys, diagrams). A
card that does not fit throws `ChatError` `invalid` with up to 5 problems, each with the id and
type of its block (`card: block "trend" (chart) series.0.values.0: …`; a repeated id or a second
primary action says which). `blocks.*` has one builder per type:

- containers, whose children are blocks or block ids: `stack(id, children)`, `row(id, children,
  { weights })` (2–3 columns; folds to a stack when narrow), `group({ id, title, summary?,
  collapsible?, children })`, `compare({ id, question?, options: [{ id, label, pros?, cons?,
  recommended?, children? }], note? })`, `tabs({ id, tabs: [{ id, label, children }] })`,
  `questionnaire({ id, children, submitLabel? })`. Containers nest to depth 3.
- choices: `decision` (also exported as `choice`: single, or `multiple` with `minSelect`,
  `maxSelect`, `allowOther`, `recommended` options), `rank`, `rating`, `scale`, `confirm`,
  `imageChoice`, `matrix`, `slots`, `poll`, and the input kind `slider`.
- `buttons` (1–3 inline buttons; a press reaches `onCardEvent` as `button.press`, with
  `ctx.button()` the button id, and is never an answer; `events` defaults to
  `["button.press"]`), `diagram` (Mermaid;
  `fallbackText` defaults to the title or `图：<first node labels>`), `embed` (a live 产出 version
  inline; its `events` must be declared on the version), and every 2.0 block.

2.1 leaves get a default `fallbackText` (old clients show it). `mermaid(source)` makes a fenced
diagram for a message body; `checkDiagram(source)` runs the server's static checks.
`parts.cell(row, col)`, `parts.point(series, x)`, `parts.node(id)`, ... make `PartRef`s.

**Example: a data report with a line chart and "这个数据对不对？"** (V11-INTERACT 7.1). The metric
row, then the chart (2/3) beside the question (1/3); the question is a non-blocking ask of the
requester, so the card stays a report. `examples/report-bot.ts` is this loop as a whole program
that answers a mention, with a chart point, an inline button and a live pick each patching the
card:

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
bot.onCardEvent({ block: "check" }, async (ctx) => {
  if (ctx.choice() === "wrong") {                 // a checked reader: undefined on another shape
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
resolve, an answer caught up after a restart, page replies and data pushes, refs to parts): the
run fails, and names the part, when the server lacks a route, a `welcome.features` entry or a
frame handler these tests need. `bun test test/unit.test.ts` checks the same SDK logic against a
fake server, without wrangler; `test/artifacts.test.ts` (pages, their data and page log, live
event values) and `test/card-checks.test.ts` (block events, card error texts, `CardDraft` ids) do
the same for the generative-UI calls. The examples cannot rot: `test/examples.test.ts` runs the
real handlers of `examples/report-bot.ts` and `examples/page-bot.ts` (every card and patch goes
through the server's own patch and schema rules, and every `teamchat.emit` and `teamchat.ask` of
the page must be declared), and `test/docs-examples.test.ts` typechecks every `ts` block of this
guide and of the SDK README against the SDK and runs the card and page examples.
`bun run typecheck` covers `examples/*.ts`. `bun run test:node` (`bun test/node-smoke.ts`) runs
the echo example under Node 22.18 or later against the same local server.

## 6. Runner-backed bots: a coding CLI on a machine

A runner-backed bot does its work with a coding CLI (Claude Code, Codex, an ACP agent) on a
machine that runs the machine daemon ([machine-protocol.md](machine-protocol.md)). Operators
set it up once per machine:

1. On the machine, install the coding CLI and sign in to it (`claude --version` or
   `codex --version` works in a Terminal).
2. Short way (1.2.1 and later): install the app on that machine, sign in as an owner, and click
   用这台电脑 in 设置 › 机器. The app registers the machine, writes the daemon config and token,
   installs the launchd service (on Windows a scheduled task of your user) and updates the
   daemon with the app.
3. Manual way: build the daemon (`scripts/build-daemon.sh`) or download it from a release,
   register the machine (`bun server/scripts/admin.ts machine --env <env> --name <name> --labels <label>`
   writes the machine token file), copy the binary and the token to the machine, write
   `~/.config/team-chat/daemon.toml` (`team-chat-daemon example-config` prints an example; set
   `server_url`, `token_file`, `name`, `labels` and the `[runtimes.<name>]` you use), then run
   `team-chat-daemon doctor` and `team-chat-daemon install-service` from a Terminal where the
   CLI is on `PATH`.

   A Linux host that runs each run in a Firecracker microVM is set up as in
   [SELF-HOSTING.md, Part 2](SELF-HOSTING.md#part-2-a-linux-host-for-the-teams-ai-members).

   On Windows the manual way is one PowerShell command, run as the user whose CLI logins the
   bots should use (no administrator rights). Register the machine first (the `admin.ts machine`
   command above, or 设置 › 机器 in the app), then on the Windows computer:

   ```powershell
   powershell -ExecutionPolicy Bypass -File install-daemon.ps1 -ServerUrl <server url> -MachineId <machine id> -DaemonUrl <https url of team-chat-daemon-<version>-x86_64-pc-windows-msvc.exe> -Sha256 <hex>
   ```

   [`scripts/install-daemon.ps1`](../scripts/install-daemon.ps1) downloads the daemon (https
   only) and checks it against `-Sha256`, which is required for a download: take the hash from
   where you trust the release (the release page, your operator), never from the download
   location itself; the script does not fetch it. It asks for the machine token
   without showing it (or reads `-TokenFile`), writes `%USERPROFILE%\.config\team-chat\daemon.toml`
   and the token into a private folder (your user, SYSTEM and Administrators; nothing inherited), runs `doctor`, and registers the
   scheduled task with `install-service`. `-DaemonFile <path>` takes a daemon that is already on
   the computer. `install-service` refuses a daemon in a folder that other users of the computer
   can change (the task would run whatever they put there as you). The log is
   `%LOCALAPPDATA%\team-chat-daemon\daemon.log`.
4. Make the bot runner-backed:
   `bun server/scripts/admin.ts runner --env <env> --bot <botId> --runtime <claude|codex> --labels <label>`
   (add `--repo-url <git url>` or `--repo-path <path>` for a repository), and check
   `bun server/scripts/admin.ts machines --env <env>` shows the machine healthy.
5. Mention the bot in a topic: its reply streams in.

To update a hand-installed daemon, wait until the machine has no running work (stopping the
daemon cancels its runs), replace the binary, and run `doctor` and `install-service` again.
A Linux daemon installed this way from a release (`scripts/runner/install-daemon.sh`, which
`scripts/runner/host-setup.sh` runs) and started by `install-service` updates itself from the
server's releases once it has no running work; `self_update = false` in `daemon.toml` turns
that off. A host whose service still starts a daemon built by hand (`ExecStart` in
`~/.config/systemd/user/<slug>-daemon.service` names another file) keeps running that one: once,
as the runner user and when the machine has no running work, run
`~/.local/bin/<slug>-daemon doctor && ~/.local/bin/<slug>-daemon install-service`
(`install-daemon.sh` prints this command when it is needed).
On Windows run `install-daemon.ps1 -Update -DaemonUrl <url> -Sha256 <hex>` instead, at any time: it renames
the running program aside and puts the new one in its place, and the daemon restarts on the new
version by itself once it has no running work.

---

# 机器人(中文)

外部机器人是一个 `kind: "bot"` 的成员。它和桌面 App 用同一套 API:WebSocket 收事件、发消息,
HTTP 传文件、产出和一次性发消息。TypeScript SDK 在 `packages/bot-sdk`(`@chat/bot-sdk`),
它是 TypeScript 源码,Bun 和 Node 22.18 以上都能直接运行(Node 22.6 到 22.17 要加
`--experimental-strip-types`)。

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
bun examples/echo-bot.ts          # 或者(Node 22.18 以上):node examples/echo-bot.ts
```

在机器人所在群的任意话题里 @ 它,它会回复 `echo: <内容>`。发 `@Deploy stream ...` 可以看到
流式回复。最小示例见上面英文部分的代码,API 相同。`tokenFile` 和 `stateFile` 开头的 `~` 会当作
用户主目录。参数不对(没有令牌、令牌文件不存在、状态文件损坏、地址不是 `http(s)://主机`)时,
`new Bot(...)` 抛出 `ChatError`(`invalid`),消息里写明是哪个文件或参数。

## 3. SDK 做了什么

- **连接**:每次连接先发 `hello`,每 30 秒 ping 一次,断线后退避重连(0.5 秒到 30 秒,带抖动)。
  令牌被拒绝时停止,报 `ChatError`,code 为 `unauthorized`。`url` 是服务器的基础地址(末尾的 `/v1`
  会去掉)。`start()` 在第一次 welcome 时完成;连不上服务器时会一直重试,每次失败打一条警告
  `cannot reach <ws 地址> (<原因>), retrying in <n> s`。
- **事件有序**:每个群按 `seq` 严格顺序处理,有缺口就用 `fetch` 补齐。重启后从保存的游标
  (`stateFile`)之后读取所有事件,这时处理函数里 `ctx.catchUp = true`。
- **第一次启动**:没有状态文件时跳过旧历史(`firstStart: "latest"`);传 `firstStart: "all"`
  则处理全部历史。之后新加入的群从头读取。
- **处理函数结束后才保存游标**,所以进程崩溃后处理函数会再跑一次。处理函数里的
  `ctx.reply`、`ctx.stream`、`ctx.ask`、`ctx.postCard`、`ctx.done` 用由消息(或卡片事件)id 生成的
  clientMsgId,服务器对重试返回第一次的回复(`duplicate: true`),不会重复发。`bot.send`、`bot.done`
  等 `bot.*` 的发送用随机 id,除非自己传 `clientMsgId`。聊天以外的副作用(部署、调用 API)也要自己做成幂等。
- **处理函数**:`onMention`(@ 我的消息)、`onMessage`(我关注的话题里的其他消息)、
  `onCardAction`(有人按了我的卡片按钮)、`onCardAnswer`(1.1:有人回答了我用 `postCardV2` 发的
  v2 卡片;用 `ctx.markAnswered()` 关闭它)、`onCardEvent`(1.1:我的 v2 卡片的实时事件)、`onArtifactEvent`
  (1.1:我的产出页面发回的事件)、`onAside`(1.1:单独问)、`onEvent`(所有事件)、`onTransient`(输入中、在线、
  流式增量)。一条消息最多触发 `onMention` 和 `onMessage` 中的一个;我自己的消息和系统消息
  都不触发。其他机器人的消息默认也不触发,避免两个机器人互相回复停不下来;传
  `allowBotAuthors: true` 可关闭这条规则。处理函数并发运行,出错交给 `onError`。
- **关注规则**:和服务器一致:我建的话题、我发过言的、@ 过我的、`waitingOn` 里加了我的、我是
  owner 的,都会自动关注。用 `bot.setFollow(...)` 修改。
- **写操作**会等连接可用,重连后重发(`send` 按 `clientMsgId` 幂等);被限流的写操作会重试;
  超过 `requestTimeoutMs`(60 秒)报 `timeout`。发送前先用协议 schema 校验。
- **错误**:调用失败时抛出 `ChatError`,带服务器的 `code`;服务器给了的话还有 `reason`、
  `retryAfterMs`(被限流后要等多久)和 `memberIds`(例如 `relay.member_unavailable` 说的是哪些
  成员)。HTTP 调用还有 `status`。`isRetryable(err)` 对 `network`、`timeout`、`internal`、`rate_limited` 返回 true。
- **流式回复**:`send` 带 `streaming: true`,每 100 毫秒合并发一次 `delta`,最后 `edit` 发完整
  正文并设 `streaming: false`。重连后会补发一次完整文本,读者不会漏内容。消息在最后这次 `edit`
  之前一直是流式状态,所以处理函数没调 `finish()` 就返回或出错时,SDK 会替它结束 `ctx.stream()`
  开的流;`stop()` 会结束所有还开着的流。

方法列表见英文部分的表格。

**进度(运行计划)**:`ctx.setPlan([...])` 设置机器人在这个话题里当前一次运行的计划,成员在话题侧栏
「{bot} 的进度」里看到:完成的打勾、正在做的、等成员的(眼睛)、之后的(虚线圆)。没有运行时会先
开始一次运行;每次调用替换整个计划,`[]` 清空。最多 30 项,每项 1..200 字,状态 `done`、`current`、
`waiting`、`pending`。完成后用 `ctx.endRun("done", "已发出")` 结束。示例见英文部分。有执行器的
机器人调用会得到 `conflict`,它们的计划由执行器从 CLI 自己的待办列表(Claude Code 的 TodoWrite、
Codex 的计划更新)生成。

**1.1:卡片 v2、回答、单独问、页面事件**(需要 1.1 服务器)。三个示例可以直接运行:`examples/ask-bot.ts`
(交付方式、提问、回答、单独问)、`examples/page-bot.ts` 和它的页面 `examples/page-bot.html`(双向页面)、
`examples/report-bot.ts`(原地变化的 2.1 卡片):

- **交付方式**:收到请求先用表情回应(`ctx.ack()`,👀),不要发「收到」;进度放在计划里
  (`ctx.setPlan`),不用流式消息(`stream()` 仍可用,但不再推荐用来汇报工作);最后用
  `ctx.done({ title, summary, evidence, refs, metrics })` 发一张「做完了」卡片(Card v2,intent
  `done`)并结束这次运行,`failed: true` 则以失败结束:卡片开头加一个 `fail` 证据块 `failed`
  (文字为 `failedLabel`,默认「没有完成」),给 1.0.x 客户端的 v1 卡片用 `danger` 色调。
- **首页的一句话(1.1)**:`bot.http.setDeskNote(groupId, text)` 设置机器人在这个群里今天最重要的
  一句话,成员在首页顶部看到(例如「今天先把 1.1 发出去」)。去掉首尾空白后最多 140 个字;`""`
  清除;同样的文字再发一次不保存(`seq: null`)。`deskNotes(groupId)` 列出群里机器人的这句话。
- **卡片构造函数**:`askCard`、`decisionCard`、`approvalCard`、`draftCard`、`formCard`、`doneCard`
  生成合法的 v2 卡片,不合协议时抛出 `ChatError`(`invalid`)。`from` 指定谁能回答(成员 id、
  `"requester"`、`"owners"`),`record: true` 表示答案是群里的决定,`irreversible` 说明不可撤销的
  后果。固定 id 见 `CARD_IDS`:选择块 `choice`、草稿块 `draft`、按钮 `submit`/`approve`/`deny`/
  `cancel`;纯文字选项的 id 是 `o1`、`o2`……
- **回答**:`onCardAnswer` 收到我的 v2 卡片的每个回答(重启后也能收到:内存里没有的卡片,SDK 会在处理函数运行前从历史消息加载,
  所以 `ctx.card`、`ctx.draft()`、`ctx.markAnswered()` 照常可用),旧客户端按 v2 卡片的
  按钮也会到这里(没有输入)。用 `ctx.choice()`、`ctx.number(id)`、`ctx.text(id)`、`ctx.bool(id)`、
  `ctx.draft()` 读取输入,`ctx.cancelled` 表示取消,`ctx.markAnswered()` 把卡片改成已回答(rev 加 1,
  去掉按钮)。输入来自客户端,使用前要检查。
- **单独问**:成员可以在和机器人的私聊里单独问某个话题,这个私聊话题的 `contextOf` 指向原话题和
  读到的位置。`onAside` 收到这些消息(不用 @;没有 `onAside` 时交给 `onMention`),`ctx.source()`
  读取原话题到 `uptoSeq` 为止的消息。用 `ctx.reply` 回答,只留在私聊里;不要往原话题发消息。
- **页面事件(双向页面)**:机器人的产出页面可以和机器人互相通信。`bot.publishPage(target, input)`
  (处理函数里用 `ctx.publishPage(input)`)把一个 HTML 页面发布成这个话题的产出,并记到机器人在这个话题的
  当前运行上;它就是填好话题和 `runId` 的 `http.publishHtml`。`input` 里:
  - `events: [{ name, kind?, label?, schema? }]` 声明页面可以发回的事件。`kind` 为 `emit`(默认,发完就走)
    或 `ask`(页面等回复);`schema` 规定 payload 的形状,不符合的服务器拒收。
  - `data`:第一份数据快照(JSON,最多 256 KiB),页面一打开就有数据,不会先拿到 `null`。
  - `preview: { title?, summary?, metrics? }`:预览卡片上的文字和最多 6 个数字。
  - `artifactId`:给已有的产出加一个版本。

  想让页面直接出现在对话里,发一张带 `blocks.embed({ id, artifactId, version, height?, events? })` 的卡片。
  完整程序见 `examples/page-bot.ts` 和它的页面 `examples/page-bot.html`,精简示例见英文部分 3a。

  `onArtifactEvent(handler)` 收到我的产出的每个页面事件,`onArtifactEvent(name, handler)` 只收这个名字的
  (只在机器人在线时)。`ask` 用 `ctx.reply(payload)` 回答一次(只给提问的人,120 秒内)或
  `ctx.decline(message)` 拒绝。`ctx.data()` 从服务器读这个版本当前的快照 `{ data, dataRev }`:页面的状态放在
  快照里,不要放在进程内存里,这样重启不会丢。`ctx.pushData(data, { baseDataRev? })` 换掉快照并推给所有打开
  这个页面的人(最多 256 KiB,每个版本每秒约 2 次);带 `baseDataRev` 时,如果中间有别的推送,服务器回
  `conflict`,重新读一次再试。`ctx.postMessage` 发消息时把页面事件作为 `causeEventId`。处理函数没有回应时 SDK
  自动回应(`emit` 回 `ignored`,`ask` 回拒绝)。页面事件不能当作批准:需要决定时发 Card v2 提问。

  页面事件不进群的事件记录,机器人不在线时错过的不会补发。重启后用
  `http.artifactEvents(artifactId, { before?, limit? })` 读页面记录(新的在前,保留 180 天),用
  `http.getArtifactData(artifactId, version)` 读状态;没有连接的脚本用 `http.putArtifactData` 写快照。

  `schema` 的 `pattern` 不能用反向引用和前后断言,被检查的字符串最多 4096 个字符;SDK 在上传前按服务器的规则
  检查声明和 `data` 的大小,不合规时抛出 `ChatError`(`invalid`,reason `artifact.bad_schema`;或 `too_large`)。
- **页面这一边:`window.teamchat`**:页面自己的脚本运行之前,`window.teamchat` 已经在页面里了。在普通
  浏览器里没有它,用 `window.teamchat?.` 判断一下,页面就还能直接打开。
  - `teamchat.version`:`1`。`teamchat.viewer`:正在看页面的成员 `{ memberId, name }`,或 `null`。
  - `teamchat.can`:`{ emit, ask, events: [{ name, kind }] }`,这份页面现在能不能发 `emit`、`ask`
    (版本没有声明这类事件、版本是人发的、发它的机器人已离开或被停用时为 `false`),以及声明了哪些事件。
  - `teamchat.getData()`:`Promise<data>`,这个版本当前的快照(没有时为 `null`)。
  - `teamchat.onData((data, meta) => …)`:先用当前快照调用一次,以后每次推送再调用。`meta` 是
    `{ rev, cause? }`,推送由页面事件引起时 `cause` 是 `{ eventId, actorId }`。返回一个取消订阅的函数。
  - `teamchat.emit(name, payload?)`:发一个声明过的 `emit` 事件,`Promise<{ eventId }>`,服务器收下就完成,
    不等机器人回应。
  - `teamchat.ask(name, payload?, { timeoutMs? })`:发一个声明过的 `ask`,完成时得到机器人的回复。默认等
    30 秒,最多 120 秒;一个页面最多同时有 4 个没回的 `ask`。
  - `teamchat.ref({ label, value?, selector? })`:把页面的一部分放进成员的输入框(引用提问);`label` 1 到 80 个字,
    `value` 最多 2048 字节,每秒最多 2 次。

  `emit` 和 `ask` 只在成员自己刚操作过页面(点击、拖动或按键后 2 秒内)时才发得出去,否则以 `no_user_action`
  失败:在事件处理函数里调用,不要放在定时器或页面加载时。payload 是 JSON,最多 16 KiB。发得比 App 的节奏
  快时,同名的后一个 `emit` 顶掉还在等的那个,被顶掉的以 `rate_limited` 失败。失败的调用得到带 `code` 的
  `Error`:`not_allowed`、`no_user_action`、`bad_payload`(payload 不符合事件的 schema,`message` 说明位置)、`bad_request`、`too_large`、`rate_limited`、`offline`、`timeout`(`ask` 没等到回复)、
  `declined`(机器人 `ctx.decline`)、`failed`(机器人的处理函数出错,或服务器拒收)、`reloaded`(页面重新加载了)、
  `unknown_method`。带 `data-ref-label="…"` 的元素可以被指着提问:⌥ 点击(Windows 上是 Alt)或右键菜单把它
  放进输入框,`data-ref-value` 是它的值(JSON,否则当作文字;没有时取元素的文字,最多 200 个字)。
  `window.artifact`(`getData`、`emitEvent`、`ref`、`call`)是 1.0 的旧名字,仍然可用;新页面用 `teamchat.emit`。
- **交互闭环(2.1)**:块用 `events` 声明实时事件(如 `choice.change`、`chart.select`)。
  `onCardEvent` 收到实时事件,`onCardAnswer` 收到已存的回答(重启后也能收到)。
  `onCardEvent("button.press", handler)` 只收这个名字的事件,`onCardEvent({ name?, block? }, handler)`
  只收这个名字、这个块的事件。用 `ctx.choice()`、`ctx.choices()`(选择题、`compare`)、`ctx.button()`
  (`buttons` 被按的按钮 id)、`ctx.rows()`(表格选中的行)、`ctx.tab()`、`ctx.point()`(图上的点
  `{ series, x, y, index }`)、`ctx.sort()` 读取事件的值:形状不对时返回 `undefined` 或 `[]`,不用自己给
  `ctx.value` 做类型断言。每个事件由哪种块发、值是什么,见英文部分 3b 的表格。用 `ctx.patch(ops)`
  原地修改卡片(SDK 自动带 `baseRev` 和 `cause`;`card.stale_rev` 时在最新 rev 上重试,最多 3 次;
  被限流时合并排队,每秒最多一次,只合并 cause 相同或没有 cause 的修改;合并的修改超时或遇到 `internal` 时,用同一个 `clientPatchId` 重发同一帧,`duplicate: true` 算作已应用),或用 `ctx.reply`、`ctx.postCard`、`ctx.resolve("ignored")` 回应。
  传函数时拿到最新 rev 的 `CardDraft`:`set`、`replace`、`remove` 和 `insert` 的位置写了卡片里没有的块 id,
  会在发送前抛出 `ChatError` `invalid`,并列出卡片现有的块 id;`d.set<"metric">(id, { value })` 按块类型检查字段;
  `d.actions(list)` 替换卡片的按钮,`[]` 去掉按钮。
  `ctx.done(card)` 发「做完了」卡片作为回应并结束运行;`ctx.setPlan`、`ctx.endRun`、`ctx.setAcceptance`
  在卡片所在的话题里和消息处理函数里一样可用。
  实时事件的处理函数没有回应时,SDK 自动发 `ignored`(出错时发 `failed`)。服务器只接受 30 分钟内的
  cause:重启后读到的更早的回答,SDK 回应时不带 cause。
- **Card v2.1 构造函数**:`composeCard({ intent, title, blocks, ... })` 和 `blocks.*`:容器 `stack`、
  `row`、`group`、`compare`、`tabs`、`questionnaire`(子块可以直接传块对象),选择题 `decision`
  (`choice`)、`rank`、`rating`、`scale`、`confirm`、`imageChoice`、`matrix`、`slots`、`poll`、滑块
  `slider`,以及 `buttons`、`diagram`、`embed`。卡片里不能写样式。`blocks.buttons` 默认声明
  `button.press`(不声明的话服务器不收按钮的点击);块声明了它这种类型发不出的事件时,`composeCard` 抛出
  `ChatError` `invalid` 并说明这种块能发哪些。卡片不合协议时,错误最多列出 5 个问题,每个都带块的 id 和类型。
  完整示例(折线图 +「这个数据对不对？」)见英文部分 3b,可以直接运行的程序见 `examples/report-bot.ts`。
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
不重复发消息,以及 1.1 的卡片回答、交付卡片、单独问、页面事件和交互闭环(服务器缺少其中任何
一部分,测试会失败并指出缺的是哪一部分)。`bun test test/unit.test.ts` 用假服务器检查同样的逻辑,
不启动 wrangler;`test/artifacts.test.ts`(页面、数据快照、页面记录、实时事件的值)和
`test/card-checks.test.ts`(块能发的事件、卡片错误信息、`CardDraft` 的 id 检查)同理。
`test/examples.test.ts` 用 `examples/report-bot.ts` 和 `examples/page-bot.ts` 的真实处理函数跑一遍
(每张卡片、每次修改都过服务器自己的规则,页面里的每个 `teamchat.emit`、`teamchat.ask` 都必须声明过);
`test/docs-examples.test.ts` 对本文和 SDK README 里的每个 `ts` 代码块做类型检查,并运行卡片和页面示例。
`bun run test:node`(即 `bun test/node-smoke.ts`)用 Node 22.18 或更新版本运行 echo 示例。

## 6. 在电脑上干活的机器人(runner)

这类机器人用一台装了机器 daemon 的电脑跑编程命令行工具(Claude Code、Codex 等),见
[machine-protocol.md](machine-protocol.md)。每台电脑做一次:

1. 在那台电脑上装好命令行工具并登录(终端里 `claude --version` 或 `codex --version` 能运行)。
2. 简单做法(1.2.1 起):在那台电脑上装 App,用 owner 账号登录,在「设置 › 机器」里点「用这台电脑」。
   App 会登记电脑、写好 daemon 配置和令牌、装好服务(Windows 上是当前用户的计划任务),以后跟着 App 自动更新。
3. 手动做法:`scripts/build-daemon.sh` 编译(或从发布页下载)daemon,
   `bun server/scripts/admin.ts machine --env <env> --name <名字> --labels <标签>` 登记电脑并保存令牌文件,
   把程序和令牌拷到那台电脑,写 `~/.config/team-chat/daemon.toml`(`team-chat-daemon example-config`
   打印示例),再在能找到命令行工具的终端里运行 `team-chat-daemon doctor` 和 `team-chat-daemon install-service`。

   Windows 上手动做法是一条 PowerShell 命令,用 bot 要借用其命令行登录的那个用户运行(不需要管理员权限)。
   先登记电脑(上面的 `admin.ts machine`,或 App 的「设置 › 机器」),再在那台 Windows 电脑上运行:

   ```powershell
   powershell -ExecutionPolicy Bypass -File install-daemon.ps1 -ServerUrl <服务器地址> -MachineId <机器 id> -DaemonUrl <team-chat-daemon-<版本>-x86_64-pc-windows-msvc.exe 的 https 地址> -Sha256 <hex>
   ```

   [`scripts/install-daemon.ps1`](../scripts/install-daemon.ps1) 下载 daemon(只接受 https)并用 `-Sha256`
   校验。下载时 `-Sha256` 必填:哈希要从你信任的地方拿(发布页、运营方),不能从下载地址旁边拿,脚本也
   不会去取。daemon 所在的文件夹如果别的用户能改,`install-service` 会拒绝。脚本不回显地问机器令牌(或用 `-TokenFile` 读文件),把
   `%USERPROFILE%\.config\team-chat\daemon.toml` 和令牌写进只有当前用户、SYSTEM 和 Administrators 能打开的文件夹,运行 `doctor`,
   再用 `install-service` 注册计划任务。`-DaemonFile <路径>` 用电脑上已有的 daemon。日志在
   `%LOCALAPPDATA%\team-chat-daemon\daemon.log`。以后更新运行 `install-daemon.ps1 -Update -DaemonUrl <地址> -Sha256 <hex>`:
   随时可以运行,daemon 等手上没有活的时候自己换到新版本。
4. 打开机器人的 runner:
   `bun server/scripts/admin.ts runner --env <env> --bot <botId> --runtime <claude|codex> --labels <标签>`,
   再用 `bun server/scripts/admin.ts machines --env <env>` 确认电脑健康。
5. 在话题里 @ 这个机器人,它的回复会一段段出来。
