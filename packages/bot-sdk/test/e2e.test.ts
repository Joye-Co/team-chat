// End-to-end tests of the bot SDK against a local `wrangler dev` of server/ (port 8795).
// Run: bun test (in packages/bot-sdk). Set BOT_SDK_E2E_PORT to use another port.
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { BlockRefInput, Card, CardV2, Event, Message, TransientEvent } from "@chat/protocol";
import {
  ANONYMOUS_ACTOR,
  approvalCard,
  type AsideContext,
  blocks,
  Bot,
  type CardAnswerContext,
  type CardEventContext,
  ChatError,
  composeCard,
  decisionCard,
  draftCard,
  formCard,
  HttpClient,
  MemoryStateStore,
  parts,
  type StreamWriter,
} from "../src/index.ts";

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
const hasAcceptance = await routeExists("PUT", "/v1/runs/none/acceptance");

// Probe the 1.1 interaction loop: welcome.features, and which new frames have a handler (a
// frame without one gets "unknown frame type"). The server tracks land these one by one; a
// test whose server part is missing is skipped with the reason printed below.
const probe = new Bot({ url: server.url, token: ownerToken, logLevel: "error", markRead: false });
await probe.start();
const features = probe.serverFeatures;
async function frameHandled(frame: Record<string, unknown> & { t: string }): Promise<boolean> {
  try {
    await (probe as unknown as { request(f: Record<string, unknown>): Promise<unknown> }).request(frame);
    return true;
  } catch (err) {
    return !(err instanceof ChatError && /unknown frame type/.test(err.message));
  }
}
const NONE = "01JC0NONE00000000000000000";
const handles = {
  cardEvent: await frameHandled({ t: "card.event", groupId: generalId, messageId: NONE, rev: 1, clientEventId: "probe1", blockId: "x", event: "choice.change" }),
  cardPatch: await frameHandled({ t: "card.patch", groupId: generalId, messageId: NONE, baseRev: 1, ops: [{ op: "remove", id: "x" }] }),
  resolve: await frameHandled({ t: "interaction.resolve", groupId: generalId, eventId: NONE, result: "ignored" }),
  artifactData: await frameHandled({ t: "artifact.data", groupId: generalId, artifactId: NONE, version: 1, data: {} }),
  artifactReply: await frameHandled({ t: "artifact.reply", groupId: generalId, eventId: NONE, ok: true }),
};
await probe.stop();
const hasInteract = features.includes("interact/1") && handles.cardEvent && handles.cardPatch;
const hasAutoResolve = hasInteract && handles.resolve;
const hasPageLoop = hasFiles && features.includes("interact/1") && handles.artifactData && handles.artifactReply;
const hasRefs = features.includes("refs/1");
const skipped: string[] = [];
if (!hasInteract) skipped.push(`live card events and patches (welcome.features ${JSON.stringify(features)}, card.event ${handles.cardEvent}, card.patch ${handles.cardPatch})`);
if (!hasAutoResolve) skipped.push(`automatic interaction.resolve (interaction.resolve handled: ${handles.resolve})`);
if (!hasPageLoop) skipped.push(`page reply and data push (artifact.reply ${handles.artifactReply}, artifact.data ${handles.artifactData}, files ${hasFiles})`);
if (!hasRefs) skipped.push("refs to parts (welcome.features has no refs/1, so blockRefs are not stored)");
if (skipped.length > 0) console.warn(`bot-sdk e2e: this server lacks parts of 1.1; skipped: ${skipped.join("; ")}`);

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

