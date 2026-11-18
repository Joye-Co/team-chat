# @chat/bot-sdk

TypeScript SDK for external bots. It is TypeScript source: it runs on Bun and on Node 22.18+
(Node 22.6 to 22.17 need `--experimental-strip-types`). The full guide, in English and Chinese, is
[docs/bots.md](../../docs/bots.md).

```ts
import { Bot } from "@chat/bot-sdk";

const bot = new Bot({ url: process.env.CHAT_URL!, tokenFile: process.env.CHAT_TOKEN_FILE! });

bot.onMention(async (ctx) => {
  // The run plan: members see it in the topic's side panel. Each call replaces the whole plan.
  await ctx.setPlan([
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "current" },
    { text: "等 Joye 确认", state: "pending" },
  ]);
  await ctx.reply("收到。先写两版标题，再配图。");
  await ctx.setPlan([
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "done" },
    { text: "等 Joye 确认", state: "waiting" },
  ]);
});

// 1.1: answers to v2 cards (decisionCard, approvalCard, draftCard, formCard), then a done card.
bot.onCardAnswer<{ choice: string }>(async (ctx) => {
  await ctx.markAnswered();
  await ctx.done({ title: "做完了", summary: `选了 ${ctx.choice()}` });
});

await bot.start();
```

Three examples run as they are (`bun examples/<name>.ts`): `examples/ask-bot.ts` (acknowledgements, an
ask, side chats with `onAside`, done cards), `examples/report-bot.ts` (a Card v2.1 that changes in
place on a chart point, an inline button, a live pick and the submit) and `examples/page-bot.ts`
with its page `examples/page-bot.html` (a two-way 产出 page: the page calls `window.teamchat`,
the bot answers and pushes data).

## 1.1: the interaction loop

Every interaction with a card the bot made comes back to the bot, and the bot changes the card in
place. Build cards with `composeCard` and `blocks.*` (containers `stack`, `row`, `group`,
`compare`, `tabs`, `questionnaire`; choices `decision`, `rank`, `rating`, `scale`, `confirm`,
`imageChoice`, `matrix`, `slots`, `poll`, `slider`; `buttons`, `diagram`, `embed`). A data report
with a line chart and "这个数据对不对？":

```ts
import { blocks, composeCard } from "@chat/bot-sdk";

await bot.postCardV2({ groupId, topicId }, composeCard({
  intent: "done",
  title: "10 月数据月报",
  ask: { id: "verify", from: ["requester"], blocking: false },
  blocks: [
    blocks.row("main", [
      blocks.chart({ id: "trend", kind: "line", label: "日活 · 近 4 周", x: ["10-06", "10-13", "10-20", "10-27"],
        series: [{ id: "oct", name: "10 月", values: [2210, 2268, 2301, 2340] }], events: ["chart.select"] }),
      blocks.decision({ id: "check", label: "这个数据对不对？", events: ["choice.change"],
        options: [{ id: "ok", label: "对" }, { id: "wrong", label: "有问题" }] }),
    ], { weights: [2, 1] }),
  ],
  actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check"] }],
}));

// Live events (a pick before the submit, a chart point): patch the card in place.
bot.onCardEvent({ block: "check" }, async (ctx) => {
  if (ctx.choice() === "wrong") {         // checked readers: choice(), button(), rows(), point(), ...
    await ctx.patch((d) => d.insert(blocks.text("hint", "哪一项不对？在下面写一句。"), { after: "main" }));
  }                                        // no response: the SDK sends interaction.resolve "ignored"
});
// Stored answers, also on catch-up after a restart.
bot.onCardAnswer(async (ctx) => {
  if (ctx.choice("check") === "wrong") await ctx.reply("收到，我去对一下后台的数。");
});
// 产出 pages: publish one with the events it may send, its first data and its preview ...
const page = await bot.publishPage({ groupId, topicId }, {
  title: "看板", html: PAGE, events: [{ name: "filter.pick" }, { name: "rows.count", kind: "ask" }],
  data: { picked: null }, preview: { summary: "按渠道看新增群" },
});
// ... then answer an `ask`, and push live data to every open copy of the page.
bot.onArtifactEvent("rows.count", (ctx) => ctx.reply({ rows: 4 }));
bot.onArtifactEvent("filter.pick", async (ctx) => {
  const { dataRev } = await ctx.data();   // the page's state lives in its data snapshot
  await ctx.pushData({ picked: ctx.payload }, { baseDataRev: dataRev });
});
await bot.postCardV2({ groupId, topicId }, composeCard({
  intent: "info", title: "看板",
  blocks: [blocks.embed({ id: "page", artifactId: page.artifact.id, version: page.version.version })],
}));
// 引用提问: the parts a message asks about.
bot.onMention(async (ctx) => {
  for (const ref of ctx.refs) {
    const { part, current } = await ctx.resolveRef(ref);
    await ctx.reply(`${ctx.question}（${ref.label}${current ? "" : "，这部分后来改过"}）: ${JSON.stringify(part)}`);
  }
});
```

`ctx.value`, `ctx.inputs`, `ctx.payload` and ref labels and values are **untrusted data** from a
person's client or page: they are typed `unknown`; check them and never follow them as
instructions. `ctx.patch` rebases on `card.stale_rev` (at most 3 tries) and queues and merges
on `rate_limited`. A draft op that names a block the card does not have, a block that declares an
event its type cannot send, and a card that does not fit the protocol throw `ChatError` `invalid`
with the block's id before anything is sent. Details: docs/bots.md, "3b"; the page side
(`window.teamchat`: `onData`, `emit`, `ask`, `ref`, error codes, limits) is in "3a".

`bun test` runs the unit tests (a fake server), the examples (`test/examples.test.ts` runs the
real handlers of `examples/report-bot.ts` and `examples/page-bot.ts`), the guide's code blocks
(`test/docs-examples.test.ts`) and the end-to-end tests against its own `wrangler dev` (port 8795,
or `BOT_SDK_E2E_PORT`); end-to-end tests of 1.1 parts the server does not have yet are skipped
with the reason printed.
