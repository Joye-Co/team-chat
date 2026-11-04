#!/usr/bin/env bun
// Ask bot: the 1.1 way to work in a topic. It reacts instead of writing "on it", keeps its
// progress in the plan, asks with a Card v2 decision, and ends with one done card with proof.
// In a side chat (单独问) it answers privately about the topic it is asked about.
//
//   CHAT_URL=http://127.0.0.1:8789 \
//   CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-ask-<id>.token \
//   bun examples/ask-bot.ts
//
// Optional: CHAT_STATE_FILE (default: <token file>.state.json), LOG_LEVEL (debug|info|warn|error).
import { homedir } from "node:os";
import { Bot, decisionCard, type LogLevel } from "../packages/bot-sdk/src/index.ts";

function expand(path: string): string {
  return path.startsWith("~/") ? `${homedir()}${path.slice(1)}` : path;
}

const url = process.env.CHAT_URL;
const tokenFile = process.env.CHAT_TOKEN_FILE && expand(process.env.CHAT_TOKEN_FILE);
if (!url || !tokenFile) {
  console.error("set CHAT_URL and CHAT_TOKEN_FILE");
  process.exit(2);
}

const bot = new Bot({
  url,
  tokenFile,
  stateFile: expand(process.env.CHAT_STATE_FILE ?? `${tokenFile}.state.json`),
  client: { name: "ask-bot", version: "1.1.0" },
  logLevel: (process.env.LOG_LEVEL as LogLevel | undefined) ?? "info",
});

bot.onMention(async (ctx) => {
  await ctx.ack(); // 👀 on the message: seen, working on it
  await ctx.setPlan([
    { text: "读需求", state: "done" },
    { text: "等你选方案", state: "waiting" },
    { text: "按方案做", state: "pending" },
  ]);
  await ctx.ask(
    decisionCard({
      title: "用哪个方案?",
      text: "两个方案都能做,差在时间。",
      options: [
        { id: "fast", label: "先上线简单版", detail: "今天能好" },
        { id: "full", label: "做完整版", detail: "要两天" },
      ],
      from: [ctx.message.authorId],
      record: true, // the answer is a group decision (群里的决定)
      cancelLabel: "先不做",
    }),
  );
  await ctx.updateTopic({ status: "waiting_answer", waitingOn: [ctx.message.authorId], ownerId: bot.me.id });
});

bot.onCardAnswer<{ choice: string }>(async (ctx) => {
  await ctx.markAnswered(); // the card shows as answered: a higher rev and no buttons
  const target = { groupId: ctx.groupId, topicId: ctx.topicId };
  if (ctx.cancelled) {
    await bot.done(target, { title: "不做了", summary: "按你的意思,先不做。" });
    await ctx.updateTopic({ status: "done", waitingOn: [] });
    return;
  }
  const plan = ctx.choice() === "full" ? "完整版" : "简单版";
  await ctx.updateTopic({ status: "in_progress", waitingOn: [] });
  await bot.setPlan(target, [
    { text: "读需求", state: "done" },
    { text: "等你选方案", state: "done" },
    { text: `做${plan}`, state: "current" },
  ]);
  // ... do the work here ...
  await bot.done(target, {
    title: "做完了",
    summary: `${plan}做好了。`,
    evidence: [{ label: "测试全部通过", state: "pass" }],
  });
  await ctx.updateTopic({ status: "waiting_review", waitingOn: [ctx.actorId] });
});

bot.onAside(async (ctx) => {
  // A member asks privately about another topic: read it up to where they are, answer here.
  const messages = await ctx.source({ limit: 50 });
  const last = messages.at(-1);
  await ctx.reply(last ? `这个话题共 ${messages.length} 条,最新一条是:${last.body.slice(0, 200)}` : "这个话题还没有消息。");
});

bot.onConnection((up) => bot.log.info(up ? "online" : "offline, reconnecting"));

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void bot.stop().then(() => process.exit(0));
  });
}

await bot.start();
bot.log.info("ask bot ready", { member: bot.me.name });
