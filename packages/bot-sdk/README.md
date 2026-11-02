# @chat/bot-sdk

TypeScript SDK for external bots (Bun and Node 22+). The full guide, in English and Chinese, is
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

await bot.start();
```

`bun test` runs the end-to-end tests against its own `wrangler dev` (port 8795, or
`BOT_SDK_E2E_PORT`).