/** A message as `history` stores it (with the derived v1 card). */
async function storedMessage(groupId: string, topicId: string, id: string): Promise<Message> {
  const page = await human.history(groupId, topicId, { limit: 100 });
  const m = page.messages.find((x) => x.id === id);
  if (!m) throw new Error(`message ${id} not in history`);
  return m;
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
    if (text.startsWith("work")) {
      // The 1.1 output contract: a reaction, the plan, then one done card with proof.
      await ctx.ack();
      await ctx.setPlan([{ text: "做事", state: "current" }]);
      await ctx.done({
        title: "做完了",
        summary: "改好了,测试通过",
        evidence: [{ label: "测试 12/12 通过", state: "pass" }],
        refs: [{ label: "PR #7", target: { kind: "pr", url: "https://github.com/o/r/pull/7", number: 7 } }],
      });
      return;
    }
    if (text.startsWith("fail")) {
      await ctx.setPlan([{ text: "部署", state: "current" }]);
      await ctx.done({ title: "部署", summary: "staging 起不来", failed: true });
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

  test("sets the plan of its current run; a member sees it and an update replaces it", async () => {
    const r = await human.startTopic(generalId, "周四小红书");
    const target = { groupId: generalId, topicId: r.message.topicId };
    const plan1 = [
      { text: "写标题和正文", state: "current" as const },
      { text: "周四 20:00 发出", state: "pending" as const },
    ];
    const run = await bot.setPlan(target, plan1);
    expect(run).toMatchObject({ botId, topicId: target.topicId, status: "running", plan: plan1 });
    const seen1 = await waitFor("run.updated with the plan", () =>
      events.find((e) => e.type === "run.updated" && e.data.run.id === run.id && e.data.run.plan?.length === 2),
    );
    expect(seen1.type === "run.updated" && seen1.data.run.plan).toEqual(plan1);
    expect(bot.currentRun(target.topicId)).toBe(run.id);

    // The same run; the whole plan is replaced.
    const plan2 = [
      { text: "写标题和正文", state: "done" as const },
      { text: "等 Joye 确认", state: "waiting" as const },
      { text: "周四 20:00 发出", state: "pending" as const },
    ];
    const again = await bot.setPlan(target, plan2);
    expect(again.id).toBe(run.id);
    await waitFor("replaced plan", () =>
      events.find((e) => e.type === "run.updated" && e.data.run.id === run.id && e.data.run.plan?.length === 3),
    );

    // Bad plans fail before they are sent.
    const err = await bot.setPlan(target, [{ text: "", state: "pending" }]).catch((e) => e);
    expect(err).toBeInstanceOf(ChatError);
    expect((err as ChatError).code).toBe("invalid");
    // A member cannot set the bot's plan.
    const denied = await human.http.setRunPlan(run.id, plan1).catch((e) => e);
    expect((denied as ChatError).code).toBe("forbidden");

    const ended = await bot.endRun(target, "done", "已排期");
    expect(ended).toMatchObject({ id: run.id, status: "done", plan: plan2 });
    expect(bot.currentRun(target.topicId)).toBeUndefined();
    // After the end, a new plan starts a new run.
    const next = await bot.setPlan(target, plan1);
    expect(next.id).not.toBe(run.id);
    await bot.endRun(target, "cancelled");
  });

  test("starts a topic, edits and deletes its own message", async () => {
    const r = await bot.startTopic(generalId, { title: "Bot report", body: "draft" });
    expect(r.topic?.title).toBe("Bot report");
    const edited = await bot.edit(generalId, r.message.id, { body: "final" });
    expect(edited.message.body).toBe("final");
    await bot.deleteMessage(generalId, r.message.id);
    await waitFor("delete event", () => events.find((e) => e.type === "message.deleted" && e.data.messageId === r.message.id));
  });

  test("paces a burst of 50 sends so all of them land (GAPS SF-10)", async () => {
    const t = await bot.startTopic(generalId, { title: "burst", body: "start" });
    const target = { groupId: generalId, topicId: t.topic!.id };
    const started = Date.now();
    const sent = await Promise.all(Array.from({ length: 50 }, (_, i) => bot.send(target, { body: `burst ${i}` })));
    expect(sent.map((r) => r.message.body)).toEqual(Array.from({ length: 50 }, (_, i) => `burst ${i}`));
    // 18 at once, then 18 per second: about 1.8 s, never a rate_limited failure.
    expect(Date.now() - started).toBeGreaterThan(1000);
    const seqs = sent.map((r) => r.message.createdSeq);
    expect([...seqs].sort((a, b) => a - b)).toEqual(seqs);
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
    expect(r.message.attachments?.[0]?.sha256).toBe(ref.sha256);
    const art = await bot.http.publishHtml({ groupId: generalId, topicId: r.message.topicId, title: "Report", html: "<h1>hi</h1>" });
    expect(art.artifact.kind).toBe("html");
    expect(art.version.version).toBe(1);
    await waitFor("artifact.created", () => events.find((e) => e.type === "artifact.created" && e.data.artifact.id === art.artifact.id));
    const v2 = await bot.http.publishHtml({ groupId: generalId, topicId: r.message.topicId, title: "Report", html: "<h1>v2</h1>", artifactId: art.artifact.id });
    expect(v2.version.version).toBe(2);
    // The media of a version after the upload (PUT .../media).
    const m = await bot.http.putArtifactMedia(art.artifact.id, 2, { kind: "html", mime: "text/html", bytes: 11, preview: "ready" });
    expect(m.version.version).toBe(2);
    expect(m.version.media).toMatchObject({ kind: "html", preview: "ready" });
    await waitFor("artifact.previewReady", () => events.find((e) => e.type === "artifact.previewReady" && e.data.artifactId === art.artifact.id));
    await expect(bot.http.putArtifactMedia(art.artifact.id, 2, { kind: "html", mime: "text/html", bytes: 30_000_000, preview: "ready" })).rejects.toMatchObject({ code: "invalid" });
  });

  test("reacts to a message and removes the reaction", async () => {
    const r = await human.startTopic(generalId, { body: "react to me" });
    expect(await bot.react(generalId, r.message.id, "👍")).toBe(true);
    expect(await bot.react(generalId, r.message.id, "👍")).toBe(false);
    await waitFor("reaction.changed", () =>
      events.find((e) => e.type === "reaction.changed" && e.data.messageId === r.message.id && e.data.on),
    );
    expect(await bot.react(generalId, r.message.id, "👍", false)).toBe(true);
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
    expect(res.message.kind).toBe("card");
    try {
      await human.pressCard(generalId, res.message.id, "ok");
    } catch (err) {
      if (err instanceof ChatError && /unknown frame type/.test(err.message)) {
        console.warn("cards: server on this build has no card.action; card press path not checked");
        return;
      }
      throw err;
    }
    await waitFor("card press", () => pressed.length === 1);
    await waitFor("card update", () =>
      events.find((e) => e.type === "message.updated" && e.data.messageId === res.message.id && e.data.card?.status?.label === "done"),
    );
  });
});

