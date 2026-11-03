// End-to-end tests of the bot SDK against a local `wrangler dev` of server/ (port 8795).
// Run: bun test (in packages/bot-sdk). Set BOT_SDK_E2E_PORT to use another port.
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Card, Event, Message } from "@chat/protocol";
import {
  approvalCard,
  type AsideContext,
  Bot,
  type CardAnswerContext,
  ChatError,
  decisionCard,
  draftCard,
  formCard,
  HttpClient,
  MemoryStateStore,
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

  test("a decision ask: the answer reaches onCardAnswer with typed inputs, resolve edits the card", async () => {
    const answers: CardAnswerContext<{ choice: string }>[] = [];
    const resolved: number[] = [];
    bot.onCardAnswer<{ choice: string }>(async (ctx) => {
      if (ctx.card?.title !== "发哪个版本") return;
      answers.push(ctx);
      const m = await ctx.resolve({ intent: "info" });
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
    // A second answer to the old rev is refused.
    const again = await human
      .answerCard(generalId, { messageId: r.message.id, rev: 1, actionId: "submit", inputs: { choice: "o1" } })
      .then(() => null, (e) => e as ChatError);
    expect(again?.code).toBe("conflict");
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
    expect(done.card?.tone).toBe("success");
    expect(botMessages(r.message.topicId).filter((m) => !m.cardV2)).toHaveLength(0);
    const ended = await waitFor("run done", () =>
      events.find(
        (e) => e.type === "run.updated" && e.data.run.topicId === r.message.topicId && e.data.run.status === "done" && e.data.run.endedAt,
      ),
    );
    if (ended.type !== "run.updated") throw new Error("unreachable");
    expect(ended.data.run.summary).toBe("改好了,测试通过");
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

console.error(`bot-sdk e2e: http send ${hasHttpSend ? "on" : "off"}, files ${hasFiles ? "on" : "off"}; log ${server.logFile}`);
