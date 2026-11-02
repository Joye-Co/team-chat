// Live check helper for the desktop cards (see provider/cards.rs, test `live_cards`).
//
// A bot that uploads a small PNG, posts one card with every block type and
// three buttons into a new topic of its first group, writes the ids to
// $CARDS_INFO, and answers a press of "approve" by replacing the card
// (status 已处理, no buttons). It exits after $CARDS_BOT_SECONDS (default 120).
//
//   CHAT_URL=http://127.0.0.1:8803 CHAT_TOKEN_FILE=<bot token file> CARDS_INFO=<json path> \
//   bun apps/desktop/src/views/cards/live_bot.ts
import { writeFileSync } from "node:fs";
import { Bot, type Card } from "../../../../../packages/bot-sdk/src/index.ts";

const url = process.env.CHAT_URL ?? "http://127.0.0.1:8803";
const tokenFile = process.env.CHAT_TOKEN_FILE;
const infoPath = process.env.CARDS_INFO;
if (!tokenFile || !infoPath) throw new Error("set CHAT_TOKEN_FILE and CARDS_INFO");

// A 2x2 red PNG.
const png = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP4z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==",
  ),
  (c) => c.charCodeAt(0),
);

const bot = new Bot({ url, tokenFile, firstStart: "latest", logLevel: "warn" });
let card: Card | undefined;

bot.onCardAction(async (ctx) => {
  if (ctx.actionId !== "approve" || !card) return;
  const next: Card = { ...card, tone: "success", status: { label: "已处理" } };
  delete next.actions;
  await ctx.updateCard(next);
});

await bot.start();
const group = bot.groupSummaries[0];
if (!group) throw new Error("the bot is in no group");
const groupId = group.group.id;
const file = await bot.uploadFile({ groupId, name: "dot.png", mime: "image/png", data: png, width: 2, height: 2 });
card = {
  v: 1,
  title: "发布 v1.0 到预发环境",
  tone: "info",
  status: { label: "等待批准" },
  blocks: [
    { type: "text", text: "包含 **3 个** 修复。" },
    { type: "fields", fields: [{ label: "版本", value: "v1.0" }] },
    { type: "table", columns: ["文件", "结果"], rows: [["a.rs", "通过"]] },
    { type: "list", items: [{ text: "构建", status: "done" }, { text: "部署", status: "todo" }] },
    { type: "progress", value: 0.5, label: "一半" },
    { type: "code", lang: "sh", text: "cargo test" },
    { type: "image", fileId: file.fileId, alt: "红点" },
    { type: "divider" },
    { type: "link", label: "日志", target: { kind: "url", url: "https://example.com/log" } },
  ],
  actions: [
    { id: "approve", label: "批准", style: "primary" },
    { id: "later", label: "稍后" },
    { id: "reject", label: "拒绝", style: "danger", confirm: "确定拒绝？" },
  ],
};
const sent = await bot.postCard({ groupId }, card, { title: "卡片实测" });
writeFileSync(
  infoPath,
  JSON.stringify({ groupId, topicId: sent.message.topicId, messageId: sent.message.id, botId: bot.me.id }),
);
console.log("card posted");
setTimeout(async () => {
  await bot.stop();
  process.exit(0);
}, Number(process.env.CARDS_BOT_SECONDS ?? 120) * 1000);