describe("1.1 cards, side chats and page events", () => {
  const ownerId = () => human.me.id;

  test("a decision ask: the answer reaches onCardAnswer with typed inputs, markAnswered edits the card", async () => {
    const answers: CardAnswerContext<{ choice: string }>[] = [];
    const resolved: number[] = [];
    bot.onCardAnswer<{ choice: string }>(async (ctx) => {
      if (ctx.card?.title !== "发哪个版本") return;
      answers.push(ctx);
      const m = await ctx.markAnswered({ intent: "info" });
      resolved.push(m.cardV2?.rev ?? 0);
    });
    const card = decisionCard({ title: "发哪个版本", text: "选一个", options: ["1.4", "1.5"], from: [ownerId()], record: true });
    const r = await bot.postCardV2({ groupId: generalId }, card);
    expect(r.message.kind).toBe("card");
    expect(r.message.cardV2?.rev).toBe(1);
    // Old readers get the derived v1 card without actions (there is a decision to fill).
    expect(r.message.card?.title).toBe("发哪个版本");
    expect(r.message.card?.actions).toBeUndefined();
    await waitFor("human sees the card", () =>
      events.find((e) => e.type === "message.created" && e.data.message.id === r.message.id),
    );
    await human.answerCard(generalId, { messageId: r.message.id, rev: 1, actionId: "submit", inputs: { choice: "o2" } });
    await waitFor("answer", () => answers.length === 1);
    const a = answers[0] as CardAnswerContext<{ choice: string }>;
    expect(a.actorId).toBe(ownerId());
    expect(a.choice()).toBe("o2");
    expect(a.choices()).toEqual(["o2"]);
    expect(a.inputs.choice).toBe("o2");
    expect(a.cancelled).toBe(false);
    expect(a.action?.id).toBe("submit");
    await waitFor("resolved", () => resolved.length === 1);
    expect(resolved[0]).toBe(2);
    const upd = await waitFor("human sees rev 2", () =>
      events.find((e) => e.type === "message.updated" && e.data.messageId === r.message.id && e.data.cardV2?.rev === 2),
    );
    if (upd.type !== "message.updated") throw new Error("unreachable");
    expect(upd.data.cardV2?.actions).toBeUndefined();
    expect(upd.data.cardV2?.intent).toBe("info");
    // A second answer to the old rev is refused: rev 2 has no submit action (INTERACT §3.7: not_found, card.block_gone).
    const again = await human
      .answerCard(generalId, { messageId: r.message.id, rev: 1, actionId: "submit", inputs: { choice: "o1" } })
      .then(() => null, (e) => e as ChatError);
    expect(again?.code).toBe("not_found");
  });

  test("an anonymous questionnaire: the answer reaches onCardAnswer as anonymous, with no member id", async () => {
    const got: CardAnswerContext[] = [];
    bot.onCardAnswer((ctx) => {
      if (ctx.card?.title === "匿名反馈") got.push(ctx);
    });
    const card = composeCard({
      intent: "ask",
      title: "匿名反馈",
      fallback: "两道题，匿名提交。",
      ask: { from: [ownerId()] },
      blocks: [
        blocks.questionnaire({
          id: "qs",
          anonymous: true,
          children: [
            blocks.rating({ id: "q1", label: "带教清楚吗？", fallbackText: "1 到 5 分" }),
            blocks.input({ id: "q2", kind: "textarea", label: "还想说什么" }),
          ],
        }),
      ],
    });
    const r = await bot.postCardV2({ groupId: generalId }, card);
    await waitFor("human sees the card", () => events.find((e) => e.type === "message.created" && e.data.message.id === r.message.id));
    // The desktop sends a questionnaire's submit as action.press of the block.
    await (human as unknown as { request(f: Record<string, unknown>): Promise<unknown> }).request({
      t: "card.event",
      groupId: generalId,
      messageId: r.message.id,
      rev: 1,
      clientEventId: "01JC0CEVANON0000000000000001",
      blockId: "qs",
      event: "action.press",
      inputs: { q1: { value: 4 }, q2: { value: "很好" } },
    });
    await waitFor("anonymous answer", () => got.length === 1);
    const a = got[0] as CardAnswerContext;
    expect(a.anonymous).toBe(true);
    expect(a.actorId).toBe(ANONYMOUS_ACTOR);
    expect(a.actorId).not.toBe(ownerId());
    expect(a.number("q1")).toBe(4);
  });

  test("an approval pressed by an old v1 client arrives as an answer", async () => {
    const got: CardAnswerContext[] = [];
    bot.onCardAnswer((ctx) => {
      if (ctx.card?.title === "发布到正式环境?") got.push(ctx);
    });
    const r = await bot.postCardV2(
      { groupId: generalId },
      approvalCard({ title: "发布到正式环境?", text: "qqbot 1.4", from: ["owners"] }),
    );
    // No inputs: old clients may press its buttons.
    expect(r.message.card?.actions?.map((x) => x.id)).toEqual(["approve", "deny"]);
    await human.pressCard(generalId, r.message.id, "deny");
    await waitFor("v1 press as answer", () => got.length === 1);
    expect(got[0]?.actionId).toBe("deny");
    expect(got[0]?.cancelled).toBe(true);
  });

  test("a form and a draft: inputs and edited text by block id", async () => {
    const got: CardAnswerContext[] = [];
    bot.onCardAnswer((ctx) => {
      if (ctx.card?.title === "排期" || ctx.card?.title === "回复草稿") got.push(ctx);
    });
    const form = await bot.postCardV2(
      { groupId: generalId },
      formCard({
        title: "排期",
        from: ["requester", ownerId()],
        inputs: [
          { id: "count", kind: "number", label: "几篇", min: 1 },
          { id: "notify", kind: "switch", label: "通知大家" },
          { id: "note", kind: "textarea", label: "备注" },
        ],
      }),
    );
    await human.answerCard(generalId, {
      messageId: form.message.id,
      rev: 1,
      actionId: "submit",
      inputs: { count: 3, notify: true, note: "周四" },
    });
    const draft = await bot.postCardV2(
      { groupId: generalId },
      draftCard({ title: "回复草稿", draft: "您好,问题已修复。", from: [ownerId()], cancelLabel: "不发" }),
    );
    await human.answerCard(generalId, {
      messageId: draft.message.id,
      rev: 1,
      actionId: "submit",
      editedValues: { draft: "您好,问题已修复,谢谢。" },
    });
    await waitFor("two answers", () => got.length === 2);
    const f = got.find((x) => x.messageId === form.message.id) as CardAnswerContext;
    expect(f.number("count")).toBe(3);
    expect(f.bool("notify")).toBe(true);
    expect(f.text("note")).toBe("周四");
    expect(f.text("missing")).toBeUndefined();
    const d = got.find((x) => x.messageId === draft.message.id) as CardAnswerContext;
    expect(d.draft()).toBe("您好,问题已修复,谢谢。");
  });

  test("after a restart the answered card is loaded again: resolve and draft work", async () => {
    const asker = await post("/v1/bots", { name: "Asker" }, { authorization: `Bearer ${ownerToken}` });
    const askerState = join(server.dir, "asker.state.json");
    const first = new Bot({ url: server.url, token: asker.token, stateFile: askerState, logLevel: "warn" });
    await first.start();
    const ask = await first.postCardV2(
      { groupId: generalId },
      decisionCard({ title: "重启后选", options: ["甲", "乙"], from: [ownerId()] }),
    );
    const draft = await first.postCardV2(
      { groupId: generalId },
      draftCard({ title: "重启后的草稿", draft: "原文不改", from: [ownerId()] }),
    );
    await first.stop();
    // State saved by an older SDK has no seq: the card is found by paging back.
    const saved = JSON.parse(readFileSync(askerState, "utf8"));
    expect(saved.cards[ask.message.id].seq).toBe(ask.message.createdSeq);
    delete saved.cards[draft.message.id].seq;
    // The cursor is past both cards, so catch-up does not show them to the new process.
    saved.cursors[generalId] = Math.max(saved.cursors[generalId] ?? 0, draft.message.createdSeq);
    writeFileSync(askerState, JSON.stringify(saved));
    // Answered while the bot is down: reaches it in catch-up.
    await human.answerCard(generalId, { messageId: draft.message.id, rev: 1, actionId: "submit" });

    const second = new Bot({ url: server.url, token: asker.token, stateFile: askerState, logLevel: "warn" });
    expect(second.knownCard(ask.message.id)).toBeUndefined();
    const drafts: (string | undefined)[] = [];
    const resolved: number[] = [];
    second.onCardAnswer(async (ctx) => {
      if (ctx.messageId === draft.message.id) drafts.push(ctx.draft());
      if (ctx.messageId === ask.message.id) {
        expect(ctx.card?.title).toBe("重启后选");
        expect(ctx.action?.id).toBe("submit");
        expect(ctx.cancelled).toBe(false);
        const m = await ctx.markAnswered({ intent: "info" });
        resolved.push(m.cardV2?.rev ?? 0);
      }
    });
    await second.start();
    try {
      await waitFor("draft answer after restart", () => drafts.length === 1, 20_000);
      expect(drafts[0]).toBe("原文不改");
      await human.answerCard(generalId, { messageId: ask.message.id, rev: 1, actionId: "submit", inputs: { choice: "o1" } });
      await waitFor("resolved after restart", () => resolved.length === 1, 20_000);
      expect(resolved[0]).toBe(2);
    } finally {
      await second.stop();
    }
  }, 60_000);

  test("builders reject cards that do not fit the protocol", () => {
    expect(() => decisionCard({ title: "x", options: ["only one"], from: ["owners"] })).toThrow(ChatError);
    expect(() => formCard({ title: "x", inputs: [{ id: "bad id!", kind: "text", label: "a" }], from: ["owners"] })).toThrow(
      ChatError,
    );
  });

  test("the output contract: ack reaction, plan, done card with proof, run ended", async () => {
    const r = await mention("work please");
    await waitFor("ack reaction", () =>
      events.find((e) => e.type === "reaction.changed" && e.data.messageId === r.message.id && e.actorId === botId && e.data.on),
    );
    const done = await waitFor("done card", () => botMessages(r.message.topicId).find((m) => m.cardV2?.intent === "done"));
    expect(done.cardV2?.blocks.map((b) => b.type)).toEqual(["text", "evidence", "ref"]);
    // A socket that reads tc.card/2.x gets no v1 fallback live; history keeps it for every reader.
    expect(done.card).toBeUndefined();
    expect((await storedMessage(r.message.groupId, r.message.topicId, done.id)).card?.tone).toBe("success");
    expect(botMessages(r.message.topicId).filter((m) => !m.cardV2)).toHaveLength(0);
    const ended = await waitFor("run done", () =>
      events.find(
        (e) => e.type === "run.updated" && e.data.run.topicId === r.message.topicId && e.data.run.status === "done" && e.data.run.endedAt,
      ),
    );
    if (ended.type !== "run.updated") throw new Error("unreachable");
    expect(ended.data.run.summary).toBe("改好了,测试通过");
  });

  test("a failed done card shows the failure in v2 and in the v1 card, and ends the run failed", async () => {
    const r = await mention("fail now");
    const done = await waitFor("failed done card", () => botMessages(r.message.topicId).find((m) => m.cardV2?.intent === "done"));
    expect(done.cardV2?.blocks[1]).toEqual({ type: "evidence", id: "failed", label: "没有完成", state: "fail" });
    const stored = await storedMessage(r.message.groupId, r.message.topicId, done.id);
    expect(stored.card?.tone).toBe("danger");
    expect(stored.card?.blocks).toContainEqual({ type: "list", items: [{ text: "没有完成", status: "failed" }] });
    await waitFor("run failed", () =>
      events.find((e) => e.type === "run.updated" && e.data.run.topicId === r.message.topicId && e.data.run.status === "failed"),
    );
  });

  test("a side chat: onAside gets the topic up to uptoSeq and answers in the DM", async () => {
    const seenAside: AsideContext[] = [];
    const sources: string[][] = [];
    bot.onAside(async (ctx) => {
      seenAside.push(ctx);
      const src = await ctx.source();
      sources.push(src.map((m) => m.body));
      await ctx.reply(`看了 ${src.length} 条`);
    });
    const t = await human.startTopic(generalId, { title: "发版", body: "第一条" });
    const second = await human.send({ groupId: generalId, topicId: t.message.topicId }, "第二条");
    await human.send({ groupId: generalId, topicId: t.message.topicId }, "之后的不算");
    const dm = await new HttpClient({ url: server.url, token: ownerToken }).dm(botId);
    const ask = await human.send(
      { groupId: dm.group.id },
      { body: "这个话题现在怎样?", context: { groupId: generalId, topicId: t.message.topicId, uptoSeq: second.message.createdSeq } },
    );
    expect(ask.topic?.contextOf?.topicId).toBe(t.message.topicId);
    await waitFor("aside handled", () => sources.length === 1);
    expect(sources[0]).toEqual(["第一条", "第二条"]);
    expect(seenAside[0]?.aside.groupId).toBe(generalId);
    expect(seenAside[0]?.groupId).toBe(dm.group.id);
    await waitFor("answer in the DM", () =>
      events.find(
        (e) =>
          e.type === "message.created" &&
          e.groupId === dm.group.id &&
          e.data.message.authorId === botId &&
          e.data.message.body === "看了 2 条",
      ),
    );
    // Nothing reached the source topic.
    expect(botMessages(t.message.topicId)).toHaveLength(0);
  });

  test.skipIf(!hasFiles)("a page event from my 产出 reaches onArtifactEvent", async () => {
    const got: { name: string; payload: unknown; actorId: string; topicId?: string }[] = [];
    bot.onArtifactEvent((ctx) => {
      got.push({ name: ctx.name, payload: ctx.payload, actorId: ctx.actorId, topicId: ctx.topicId });
    });
    const t = await human.startTopic(generalId, { title: "看板", body: "做个页面" });
    const v = await bot.http.publishHtml({
      groupId: generalId,
      topicId: t.message.topicId,
      title: "看板",
      html: "<!doctype html><p>hi</p>",
      events: [{ name: "pick", label: "选这个" }],
    });
    expect(v.version.events?.[0]?.name).toBe("pick");
    // The human's page sends the declared event (the desktop bridge does this).
    await (human as unknown as { request(f: Record<string, unknown>): Promise<unknown> }).request({
      t: "artifact.event",
      groupId: generalId,
      artifactId: v.artifact.id,
      version: v.version.version,
      name: "pick",
      payload: { row: 2 },
    });
    await waitFor("page event", () => got.length === 1);
    expect(got[0]).toEqual({ name: "pick", payload: { row: 2 }, actorId: ownerId(), topicId: t.message.topicId });
  });

  test.skipIf(!hasAcceptance)("sets the acceptance criteria of its run", async () => {
    const t = await human.startTopic(generalId, { title: "验收", body: "做吧" });
    const target = { groupId: generalId, topicId: t.message.topicId };
    const run = await bot.setAcceptance(target, [
      { text: "staging 能打开", state: "pass", evidence: { url: "https://staging.example.com" } },
      { text: "测试通过", state: "pending" },
    ]);
    expect(run.acceptance?.map((a) => a.state)).toEqual(["pass", "pending"]);
    await bot.endRun(target, "done");
  });
});

