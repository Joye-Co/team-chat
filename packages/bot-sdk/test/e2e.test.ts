// End-to-end tests of the bot SDK against a local `wrangler dev` of server/ (port 8795).
// Run: bun test (in packages/bot-sdk). Set BOT_SDK_E2E_PORT to use another port.
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Card, Event, Message } from "@chat/protocol";
import { Bot, ChatError, HttpClient, MemoryStateStore, type StreamWriter } from "../src/index.ts";

const PORT = Number(process.env.BOT_SDK_E2E_PORT ?? 8795);

import { localServer } from "./server.ts";

const server = localServer(PORT);
await server.start();

// Bootstrap an owner (the "human" side) and a bot member.
async function post(path: string, body: unknown, headers: Record<string, string>): Promise<any> {
  const r = await fetch(`${server.url}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
  const data = await r.json();
  if (!r.ok) throw new Error(`${path} -> ${r.status} ${JSON.stringify(data)}`);
  return data;
}
const boot = await post("/v1/admin/bootstrap", { ownerName: "Joye" }, { "x-admin-secret": server.adminSecret });
const ownerToken: string = boot.token;
const generalId: string = boot.group.id;
const created = await post("/v1/bots", { name: "Echo", role: "test" }, { authorization: `Bearer ${ownerToken}` });
const botId: string = created.member.id;
const tokenFile = join(server.dir, "echo.token");
writeFileSync(tokenFile, `${created.token}\n`, { mode: 0o600 });
chmodSync(tokenFile, 0o600);
const stateFile = join(server.dir, "echo.state.json");

// Probe optional V1 features on this server build.
const probeHttp = new HttpClient({ url: server.url, token: ownerToken });
async function routeExists(method: string, path: string): Promise<boolean> {
  try {
    await probeHttp.request(method, path, { json: {} });
    return true;
  } catch (err) {
    return !(err instanceof ChatError && err.code === "not_found" && /no such route/.test(err.message));
  }
}
const hasHttpSend = await routeExists("POST", `/v1/groups/${generalId}/messages`);
const hasFiles = await routeExists("POST", "/v1/files");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** The human side, built on the SDK too. Records every event and transient it sees. */
const events: Event[] = [];
const deltas: { messageId: string; append: string }[] = [];
const human = new Bot({ url: server.url, token: ownerToken, logLevel: "warn", markRead: false });
human.onEvent(({ event }) => {
  if (event) events.push(event);
});
human.onTransient((e) => {
  if (e.type === "message.delta") deltas.push({ messageId: e.messageId, append: e.append });
});

async function waitFor<T>(what: string, fn: () => T | undefined | null | false, ms = 15_000): Promise<T> {
  const until = Date.now() + ms;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > until) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 20));
  }
}

function botMessages(topicId: string): Message[] {
  return events
    .filter((e): e is Extract<Event, { type: "message.created" }> => e.type === "message.created")
    .map((e) => e.data.message)
    .filter((m) => m.authorId === botId && m.topicId === topicId);
}

function mention(body: string) {
  return human.startTopic(generalId, { body: `@Echo ${body}`, mentions: [botId] });
}

/** The echo bot under test: replies to mentions, streams on "stream", marks status on "status". */
function makeEchoBot(seen: { mentions: string[]; followed: string[]; catchUp: string[] }) {
  const bot = new Bot({ url: server.url, tokenFile, stateFile, logLevel: "warn" });
  bot.onMention(async (ctx) => {
    seen.mentions.push(ctx.message.id);
    if (ctx.catchUp) seen.catchUp.push(ctx.message.id);
    const text = ctx.message.body.replace(/^@Echo\s*/, "");
    if (text.startsWith("stream")) {
      const w: StreamWriter = await ctx.stream();
      for (const part of ["one ", "two ", "three"]) {
        w.append(part);
        await new Promise((r) => setTimeout(r, 150));
      }
      await w.finish();
      return;
    }
    if (text.startsWith("status")) {
      await ctx.updateTopic({ status: "in_progress" });
    }
    await ctx.reply(`echo: ${text}`);
  });
  bot.onMessage(async (ctx) => {
    seen.followed.push(ctx.message.id);
    await ctx.reply(`follow-up seen: ${ctx.message.body}`);
  });
  return bot;
}

const seen = { mentions: [] as string[], followed: [] as string[], catchUp: [] as string[] };
let bot = makeEchoBot(seen);

beforeAll(async () => {
  await human.start();
  await bot.start();
});

afterAll(async () => {
  await bot.stop();
  await human.stop();
  await server.dispose();
});

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bot sdk e2e", () => {
  test("connects with a token file and sees the default group", () => {
    expect(bot.me.id).toBe(botId);
    expect(bot.me.kind).toBe("bot");
    expect(bot.groupSummaries.map((s) => s.group.id)).toContain(generalId);
  });

  test("a bad token fails start with unauthorized", async () => {
    const bad = new Bot({ url: server.url, token: "tc_notarealtokennotarealtokennotarealtoken12", logLevel: "silent" });
    const err = await bad.start().catch((e) => e);
    expect(err).toBeInstanceOf(ChatError);
    expect((err as ChatError).code).toBe("unauthorized");
    await bad.stop();
  });

  test("mention gets one reply in the same topic", async () => {
    const r = await mention("hello");
    const topicId = r.message.topicId;
    const replies = await waitFor("echo reply", () => botMessages(topicId).length > 0 && botMessages(topicId));
    expect(replies[0].body).toBe("echo: hello");
    expect(seen.mentions).toContain(r.message.id);
    await new Promise((r) => setTimeout(r, 300));
    expect(botMessages(topicId)).toHaveLength(1);
  });

  test("a message in a topic the bot follows reaches onMessage", async () => {
    const r = await mention("start a thread");
    const topicId = r.message.topicId;
    await waitFor("first reply", () => botMessages(topicId).length === 1);
    const follow = await human.send({ groupId: generalId, topicId }, "no mention here");
    await waitFor("follow-up reply", () => botMessages(topicId).length === 2);
    expect(seen.followed).toContain(follow.message.id);
    expect(botMessages(topicId)[1].body).toBe("follow-up seen: no mention here");
    // A message in a topic the bot does not follow is not delivered.
    const other = await human.startTopic(generalId, "unrelated topic");
    await new Promise((r) => setTimeout(r, 500));
    expect(seen.followed).not.toContain(other.message.id);
    expect(botMessages(other.message.topicId)).toHaveLength(0);
  });

  test("streams a reply: streaming send, deltas, final edit", async () => {
    const r = await mention("stream please");
    const topicId = r.message.topicId;
    const [msg] = await waitFor("streaming message", () => botMessages(topicId).length > 0 && botMessages(topicId));
    expect(msg.streaming).toBe(true);
    const final = await waitFor("final edit", () =>
      events.find((e) => e.type === "message.updated" && e.data.messageId === msg.id && e.data.streaming === false),
    );
    expect(final.type === "message.updated" && final.data.body).toBe("one two three");
    const streamed = deltas.filter((d) => d.messageId === msg.id).map((d) => d.append).join("");
    expect(streamed.length).toBeGreaterThan(0);
    expect("one two three".startsWith(streamed)).toBe(true);
  });

  test("sets the topic status", async () => {
    const r = await mention("status check");
    const topicId = r.message.topicId;
    const upd = await waitFor("topic.updated", () =>
      events.find((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "in_progress"),
    );
    expect(upd.actorId).toBe(botId);
    await waitFor("reply after status", () => botMessages(topicId).length === 1);
  });

  test("starts a topic, edits and deletes its own message", async () => {
    const r = await bot.startTopic(generalId, { title: "Bot report", body: "draft" });
    expect(r.topic?.title).toBe("Bot report");
    const edited = await bot.edit(generalId, r.message.id, { body: "final" });
    expect(edited.message.body).toBe("final");
    await bot.deleteMessage(generalId, r.message.id);
    await waitFor("delete event", () => events.find((e) => e.type === "message.deleted" && e.data.messageId === r.message.id));
  });

  test("validates frames before sending", async () => {
    const err = await bot.send({ groupId: generalId }, { body: "x".repeat(40_000) }).catch((e) => e);
    expect(err).toBeInstanceOf(ChatError);
    expect((err as ChatError).code).toBe("invalid");
  });

  test("reconnects after the server restarts and sends queued writes", async () => {
    const down = waitFor("bot disconnected", () => !bot.connected, 20_000);
    await server.stop();
    await down;
    // A write made while disconnected waits for the connection.
    const queued = bot.startTopic(generalId, "written while the server was down");
    await server.start();
    await waitFor("bot reconnected", () => bot.connected, 45_000);
    await waitFor("human reconnected", () => human.connected, 45_000);
    const sent = await queued;
    expect(sent.message.body).toBe("written while the server was down");
    const r = await mention("after restart");
    await waitFor("reply after restart", () => botMessages(r.message.topicId).length === 1, 20_000);
  }, 120_000);

  test("catches up after a bot restart, once per missed mention", async () => {
    await bot.stop();
    expect(existsSync(stateFile)).toBe(true);
    const saved = JSON.parse(readFileSync(stateFile, "utf8"));
    expect(saved.cursors[generalId]).toBeGreaterThan(0);
    const before = seen.mentions.length;

    const missed = [await mention("missed one"), await mention("missed two"), await mention("missed three")];
    bot = makeEchoBot(seen);
    await bot.start();
    for (const m of missed) {
      await waitFor(`reply to ${m.message.body}`, () => botMessages(m.message.topicId).length === 1, 20_000);
    }
    for (const m of missed) expect(seen.catchUp).toContain(m.message.id);
    // Only the missed mentions were handled again, nothing older.
    expect(seen.mentions.length - before).toBe(3);
    await new Promise((r) => setTimeout(r, 500));
    for (const m of missed) expect(botMessages(m.message.topicId)).toHaveLength(1);
  }, 60_000);

  test("a handler re-run after a crash does not post twice (stable clientMsgId)", async () => {
    // Simulate a crash before the cursor was saved: a fresh bot with an in-memory state that
    // starts from the full history replays every mention; its replies dedupe on the server.
    const replay = new Bot({ url: server.url, tokenFile, stateStore: new MemoryStateStore(), firstStart: "all", logLevel: "warn" });
    const duplicates: boolean[] = [];
    let handled = 0;
    replay.onMention(async (ctx) => {
      handled++;
      if (ctx.message.body.includes("stream")) return;
      const text = ctx.message.body.replace(/^@Echo\s*/, "");
      const r = await ctx.reply(`echo: ${text}`);
      duplicates.push(r.duplicate === true);
    });
    const countBefore = events.filter((e) => e.type === "message.created").length;
    await replay.start();
    await waitFor("replayed mentions", () => handled >= 5, 20_000);
    await new Promise((r) => setTimeout(r, 1000));
    await replay.stop();
    expect(duplicates.length).toBeGreaterThan(0);
    expect(duplicates.every(Boolean)).toBe(true);
    expect(events.filter((e) => e.type === "message.created").length).toBe(countBefore);
  }, 60_000);

  test("a handler cut off by stop() runs again on the next start", async () => {
    const slow = await post("/v1/bots", { name: "Slow" }, { authorization: `Bearer ${ownerToken}` });
    const slowState = join(server.dir, "slow.state.json");
    const calls: boolean[] = [];
    const make = (hang: boolean) => {
      const b = new Bot({ url: server.url, token: slow.token, stateFile: slowState, stopTimeoutMs: 200, logLevel: "silent" });
      b.onMention(async (ctx) => {
        calls.push(ctx.catchUp);
        if (hang) await new Promise(() => {});
        await ctx.reply("done");
      });
      return b;
    };
    const first = make(true);
    await first.start();
    const r = await human.startTopic(generalId, { body: "@Slow work", mentions: [slow.member.id] });
    await waitFor("first call", () => calls.length === 1);
    await first.stop();
    const second = make(false);
    await second.start();
    await waitFor("reply after restart", () =>
      events.find((e) => e.type === "message.created" && e.data.message.authorId === slow.member.id && e.data.message.topicId === r.message.topicId),
    );
    expect(calls).toEqual([false, true]);
    await second.stop();
  }, 30_000);

  test.skipIf(!hasHttpSend)("HTTP send for one-shot scripts", async () => {
    const http = new HttpClient({ url: server.url, token: readFileSync(tokenFile, "utf8").trim() });
    const r = await http.postMessage(generalId, { title: "From a script", body: "deploy done", clientMsgId: "script-1" });
    expect(r.message.authorId).toBe(botId);
    const again = await http.postMessage(generalId, { title: "From a script", body: "deploy done", clientMsgId: "script-1" });
    expect(again.duplicate).toBe(true);
  });

  test.skipIf(!hasFiles)("uploads a file and attaches it", async () => {
    const ref = await bot.uploadFile({ groupId: generalId, name: "a.txt", mime: "text/plain", data: new TextEncoder().encode("hi") });
    const r = await bot.startTopic(generalId, { body: "file", attachments: [ref] });
    expect(r.message.attachments?.[0]?.fileId).toBe(ref.fileId);
  });

  test("posts a card when the server supports cards", async () => {
    const card: Card = { v: 1, title: "Deploy?", blocks: [{ type: "text", text: "ship it" }], actions: [{ id: "ok", label: "OK" }] };
    let res: Awaited<ReturnType<Bot["postCard"]>> | ChatError;
    try {
      res = await bot.postCard({ groupId: generalId }, card);
    } catch (err) {
      res = err as ChatError;
    }
    if (res instanceof ChatError || !res.message.card) {
      console.warn("cards: server on this build does not store cards; card press path not checked");
      return;
    }
    const pressed: string[] = [];
    bot.onCardAction(async (ctx) => {
      pressed.push(ctx.actionId);
      await ctx.updateCard({ ...card, actions: [], status: { label: "done" } });
    });
    await human.pressCard(generalId, res.message.id, "ok");
    await waitFor("card press", () => pressed.length === 1);
  });
});

console.error(`bot-sdk e2e: http send ${hasHttpSend ? "on" : "off"}, files ${hasFiles ? "on" : "off"}; log ${server.logFile}`);
