#!/usr/bin/env bun
// Page bot: a two-way 产出 page. Mention it and it publishes a sign-up sheet (page-bot.html) and
// shows it inline in a card. The page and the bot talk both ways:
//
//   page -> bot   teamchat.emit("signup.join" | "signup.leave"), teamchat.ask("signup.seats")
//   bot -> page   ctx.pushData(...) to every open copy, ctx.reply(...) to the one who asked
//
// The list lives in the version's data snapshot on the server, not in this process: a handler
// reads it (`ctx.data()`), changes it and pushes it back with `baseDataRev`, so a restart loses
// nothing and two sign-ups at once cannot overwrite each other.
//
//   CHAT_URL=http://127.0.0.1:8789 \
//   CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-page-<id>.token \
//   bun examples/page-bot.ts
//
// Optional: CHAT_STATE_FILE (default: <token file>.state.json), LOG_LEVEL (debug|info|warn|error).
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import {
  type ArtifactEventContext,
  type ArtifactEventDecl,
  blocks,
  Bot,
  ChatError,
  composeCard,
  type LogLevel,
} from "../packages/bot-sdk/src/index.ts";

/** The page: one self-contained HTML file. */
export const PAGE = readFileSync(new URL("./page-bot.html", import.meta.url), "utf8");

/**
 * What the page may send. The server refuses any other name, a name of the other kind, and a
 * payload that does not fit `schema`; the handlers still check what they read.
 */
export const EVENTS: ArtifactEventDecl[] = [
  { name: "signup.join", label: "报名", schema: { type: "object", properties: { note: { type: "string", maxLength: 40 } } } },
  { name: "signup.leave", label: "取消报名" },
  { name: "signup.seats", kind: "ask", label: "问还有没有名额" },
];

interface Person {
  memberId: string;
  name: string;
  note?: string;
}
/** The data snapshot of the sheet: what every open copy of the page draws. */
export interface Sheet {
  title: string;
  when: string;
  /** How many may sign up. */
  seats: number;
  people: Person[];
}

/** A snapshot read back from the server, as a sheet. It is this bot's own data, but check it. */
function sheetOf(data: unknown): Sheet {
  const d = (data && typeof data === "object" ? data : {}) as Partial<Sheet>;
  const people = Array.isArray(d.people) ? d.people.filter((p): p is Person => typeof p?.memberId === "string" && typeof p?.name === "string") : [];
  const seats = typeof d.seats === "number" && Number.isInteger(d.seats) && d.seats > 0 ? d.seats : 12;
  return { title: typeof d.title === "string" ? d.title : "团建报名", when: typeof d.when === "string" ? d.when : "", seats, people };
}

/**
 * Reads the sheet, applies `change` and pushes the result to every open copy of the page. When
 * another push came in between (`conflict`), it reads again and tries once more.
 */
async function update(ctx: ArtifactEventContext, change: (sheet: Sheet) => Sheet): Promise<Sheet> {
  for (let attempt = 1; ; attempt++) {
    const { data, dataRev } = await ctx.data();
    const next = change(sheetOf(data));
    try {
      await ctx.pushData(next, { baseDataRev: dataRev });
      return next;
    } catch (err) {
      if (attempt >= 2 || !(err instanceof ChatError && err.code === "conflict")) throw err;
    }
  }
}

export function attach(bot: Bot): void {
  bot.onMention(async (ctx) => {
    await ctx.ack();
    const sheet: Sheet = { title: "团建报名", when: "周六 14:00 · 奥森公园南门", seats: 12, people: [] };
    // The page with its first snapshot and its preview, made by this bot's run in the topic.
    const page = await ctx.publishPage({
      title: sheet.title,
      html: PAGE,
      events: EVENTS,
      data: sheet,
      preview: { title: sheet.title, summary: "点开报名，名单大家都看得到。", metrics: [{ label: "时间", value: "周六 14:00" }] },
    });
    // The page itself, live in the conversation. `events` names the page events the embedded copy
    // may send; each must be declared on the version.
    await ctx.postCard(
      composeCard({
        intent: "info",
        title: sheet.title,
        summary: sheet.when,
        blocks: [
          blocks.embed({
            id: "sheet",
            artifactId: page.artifact.id,
            version: page.version.version,
            height: 360,
            title: sheet.title,
            events: EVENTS.map((e) => e.name),
          }),
        ],
      }),
    );
  });

  // An emit: someone signs up. `actorId` comes from the server; `payload` comes from the page.
  bot.onArtifactEvent("signup.join", async (ctx) => {
    const raw = (ctx.payload as { note?: unknown } | null)?.note;
    const note = typeof raw === "string" ? raw.trim().slice(0, 40) : "";
    const name = bot.member(ctx.actorId)?.name ?? "成员";
    await update(ctx, (sheet) => {
      const others = sheet.people.filter((p) => p.memberId !== ctx.actorId);
      if (others.length >= sheet.seats) return sheet; // full: the push still answers the emit, and the page shows the same list
      return { ...sheet, people: [...others, note ? { memberId: ctx.actorId, name, note } : { memberId: ctx.actorId, name }] };
    });
  });

  bot.onArtifactEvent("signup.leave", async (ctx) => {
    await update(ctx, (sheet) => ({ ...sheet, people: sheet.people.filter((p) => p.memberId !== ctx.actorId) }));
  });

  // An ask: the page waits for the reply, and only the member who asked gets it.
  bot.onArtifactEvent("signup.seats", async (ctx) => {
    const sheet = sheetOf((await ctx.data()).data);
    await ctx.reply({ left: Math.max(0, sheet.seats - sheet.people.length) });
  });
  // A page event never approves anything. To decide something, post a Card v2 ask.
  //
  // Page events reach the bot only while it is connected. What it missed while it was down is in
  // the page log: `bot.http.artifactEvents(artifactId)` lists it, newest first.
}

function expand(path: string): string {
  return path.startsWith("~/") ? `${homedir()}${path.slice(1)}` : path;
}

if (import.meta.main) {
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
    client: { name: "page-bot", version: "1.1.0" },
    logLevel: (process.env.LOG_LEVEL as LogLevel | undefined) ?? "info",
  });
  attach(bot);
  bot.onConnection((up) => bot.log.info(up ? "online" : "offline, reconnecting"));
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.on(signal, () => {
      void bot.stop().then(() => process.exit(0));
    });
  }
  await bot.start();
  bot.log.info("page bot ready", { member: bot.me.name });
}