describe("1.1 interaction loop (INTERACT 8.4, REFS C3)", () => {
  const ownerId = () => human.me.id;
  const auth = { authorization: `Bearer ${ownerToken}` };
  let ledger: Bot;
  let ledgerId = "";
  let ledgerToken = "";
  const ledgerState = join(server.dir, "ledger.state.json");
  const transients: TransientEvent[] = [];
  human.onTransient((e) => transients.push(e));

  /** The 7.1 report (rev 1), asked of the human. */
  function report(): CardV2 {
    const card = JSON.parse(readFileSync(join(import.meta.dir, "../../../protocol/fixtures/entities/card-v2-report-compose.json"), "utf8")).value as CardV2;
    // The fixture links a 产出 of the fixture group; a card names only 产出 of its own group.
    const blocks = card.blocks.filter((b) => !(b.type === "ref" && b.target.kind === "artifact"));
    return { ...card, blocks, ask: { ...(card.ask as NonNullable<CardV2["ask"]>), from: [ownerId()] } };
  }
  /** The human's side: a raw 1.1 frame (the desktop sends these). */
  function frame(f: Record<string, unknown> & { t: string }): Promise<any> {
    return (human as unknown as { request(f: Record<string, unknown>): Promise<unknown> }).request(f);
  }
  let n = 0;
  const clientEventId = () => `01JC0CEV${String(++n).padStart(18, "0")}`;

  beforeAll(async () => {
    const made = await post("/v1/bots", { name: "Ledger", role: "test" }, auth);
    ledgerId = made.member.id;
    ledgerToken = made.token;
    ledger = new Bot({ url: server.url, token: ledgerToken, stateFile: ledgerState, logLevel: "warn", markRead: false });
    await ledger.start();
    await waitFor("the human knows Ledger", () => human.members.some((m) => m.id === ledgerId));
  });
  afterAll(async () => {
    await ledger.stop();
  });

  test.skipIf(!hasInteract)("7.1: choice.change reaches onCardEvent, the patch reaches the member as rev 2 with the cause", async () => {
    const seen: CardEventContext[] = [];
    ledger.onCardEvent(async (ctx) => {
      if (ctx.card.title !== "10 月数据月报" || ctx.blockId !== "check") return;
      seen.push(ctx);
      if ((ctx.value as { value?: string }).value !== "wrong") return;
      await ctx.patch((d) => {
        d.insert(
          blocks.group({
            id: "drill",
            title: "按渠道拆开 · 新增群",
            collapsible: false,
            children: [
              blocks.table({
                id: "by_channel",
                columns: ["渠道", "新增群", "占比", "比 9 月"],
                rows: [["小红书", "14", "37%", "+6"], ["群内邀请", "10", "26%", "+1"], ["B 站", "9", "24%", "−1"], ["抖音", "5", "13%", "−2"]],
                rowKeys: ["xhs", "invite", "bili", "douyin"],
                select: "multiple",
                events: ["table.rowSelect"],
              }),
              blocks.input({ id: "what", kind: "textarea", label: "哪一项不对？后台看到的是多少？", required: false, maxLength: 300 }),
            ],
          }),
          { after: "main" },
        );
        d.remove("note");
        d.cardProps({ actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "by_channel", "what"] }] });
      });
    });
    const r = await ledger.postCardV2({ groupId: generalId }, report());
    await waitFor("human sees the report", () => events.find((e) => e.type === "message.created" && e.data.message.id === r.message.id));
    const ack = await frame({ t: "card.event", groupId: generalId, messageId: r.message.id, rev: 1, clientEventId: clientEventId(), blockId: "check", event: "choice.change", value: { value: "wrong" } });
    expect(ack.stored).toBe(false);
    expect(ack.expect.by).toBe(ledgerId);
    const upd = await waitFor("rev 2", () =>
      events.find((e) => e.type === "message.updated" && e.data.messageId === r.message.id && e.data.cardV2?.rev === 2),
    );
    if (upd.type !== "message.updated") throw new Error("unreachable");
    expect(upd.data.cause?.eventId).toBe(ack.eventId);
    expect(upd.data.cause?.actorId).toBe(ownerId());
    expect(upd.data.cardV2?.blocks.some((b) => b.id === "by_channel")).toBe(true);
    expect(upd.data.cardV2?.blocks.some((b) => b.id === "note")).toBe(false);
    expect(seen[0]?.eventId).toBe(ack.eventId);
    await waitFor("patched resolve", () => transients.find((e) => e.type === "interaction.resolved" && e.eventId === ack.eventId));
  });

  test.skipIf(!hasAutoResolve)("a live event the handler does not answer is resolved ignored", async () => {
    const r = await ledger.postCardV2({ groupId: generalId }, { ...report(), title: "没人管的图" });
    const ack = await frame({ t: "card.event", groupId: generalId, messageId: r.message.id, rev: 1, clientEventId: clientEventId(), blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-13", y: 2268, index: 1 }, part: parts.point("oct", "10-13") });
    const res = await waitFor("interaction.resolved", () =>
      transients.find((e): e is Extract<TransientEvent, { type: "interaction.resolved" }> => e.type === "interaction.resolved" && e.eventId === ack.eventId),
    );
    expect(res.result).toBe("ignored");
  });

  test.skipIf(!hasInteract)("ctx.patch rebases after a concurrent edit", async () => {
    const r = await ledger.postCardV2({ groupId: generalId }, { ...report(), title: "并发" });
    const done: number[] = [];
    ledger.onCardEvent(async (ctx) => {
      if (ctx.card.title !== "并发") return;
      // A full edit the SDK has not applied yet: the patch below starts from rev 1.
      await ledger.edit(generalId, ctx.messageId, { body: "并发", cardV2: { ...ctx.card, rev: 2, summary: "改过一次" } });
      const m = await ctx.patch([{ op: "set", id: "m_groups", props: { value: 40 } }]);
      done.push(m.cardV2?.rev ?? 0);
    });
    await frame({ t: "card.event", groupId: generalId, messageId: r.message.id, rev: 1, clientEventId: clientEventId(), blockId: "check", event: "choice.change", value: { value: "ok" } });
    await waitFor("patched after the edit", () => done.length === 1, 20_000);
    expect(done[0]).toBe(3);
    const last = await waitFor("rev 3", () =>
      events.find((e) => e.type === "message.updated" && e.data.messageId === r.message.id && e.data.cardV2?.rev === 3),
    );
    if (last.type !== "message.updated") throw new Error("unreachable");
    expect(last.data.cardV2?.summary).toBe("改过一次");
  });

  test("onCardAnswer gets an answer made while the bot was away, on catch-up, with the card", async () => {
    const r = await ledger.postCardV2({ groupId: generalId }, { ...report(), title: "离线时回答" });
    await waitFor("human sees it", () => events.find((e) => e.type === "message.created" && e.data.message.id === r.message.id));
    await new Promise((res) => setTimeout(res, 700)); // the state file is saved after 500 ms
    await ledger.stop();
    const inputs = { check: { value: "wrong" }, note: { value: "后台是 7 个" } };
    if (hasInteract) {
      await frame({ t: "card.event", groupId: generalId, messageId: r.message.id, rev: 1, clientEventId: clientEventId(), actionId: "send", event: "action.press", inputs });
    } else {
      await human.answerCard(generalId, { messageId: r.message.id, rev: 1, actionId: "send", inputs: inputs as never });
    }
    ledger = new Bot({ url: server.url, token: ledgerToken, stateFile: ledgerState, logLevel: "warn", markRead: false });
    const got: CardAnswerContext[] = [];
    ledger.onCardAnswer((ctx) => {
      if (ctx.messageId === r.message.id) got.push(ctx);
    });
    await ledger.start();
    await waitFor("caught-up answer", () => got.length === 1, 20_000);
    const a = got[0] as CardAnswerContext;
    expect(a.catchUp).toBe(true);
    expect(a.stored).toBe(true);
    expect(a.card.title).toBe("离线时回答");
    expect(a.actorId).toBe(ownerId());
    expect(a.choice("check")).toBe("wrong");
    expect(a.text("note")).toBe("后台是 7 个");
  });

  test.skipIf(!hasPageLoop)("onArtifactEvent: the reply reaches the asker, pushData reaches a second member", async () => {
    const watcherMade = await post("/v1/bots", { name: "Watcher", role: "test" }, auth);
    const watcher = new Bot({ url: server.url, token: watcherMade.token, logLevel: "warn", markRead: false });
    const watched: TransientEvent[] = [];
    watcher.onTransient((e) => watched.push(e));
    await watcher.start();
    try {
      const t = await human.startTopic(generalId, { title: "看板 1.1", body: "做个页面" });
      const v = await ledger.http.publishHtml({
        groupId: generalId,
        topicId: t.message.topicId,
        title: "看板",
        html: "<!doctype html><p>hi</p>",
        events: [{ name: "filter.pick", kind: "ask" }],
      });
      ledger.onArtifactEvent(async (ctx) => {
        if (ctx.artifactId !== v.artifact.id) return;
        await ctx.pushData({ channel: ctx.payload });
        await ctx.reply({ rows: 4 });
      });
      const ack = await frame({ t: "artifact.event", groupId: generalId, artifactId: v.artifact.id, version: v.version.version, clientEventId: clientEventId(), name: "filter.pick", kind: "ask", payload: "xhs" });
      const reply = await waitFor("artifact.reply to the asker", () =>
        transients.find((e): e is Extract<TransientEvent, { type: "artifact.reply" }> => e.type === "artifact.reply" && e.eventId === ack.eventId),
      );
      expect(reply.ok).toBe(true);
      expect(reply.payload).toEqual({ rows: 4 });
      const data = await waitFor("artifact.data to a second member", () =>
        watched.find((e): e is Extract<TransientEvent, { type: "artifact.data" }> => e.type === "artifact.data" && e.artifactId === v.artifact.id),
      );
      expect(data.data).toEqual({ channel: "xhs" });
      expect(data.cause).toBe(ack.eventId);
      expect(watched.some((e) => e.type === "artifact.reply")).toBe(false);
    } finally {
      await watcher.stop();
    }
  });

  test.skipIf(!hasPageLoop)("page emits: ctx.postMessage names the emit as cause and resolves it posted; an unanswered emit is resolved ignored", async () => {
    const t = await human.startTopic(generalId, { title: "看板 emit", body: "做个页面" });
    const v = await ledger.http.publishHtml({
      groupId: generalId,
      topicId: t.message.topicId,
      title: "看板 emit",
      html: "<!doctype html><p>hi</p>",
      events: [
        { name: "report.flag", kind: "emit" },
        { name: "report.seen", kind: "emit" },
      ],
    });
    const errors: unknown[] = [];
    ledger.onError((e) => errors.push(e));
    ledger.onArtifactEvent(async (ctx) => {
      if (ctx.artifactId !== v.artifact.id || ctx.name !== "report.flag") return;
      await ctx.postMessage("收到，我去看一下");
    });
    const emit = (name: string) =>
      frame({ t: "artifact.event", groupId: generalId, artifactId: v.artifact.id, version: v.version.version, clientEventId: clientEventId(), name, kind: "emit", payload: null });
    const resolvedOf = (eventId: string) =>
      waitFor(`interaction.resolved for ${eventId}`, () =>
        transients.find((e): e is Extract<TransientEvent, { type: "interaction.resolved" }> => e.type === "interaction.resolved" && e.eventId === eventId),
      );
    const flag = await emit("report.flag");
    const posted = await waitFor("the post of the emit", () =>
      events.find((e) => e.type === "message.created" && e.data.message.body === "收到，我去看一下" && e.data.message.topicId === t.message.topicId),
    );
    if (posted.type !== "message.created") throw new Error("unreachable");
    expect(posted.data.message.cause?.eventId).toBe(flag.eventId);
    expect((await resolvedOf(flag.eventId)).result).toBe("posted");
    const seen = await emit("report.seen");
    expect((await resolvedOf(seen.eventId)).result).toBe("ignored");
    expect(errors).toHaveLength(0);
  });

  test.skipIf(!hasRefs)("REFS C3 1-4: a question about a cell and a node; ctx.refs, ctx.question, resolveRef current, then not current", async () => {
    const card = composeCard({
      intent: "info",
      title: "10 月数据月报",
      blocks: [
        blocks.table({ id: "channels", title: "渠道表", columns: ["渠道", "新增群"], rows: [["小红书", "14"], ["B 站", "9"]], rowKeys: ["xhs", "bili"] }),
        blocks.diagram({ id: "flow", source: "flowchart LR\n  a[收到消息] --> b{要回复吗}\n  b --> c[回复]" }),
      ],
    });
    const posted = await ledger.postCardV2({ groupId: generalId }, card);
    const asked: { refs: number; question: string; current: boolean[]; cell: unknown; node: unknown }[] = [];
    ledger.onMention(async (ctx) => {
      if (ctx.refs.length === 0) return;
      const [cell, node] = await Promise.all(ctx.refs.map((x) => ctx.resolveRef(x)));
      asked.push({ refs: ctx.refs.length, question: ctx.question, current: [cell?.current ?? false, node?.current ?? false], cell: cell?.part, node: node?.part });
    });
    const refs: BlockRefInput[] = [
      { messageId: posted.message.id, cardRev: 1, blockId: "channels", part: parts.cell("bili", 1), label: "10 月数据月报 · 渠道表 · B 站 · 新增群 9", value: { row: "B 站", column: "新增群", display: "9", raw: 9 } },
      { messageId: posted.message.id, cardRev: 1, blockId: "flow", part: parts.node("b"), label: "10 月数据月报 · 图 · 要回复吗", value: { node: "b", label: "要回复吗" } },
    ];
    const q = await human.send({ groupId: generalId, topicId: posted.message.topicId }, { body: "@Ledger 为什么比上月少了 1 个？", mentions: [ledgerId], blockRefs: refs });
    expect(q.message.body).toBe(`> ↳ ${refs[0]?.label}\n> ↳ ${refs[1]?.label}\n\n@Ledger 为什么比上月少了 1 个？`);
    expect(q.message.blockRefs?.map((x) => x.sourceAuthorId)).toEqual([ledgerId, ledgerId]);
    await waitFor("ref question", () => asked.length === 1);
    expect(asked[0]?.refs).toBe(2);
    expect(asked[0]?.question).toBe("@Ledger 为什么比上月少了 1 个？");
    expect(asked[0]?.cell).toEqual({ row: "B 站", column: "新增群", display: "9" });
    expect(asked[0]?.node).toEqual({ node: "b", label: "要回复吗" });
    expect(asked[0]?.current).toEqual([true, true]);
    // A newer rev: the same refs are no longer current.
    if (hasInteract) await ledger.patchCard(generalId, posted.message.id, [{ op: "set", id: "channels", props: { title: "渠道" } }]);
    else await ledger.updateCardV2(generalId, posted.message.id, { ...card, rev: 2 });
    await human.send({ groupId: generalId, topicId: posted.message.topicId }, { body: "@Ledger 再看一下", mentions: [ledgerId], blockRefs: refs });
    await waitFor("second ref question", () => asked.length === 2);
    expect(asked[1]?.current).toEqual([false, false]);
    // The runner prompt's changedSince is checked in the server tests (buildPrompt).
  });
});

