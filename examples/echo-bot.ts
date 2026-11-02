#!/usr/bin/env bun
// Echo bot: replies to every mention in the same topic, and streams when asked.
//
//   CHAT_URL=http://127.0.0.1:8789 \
//   CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-echo-<id>.token \
//   bun examples/echo-bot.ts
//
// Optional: CHAT_STATE_FILE (default: <token file>.state.json), LOG_LEVEL (debug|info|warn|error).
import { homedir } from "node:os";
import { Bot, type LogLevel } from "../packages/bot-sdk/src/index.ts";

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
  client: { name: "echo-bot", version: "1.0.0" },
  logLevel: (process.env.LOG_LEVEL as LogLevel | undefined) ?? "info",
});

bot.onMention(async (ctx) => {
  // Drop the leading @name so the echo shows only the request.
  const text = ctx.message.body.replace(/^@\S+\s*/, "").trim();
  if (text.startsWith("stream")) {
    const w = await ctx.stream();
    for (const word of `Streaming back: ${text}`.split(" ")) {
      w.append(`${word} `);
      await new Promise((r) => setTimeout(r, 120));
    }
    await w.finish();
    return;
  }
  await ctx.reply(text ? `echo: ${text}` : "echo");
});

bot.onConnection((up) => bot.log.info(up ? "online" : "offline, reconnecting"));

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void bot.stop().then(() => process.exit(0));
  });
}

await bot.start();
bot.log.info("echo bot ready", { member: bot.me.name });