describe("bot loops", () => {
  test("messages of other bots reach no handler unless allowBotAuthors is set", async () => {
    const auth = { authorization: `Bearer ${ownerToken}` };
    const taro = await post("/v1/bots", { name: "Taro", role: "test" }, auth);
    const lis = await post("/v1/bots", { name: "Listener", role: "test" }, auth);
    const taroBot = new Bot({ url: server.url, token: taro.token, logLevel: "warn", markRead: false });
    const listener = new Bot({ url: server.url, token: lis.token, logLevel: "warn", markRead: false, allowBotAuthors: true });
    const heard: string[] = [];
    listener.onMention(async (ctx) => {
      heard.push(ctx.message.id);
    });
    await taroBot.start();
    await listener.start();
    try {
      await waitFor("echo knows the other bots", () =>
        [taro.member.id, lis.member.id].every((id) => bot.members.some((m) => m.id === id)),
      );
      const r = await taroBot.startTopic(generalId, {
        body: "@Echo @Listener hi",
        mentions: [botId, lis.member.id],
      });
      await waitFor("listener (allowBotAuthors) hears the bot", () => heard.includes(r.message.id));
      await new Promise((res) => setTimeout(res, 500));
      expect(seen.mentions).not.toContain(r.message.id);
      expect(botMessages(r.message.topicId)).toHaveLength(0);
      // A human's message in the same topic still reaches Echo.
      const h = await human.send({ groupId: generalId, topicId: r.message.topicId }, {
        body: "@Echo from a human",
        mentions: [botId],
      });
      await waitFor("echo answers the human", () => seen.mentions.includes(h.message.id));
    } finally {
      await taroBot.stop();
      await listener.stop();
    }
  });
});

console.error(
  `bot-sdk e2e: http send ${hasHttpSend ? "on" : "off"}, files ${hasFiles ? "on" : "off"}, interact ${hasInteract ? "on" : "off"}, refs ${hasRefs ? "on" : "off"}; log ${server.logFile}`,
);
