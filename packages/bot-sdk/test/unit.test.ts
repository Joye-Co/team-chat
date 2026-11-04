// Unit tests of the 1.1 interaction loop, refs and Card v2.1 builders against a fake server
// (test/fake.ts): no network, no wrangler. Run: bun test test/unit.test.ts
import { afterEach, describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ACK_EMOJI as PROTOCOL_ACK_EMOJI, type CardV2 } from "@chat/protocol";
import {
  ACK_EMOJI,
  blocks,
  Bot,
  type CardAnswerContext,
  CardDraft,
  type CardEventContext,
  ChatError,
  checkArtifactEvents,
  checkDiagram,
  composeCard,
  HttpClient,
  MAX_PATTERN_INPUT,
  markdownBlocks,
  MemoryStateStore,
  mermaid,
  opsFit,
  parts,
  patternProblem,
  stripRefPrefix,
} from "../src/index.ts";
import { BOT_ID, FakeServer, type Frame, GROUP_ID, HUMAN_ID, message, type Reply, TOPIC_ID } from "./fake.ts";

const FIXTURES = join(import.meta.dir, "../../../protocol/fixtures");
const fixture = (path: string) => JSON.parse(readFileSync(join(FIXTURES, path), "utf8")).value;

const bots: Bot[] = [];
afterEach(async () => {
  for (const b of bots.splice(0)) await b.stop();
});

async function start(server: FakeServer, store = new MemoryStateStore()): Promise<Bot> {
  const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: store, logLevel: "error", stopTimeoutMs: 500 });
  bots.push(bot);
  await bot.start();
  return bot;
}

/** The fake answers `send` with the stored message (rev 1 card) and `card.patch` with rev + 1. */
function cardServer(): { server: FakeServer; revs: Map<string, CardV2> } {
  const server = new FakeServer();
  const revs = new Map<string, CardV2>();
  let n = 0;
  server.on("send", (f) => {
    const id = `01JC0MSG0000000000000000${String(++n).padStart(2, "0")}`;
    const cardV2 = f.cardV2 as CardV2 | undefined;
    if (cardV2) revs.set(id, cardV2);
    return { ok: true, result: { message: message({ id, body: f.body, kind: cardV2 ? "card" : undefined, card: cardV2 ? { v: 1, title: cardV2.title, blocks: [] } : undefined, cardV2 }) } };
  });
  server.on("card.patch", (f) => {
    const cur = revs.get(f.messageId as string) as CardV2;
    if (f.baseRev !== cur.rev) return { ok: false, error: { code: "conflict", message: "stale", reason: "card.stale_rev" } };
    const next = { ...cur, rev: cur.rev + 1 };
    revs.set(f.messageId as string, next);
    return { ok: true, result: { message: message({ id: f.messageId as string, kind: "card", card: { v: 1, title: next.title, blocks: [] }, cardV2: next }), rev: next.rev } };
  });
  return { server, revs };
}

const report = (): CardV2 => fixture("entities/card-v2-report-compose.json");

function liveEvent(messageId: string, o: Record<string, unknown> = {}) {
  return {
    type: "card.event",
    groupId: GROUP_ID,
    topicId: TOPIC_ID,
    messageId,
    eventId: `01JC0EV${Math.random().toString(36).slice(2, 12).toUpperCase().padEnd(19, "0")}`,
    rev: 1,
    blockId: "check",
    event: "choice.change",
    value: { value: "wrong" },
    actorId: HUMAN_ID,
    at: Date.now(),
    ...o,
  };
}

describe("ack", () => {
  test("SDK bots acknowledge with the emoji runner runs use", () => {
    expect(ACK_EMOJI).toBe(PROTOCOL_ACK_EMOJI);
  });
});

describe("hello and features", () => {
  test("hello sends interact/1, refs/1 and both catalogs; welcome.features is kept", async () => {
    const server = new FakeServer();
    const bot = await start(server);
    const hello = server.of("hello")[0] as Frame;
    expect(hello.features).toEqual(["interact/1", "refs/1"]);
    expect(hello.supportedCatalogs).toEqual(["tc.card/2.0", "tc.card/2.1"]);
    expect(bot.serverFeatures).toEqual(["interact/1", "refs/1", "diagram/1"]);
  });
});

describe("live card events", () => {
  test("onCardEvent patches the card: baseRev, cause and the newest card", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const seen: CardEventContext[] = [];
    bot.onCardEvent(async (ctx) => {
      seen.push(ctx);
      await ctx.patch((d) => {
        d.insert(
          blocks.group({ id: "drill", title: "按渠道拆开 · 新增群", collapsible: false, children: [blocks.text("fixed", "已按后台改正。")] }),
          { after: "main" },
        );
        d.remove("note");
      });
    });
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    const ev = liveEvent(r.message.id, { part: { kind: "point", keys: { series: "oct", x: "10-13" } } });
    server.transient(ev);
    const patch = await server.next((f) => f.t === "card.patch");
    expect(patch.baseRev).toBe(1);
    expect(patch.cause).toBe(ev.eventId);
    expect((patch.ops as { op: string }[]).map((o) => o.op)).toEqual(["insert", "insert", "remove"]);
    expect(seen[0]?.card.title).toBe("10 月数据月报");
    expect(seen[0]?.value).toEqual({ value: "wrong" });
    expect(seen[0]?.part).toEqual({ kind: "point", keys: { series: "oct", x: "10-13" } });
    expect(seen[0]?.stored).toBe(false);
    await new Promise((r) => setTimeout(r, 50));
    expect(bot.knownCard(r.message.id)?.rev).toBe(2);
    // The patch answered the event: no automatic resolve.
    expect(server.of("interaction.resolve")).toHaveLength(0);
  });

  test("a handler that does not answer gets ignored; one that throws gets failed", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    bot.onError(() => {});
    bot.onCardEvent((ctx) => {
      if (ctx.blockId === "trend") throw new Error("boom");
    });
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    const a = liveEvent(r.message.id);
    const b = liveEvent(r.message.id, { blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-13", y: 2268, index: 1 } });
    server.transient(a);
    server.transient(b);
    const ra = await server.next((f) => f.t === "interaction.resolve" && f.eventId === a.eventId);
    const rb = await server.next((f) => f.t === "interaction.resolve" && f.eventId === b.eventId);
    expect(ra.result).toBe("ignored");
    expect(rb.result).toBe("failed");
  });

  test("latest wins: isLatest is false when a newer event of the block arrived", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const latest: boolean[] = [];
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => {
      release = r;
    });
    bot.onCardEvent(async (ctx) => {
      if (latest.length === 0) await gate;
      latest.push(ctx.isLatest());
      await ctx.resolve("ignored", "没有 10-14 之前的数");
    });
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    server.transient(liveEvent(r.message.id));
    await new Promise((res) => setTimeout(res, 20));
    server.transient(liveEvent(r.message.id, { value: { value: "ok" } }));
    await new Promise((res) => setTimeout(res, 20));
    release();
    await server.next((f) => f.t === "interaction.resolve" && server.of("interaction.resolve").length === 2);
    expect(latest).toEqual([false, true]);
    expect(server.of("interaction.resolve")[0]?.note).toBe("没有 10-14 之前的数");
  });
});

describe("patches", () => {
  test("a stale rev rebases on the newest rev and sends again", async () => {
    const { server, revs } = cardServer();
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    // Someone else's change made rev 2; the bot has not seen it yet.
    const rev2 = { ...report(), rev: 2 };
    revs.set(r.message.id, rev2);
    const p = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 40 } }]);
    await server.next((f) => f.t === "card.patch" && f.baseRev === 1);
    server.event({ seq: 1, groupId: GROUP_ID, at: Date.now(), actorId: BOT_ID, type: "message.updated", data: { messageId: r.message.id, body: "", editedAt: Date.now(), streaming: false, cardV2: rev2 } });
    const m = await p;
    expect(m.cardV2?.rev).toBe(3);
    expect(server.of("card.patch").map((f) => f.baseRev)).toEqual([1, 2]);
  });

  test("a stale rev whose ops no longer fit throws conflict; an explicit baseRev never rebases", async () => {
    const { server, revs } = cardServer();
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    const card = report();
    // Rev 2 (not seen by the bot yet) removed the note.
    const rev2: CardV2 = {
      ...card,
      rev: 2,
      blocks: card.blocks.filter((b) => b.id !== "note").map((b) => (b.id === "check_col" ? ({ ...b, children: ["check"] } as typeof b) : b)),
    };
    revs.set(r.message.id, rev2);
    const p = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "note", props: { help: "x" } }]).catch((e) => e as ChatError);
    await server.next((f) => f.t === "card.patch");
    server.event({ seq: 1, groupId: GROUP_ID, at: Date.now(), actorId: BOT_ID, type: "message.updated", data: { messageId: r.message.id, body: "", editedAt: Date.now(), streaming: false, cardV2: rev2 } });
    const err = (await p) as ChatError;
    expect(err).toBeInstanceOf(ChatError);
    expect(err.code).toBe("conflict");
    expect(err.reason).toBe("card.stale_rev");
    expect(server.of("card.patch")).toHaveLength(1);
    const explicit = (await bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "check", props: { help: "x" } }], { baseRev: 1 }).catch((e) => e)) as ChatError;
    expect(explicit.reason).toBe("card.stale_rev");
    expect(server.of("card.patch")).toHaveLength(2);
    expect(opsFit(rev2, [{ op: "remove", id: "note" }])).toBe(false);
    expect(opsFit(rev2, [{ op: "set", id: "check", props: { help: "x" } }])).toBe(true);
  });

  test("rate_limited: later patches wait and merge into one frame per cause", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    let limited = false;
    server.on("card.patch", (f) => {
      if (!limited) {
        limited = true;
        return { ok: false, error: { code: "rate_limited", message: "slow down" } };
      }
      const next = { ...report(), rev: (f.baseRev as number) + 1 };
      return { ok: true, result: { message: message({ id: r.message.id, kind: "card", cardV2: next }), rev: next.rev } };
    });
    const started = Date.now();
    const p1 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 39 } }], { cause: "EV1" });
    await server.next((f) => f.t === "card.patch");
    const p2 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 40 } }], { cause: "EV2" });
    const p3 = bot.patchCard(GROUP_ID, r.message.id, (d) => d.set("m_dau", { value: 2341 }));
    const [m1, m2, m3] = await Promise.all([p1, p2, p3]);
    expect(Date.now() - started).toBeGreaterThanOrEqual(900);
    // The server resolves only the frame's cause: EV1 and EV2 each get a frame; the part
    // without a cause joins EV2.
    const frames = server.of("card.patch");
    expect(frames).toHaveLength(3);
    expect(frames[1]?.cause).toBe("EV1");
    expect((frames[1]?.ops as unknown[]).length).toBe(1);
    expect(frames[2]?.cause).toBe("EV2");
    expect((frames[2]?.ops as unknown[]).length).toBe(2);
    expect(m1.cardV2?.rev).toBe(2);
    expect(m2.cardV2?.rev).toBe(3);
    expect(m3).toEqual(m2);
  });

  test("a merged patch the server rejects: each part goes again alone, so one bad part fails only itself", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    let limited = false;
    let rev = 1;
    server.on("card.patch", (f) => {
      if (!limited) {
        limited = true;
        return { ok: false, error: { code: "rate_limited", message: "slow down" } };
      }
      if ((f.ops as { props?: { value?: number } }[]).some((o) => o.props?.value === -1)) {
        return { ok: false, error: { code: "invalid", message: "bad value", reason: "card.bad_op" } };
      }
      rev++;
      return { ok: true, result: { message: message({ id: r.message.id, kind: "card", cardV2: { ...report(), rev } }), rev } };
    });
    const p1 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 39 } }]);
    await server.next((f) => f.t === "card.patch");
    const good = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 40 } }]);
    const bad = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_dau", props: { value: -1 } }]).catch((e) => e as ChatError);
    await p1;
    expect((await good).cardV2?.rev).toBeGreaterThan(1);
    const err = (await bad) as ChatError;
    expect(err).toBeInstanceOf(ChatError);
    expect(err.reason).toBe("card.bad_op");
  });

  /** A bot with a short request timeout, so a lost ack shows as `timeout` quickly. */
  async function quickBot(server: FakeServer): Promise<Bot> {
    const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: new MemoryStateStore(), logLevel: "error", stopTimeoutMs: 500, requestTimeoutMs: 300 });
    bots.push(bot);
    await bot.start();
    return bot;
  }

  test("a merged patch whose ack is lost goes again as the same frame; duplicate counts as applied", async () => {
    const server = new FakeServer();
    const r0 = message({ id: "01JC0MSG000000000000000001", kind: "card", cardV2: report() });
    server.on("send", () => ({ ok: true, result: { message: r0 } }));
    const bot = await quickBot(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    let rev = 1;
    let limited = false;
    const applied = new Map<string, unknown>();
    server.on("card.patch", (f) => {
      if (!limited) {
        limited = true;
        return { ok: false, error: { code: "rate_limited", message: "slow down" } };
      }
      const id = f.clientPatchId as string;
      const prev = applied.get(id);
      if (prev) return { ok: true, result: { ...(prev as object), duplicate: true } };
      rev++;
      const result = { message: message({ id: r.message.id, kind: "card", cardV2: { ...report(), rev } }), rev };
      applied.set(id, result);
      return "none"; // committed, but the ack is lost
    });
    const p1 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 39 } }]);
    await server.next((f) => f.t === "card.patch");
    const p2 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 40 } }]);
    const p3 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_dau", props: { value: 41 } }]);
    const [m1, m2, m3] = await Promise.all([p1, p2, p3]);
    const frames = server.of("card.patch");
    expect(frames).toHaveLength(3);
    expect((frames[1]?.ops as unknown[]).length).toBe(3);
    expect(frames[2]?.clientPatchId).toBe(frames[1]?.clientPatchId as string);
    expect(frames[2]?.ops).toEqual(frames[1]?.ops);
    expect(frames[2]?.baseRev).toBe(frames[1]?.baseRev as number);
    expect(applied.size).toBe(1);
    expect(m1.cardV2?.rev).toBe(2);
    expect(m2).toEqual(m1);
    expect(m3).toEqual(m1);
  });

  test("a merged patch that times out and then gets a definite rejection: each part goes again alone", async () => {
    const server = new FakeServer();
    const r0 = message({ id: "01JC0MSG000000000000000001", kind: "card", cardV2: report() });
    server.on("send", () => ({ ok: true, result: { message: r0 } }));
    const bot = await quickBot(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    let rev = 1;
    let calls = 0;
    server.on("card.patch", (f) => {
      calls++;
      if (calls === 1) return { ok: false, error: { code: "rate_limited", message: "slow down" } };
      if (calls === 2) return "none"; // the merged frame: no answer, nothing applied
      if ((f.ops as { props?: { value?: number } }[]).some((o) => o.props?.value === -1)) {
        return { ok: false, error: { code: "invalid", message: "bad value", reason: "card.bad_op" } };
      }
      rev++;
      return { ok: true, result: { message: message({ id: r.message.id, kind: "card", cardV2: { ...report(), rev } }), rev } };
    });
    const p1 = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 39 } }]);
    await server.next((f) => f.t === "card.patch");
    const good = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_groups", props: { value: 40 } }]);
    const bad = bot.patchCard(GROUP_ID, r.message.id, [{ op: "set", id: "m_dau", props: { value: -1 } }]).catch((e) => e as ChatError);
    await p1;
    expect((await good).cardV2?.rev).toBe(3);
    const err = (await bad) as ChatError;
    expect(err).toBeInstanceOf(ChatError);
    expect(err.reason).toBe("card.bad_op");
    const frames = server.of("card.patch");
    // rate_limited, merged (no answer), merged again with the same id (rejected), then each alone.
    expect(frames).toHaveLength(6);
    expect((frames[1]?.ops as unknown[]).length).toBe(3);
    expect(frames[2]?.clientPatchId).toBe(frames[1]?.clientPatchId as string);
    for (const f of frames.slice(3)) {
      expect((f.ops as unknown[]).length).toBe(1);
      expect(f.clientPatchId).not.toBe(frames[1]?.clientPatchId as string);
    }
  });
});

describe("answers", () => {
  test("onCardAnswer reads 2.1 input values and patches with the answer as cause", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const got: CardAnswerContext[] = [];
    bot.onCardAnswer(async (ctx) => {
      got.push(ctx);
      await ctx.patch([{ op: "card", props: { actions: [] } }]);
    });
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    server.event({
      seq: 1,
      groupId: GROUP_ID,
      at: Date.now(),
      actorId: HUMAN_ID,
      type: "card.answered",
      data: { messageId: r.message.id, answerId: "AN1", askId: "verify", eventId: "EV3", rev: 1, event: "action.press", actionId: "send", inputs: { check: { value: "wrong" }, note: { value: "后台是 7 个" } } },
    });
    const patch = await server.next((f) => f.t === "card.patch");
    expect(patch.cause).toBe("EV3");
    const a = got[0] as CardAnswerContext;
    expect(a.stored).toBe(true);
    expect(a.askId).toBe("verify");
    expect(a.answerId).toBe("AN1");
    expect(a.name).toBe("action.press");
    expect(a.choice("check")).toBe("wrong");
    expect(a.text("note")).toBe("后台是 7 个");
    expect(a.action?.id).toBe("send");
    expect(a.cancelled).toBe(false);
  });

  test("an answer caught up after a restart reads the card from the history", async () => {
    const server = new FakeServer();
    const store = new MemoryStateStore();
    const card = report();
    const mid = "01JC0MSG000000000000000099";
    store.save({ v: 1, memberId: BOT_ID, cursors: { [GROUP_ID]: 0 }, following: {}, cards: { [mid]: { groupId: GROUP_ID, topicId: TOPIC_ID } } });
    server.groups = [{ group: { id: GROUP_ID, kind: "group", name: "夹夹增长", createdAt: 1 }, lastSeq: 1, readSeq: 0, unread: 0, mentions: 0 }];
    server.on("fetch", () => ({
      ok: true,
      result: {
        events: [{ seq: 1, groupId: GROUP_ID, at: 1, actorId: HUMAN_ID, type: "card.answered", data: { messageId: mid, eventId: "EV9", rev: 1, actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } } }],
        hasMore: false,
        lastSeq: 1,
      },
    }));
    server.on("history", () => ({ ok: true, result: { messages: [message({ id: mid, kind: "card", card: { v: 1, title: card.title, blocks: [] }, cardV2: card })], hasMore: false } }));
    const got: CardAnswerContext[] = [];
    const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: store, logLevel: "error" });
    bots.push(bot);
    bot.onCardAnswer((ctx) => {
      got.push(ctx);
    });
    await bot.start();
    await server.next((f) => f.t === "history");
    for (let i = 0; i < 100 && got.length === 0; i++) await new Promise((r) => setTimeout(r, 10));
    expect(got[0]?.catchUp).toBe(true);
    expect(got[0]?.card.title).toBe("10 月数据月报");
    expect(got[0]?.choice("check")).toBe("ok");
  });
});

describe("causes outside the server's window", () => {
  /** A server that, like the real one, takes no cause older than 30 minutes. */
  function strictServer(old: Set<string>) {
    const { server, revs } = cardServer();
    const bad = { ok: false as const, error: { code: "invalid", message: "cause is not a recent event of this card", reason: "interact.bad_cause" } };
    const send = server.handler("send") as (f: Frame) => Reply;
    const patch = server.handler("card.patch") as (f: Frame) => Reply;
    server.on("send", (f) => (old.has(f.causeEventId as string) ? bad : send(f)));
    server.on("card.patch", (f) => (old.has(f.cause as string) ? bad : patch(f)));
    server.on("edit", (f) => {
      if (old.has(f.causeEventId as string)) return bad;
      const next = f.cardV2 as CardV2;
      revs.set(f.messageId as string, next);
      return { ok: true, result: { message: message({ id: f.messageId as string, kind: "card", card: { v: 1, title: next.title, blocks: [] }, cardV2: next }) } };
    });
    return server;
  }

  test("onCardAnswer on catch-up older than 30 minutes: patch, reply, postCard and markAnswered go without the cause", async () => {
    const server = strictServer(new Set(["EVOLD"]));
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    const done: string[] = [];
    bot.onCardAnswer(async (ctx) => {
      await ctx.patch([{ op: "set", id: "m_groups", props: { value: 40 } }]);
      done.push("patch");
      await ctx.reply("收到");
      done.push("reply");
      await ctx.postCard(report());
      done.push("postCard");
      await ctx.markAnswered();
      done.push("markAnswered");
    });
    server.event({
      seq: 1,
      groupId: GROUP_ID,
      at: Date.now() - 60 * 60_000,
      actorId: HUMAN_ID,
      type: "card.answered",
      data: { messageId: r.message.id, eventId: "EVOLD", rev: 1, event: "action.press", actionId: "send", inputs: { check: { value: "wrong" } } },
    });
    await server.next((f) => f.t === "edit");
    for (let i = 0; i < 50 && done.length < 4; i++) await new Promise((res) => setTimeout(res, 10));
    expect(done).toEqual(["patch", "reply", "postCard", "markAnswered"]);
    // Nothing was sent with the old cause: the SDK left it out instead of retrying.
    expect(server.of("card.patch").map((f) => f.cause)).toEqual([undefined]);
    expect(server.of("send").filter((f) => f.causeEventId !== undefined)).toHaveLength(0);
    expect(server.of("edit")[0]?.causeEventId).toBeUndefined();
  });

  test("a cause the server does not take (clock skew, an older server) is dropped and the write sent again", async () => {
    const server = strictServer(new Set(["EVSKEW"]));
    const bot = await start(server);
    const r = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, report());
    const errors: unknown[] = [];
    bot.onError((e) => errors.push(e));
    let patched: number | undefined;
    bot.onCardEvent(async (ctx) => {
      patched = (await ctx.patch([{ op: "set", id: "m_groups", props: { value: 40 } }])).cardV2?.rev;
    });
    server.transient(liveEvent(r.message.id, { eventId: "EVSKEW" }));
    await server.next((f) => f.t === "card.patch" && f.cause === undefined);
    for (let i = 0; i < 50 && patched === undefined; i++) await new Promise((res) => setTimeout(res, 10));
    expect(patched).toBe(2);
    expect(server.of("card.patch").map((f) => f.cause)).toEqual(["EVSKEW", undefined]);
    expect(errors).toHaveLength(0);
  });
});

describe("page events", () => {
  test("emit: ctx.postMessage names the page event as cause; without page causes it posts without one", async () => {
    for (const takesPageCauses of [true, false]) {
      const { server } = cardServer();
      const send = server.handler("send") as (f: Frame) => Reply;
      server.on("send", (f) =>
        f.causeEventId === "PE7" && !takesPageCauses
          ? { ok: false, error: { code: "invalid", message: "cause is not a recent event of this card", reason: "interact.bad_cause" } }
          : send(f),
      );
      const bot = await start(server);
      const errors: unknown[] = [];
      bot.onError((e) => errors.push(e));
      bot.onArtifactEvent(async (ctx) => {
        await ctx.postMessage("收到，我去看一下");
      });
      server.transient({
        type: "artifact.event",
        groupId: GROUP_ID,
        topicId: TOPIC_ID,
        artifactId: "01JC0ART00000000000000OCT3",
        version: 3,
        eventId: "PE7",
        name: "report.flag",
        kind: "emit",
        payload: null,
        actorId: HUMAN_ID,
        at: Date.now(),
      });
      const posted = await server.next((f) => f.t === "send" && f.body === "收到，我去看一下" && (f.causeEventId === undefined) !== takesPageCauses);
      expect(posted.causeEventId).toBe(takesPageCauses ? "PE7" : undefined);
      await new Promise((res) => setTimeout(res, 50));
      expect(server.of("send").filter((f) => f.body === "收到，我去看一下")).toHaveLength(takesPageCauses ? 1 : 2);
      // The post answered the emit: no automatic resolve, no handler error.
      expect(server.of("interaction.resolve")).toHaveLength(0);
      expect(errors).toHaveLength(0);
      await bot.stop();
    }
  });

  test("ask: reply once; emit without an answer: ignored; ask without an answer: declined; pushData has the cause", async () => {
    const server = new FakeServer();
    server.on("artifact.data", () => ({ ok: true, result: { dataRev: 1 } }));
    const bot = await start(server);
    bot.onArtifactEvent(async (ctx) => {
      if (ctx.name === "filter.pick") {
        await ctx.pushData({ channel: ctx.payload });
        await ctx.reply({ rows: 4 });
        await expect(ctx.reply({ rows: 5 })).rejects.toThrow(ChatError);
      }
    });
    const ev = (name: string, kind: "emit" | "ask", eventId: string) => ({
      type: "artifact.event",
      groupId: GROUP_ID,
      topicId: TOPIC_ID,
      artifactId: "01JC0ART00000000000000OCT3",
      version: 3,
      eventId,
      name,
      kind,
      payload: "xhs",
      actorId: HUMAN_ID,
      at: Date.now(),
    });
    server.transient(ev("filter.pick", "ask", "PE1"));
    server.transient(ev("report.flag", "emit", "PE2"));
    server.transient(ev("report.ask", "ask", "PE3"));
    const data = await server.next((f) => f.t === "artifact.data");
    expect(data.cause).toBe("PE1");
    expect(data.data).toEqual({ channel: "xhs" });
    const reply = await server.next((f) => f.t === "artifact.reply" && f.eventId === "PE1");
    expect(reply.ok).toBe(true);
    expect(reply.payload).toEqual({ rows: 4 });
    expect((await server.next((f) => f.t === "interaction.resolve" && f.eventId === "PE2")).result).toBe("ignored");
    const declined = await server.next((f) => f.t === "artifact.reply" && f.eventId === "PE3");
    expect(declined.ok).toBe(false);
    expect((declined.error as { code: string }).code).toBe("declined");
    expect(server.of("artifact.reply").filter((f) => f.eventId === "PE1")).toHaveLength(1);
  });
});

describe("refs (引用提问)", () => {
  test("ctx.refs and ctx.question; resolveRef finds the cell and the node, current until a newer rev", async () => {
    const { server } = cardServer();
    const bot = await start(server);
    const card = composeCard({
      intent: "info",
      title: "10 月数据月报",
      blocks: [
        blocks.table({ id: "channels", title: "渠道表", columns: ["渠道", "新增群"], rows: [["小红书", "14"], ["B 站", "9"]], rowKeys: ["xhs", "bili"] }),
        blocks.diagram({ id: "flow", source: "flowchart LR\n  a[收到消息] --> b{要回复吗}\n  b --> c[回复]" }),
      ],
    });
    const posted = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, card);
    const refs = [
      { messageId: posted.message.id, cardRev: 1, blockId: "channels", part: parts.cell("bili", 1), label: "10 月数据月报 · 渠道表 · B 站 · 新增群 9", value: { row: "B 站", column: "新增群", display: "9", raw: 9 }, sourceAuthorId: BOT_ID, sourceTopicId: TOPIC_ID },
      { messageId: posted.message.id, cardRev: 1, blockId: "flow", part: parts.node("b"), label: "10 月数据月报 · 图 · 要回复吗", sourceAuthorId: BOT_ID, sourceTopicId: TOPIC_ID },
    ];
    const body = `> ↳ ${refs[0]?.label}\n> ↳ ${refs[1]?.label}\n\n为什么比上月少了 1 个？`;
    const seen: { question: string; n: number; cell: unknown; node: unknown; current: boolean[] }[] = [];
    bot.onMention(async (ctx) => {
      const [cell, node] = await Promise.all(ctx.refs.map((r) => ctx.resolveRef(r)));
      seen.push({ question: ctx.question, n: ctx.refs.length, cell: cell?.part, node: node?.part, current: [cell?.current ?? false, node?.current ?? false] });
    });
    server.event({
      seq: 1,
      groupId: GROUP_ID,
      at: Date.now(),
      actorId: HUMAN_ID,
      type: "message.created",
      data: { message: message({ id: "01JC0MSG0000000000000000Q1", authorId: HUMAN_ID, body, mentions: [BOT_ID], blockRefs: refs }) },
    });
    for (let i = 0; i < 100 && seen.length === 0; i++) await new Promise((r) => setTimeout(r, 10));
    expect(seen[0]?.n).toBe(2);
    expect(seen[0]?.question).toBe("为什么比上月少了 1 个？");
    expect(seen[0]?.cell).toEqual({ row: "B 站", column: "新增群", display: "9" });
    expect(seen[0]?.node).toEqual({ node: "b", label: "要回复吗" });
    expect(seen[0]?.current).toEqual([true, true]);
    // After a patch (rev 2), the same ref is no longer current.
    await bot.patchCard(GROUP_ID, posted.message.id, [{ op: "set", id: "channels", props: { title: "渠道" } }]);
    const again = await bot.resolveRef(refs[0] as never, GROUP_ID);
    expect(again.current).toBe(false);
  });

  test("prefix strip is exact; a different body stays unchanged", () => {
    const refs = [{ label: "A · 表格 · 整行", blockId: "t", messageId: "m", sourceTopicId: "t" }];
    expect(stripRefPrefix("> ↳ A · 表格 · 整行\n\n问题", refs as never)).toBe("问题");
    expect(stripRefPrefix("> ↳ A · 表格 · 整行", refs as never)).toBe("");
    expect(stripRefPrefix("> ↳ B\n\n问题", refs as never)).toBe("> ↳ B\n\n问题");
    expect(stripRefPrefix("> ↳ 原消息已删除\n\n问题", [{ ...refs[0], label: "", deleted: true }] as never)).toBe("问题");
  });

  test("markdown block counter: headings, lists, fences and tables", () => {
    const md = "# 标题\n\n第一段\n继续\n\n- a\n- b\n\n  - c\n\n```mermaid\nflowchart LR\n\n  a --> b\n```\n\n| 渠道 | 新增群 |\n| --- | --- |\n| 小红书 | 14 |\n\n---\n> 引用";
    expect(markdownBlocks(md).map((b) => b.kind)).toEqual(["heading", "paragraph", "list", "code", "table", "rule", "quote"]);
    expect(markdownBlocks(md)[3]?.lang).toBe("mermaid");
  });
});

describe("Card v2.1 builders", () => {
  test("composeCard builds the 7.1 report from nested containers, equal to the fixture", () => {
    const card = composeCard({
      intent: "done",
      title: "10 月数据月报",
      summary: "新增群 38 个，比 9 月多 12%；日活 2,340。",
      fallback: "Ledger 发了 10 月数据月报：新增群 38，日活 2,340。请看一眼数据对不对。",
      data: { weeks: ["10-06", "10-13", "10-20", "10-27"] },
      ask: { id: "verify", from: ["requester"], blocking: false, promise: "你确认之前，我不会把月报发到全员群。" },
      blocks: [
        blocks.row("kpis", [
          blocks.metric({ id: "m_groups", label: "新增群", value: 38, delta: 4, better: "up", note: "比 9 月" }),
          blocks.metric({ id: "m_dau", label: "日活", value: 2340, delta: 90, better: "up", note: "比 9 月" }),
          blocks.metric({ id: "m_reply", label: "平均回复", value: 1.2, unit: "秒", delta: -0.3, better: "down", tone: "good", note: "比 9 月" }),
        ]),
        blocks.row(
          "main",
          [
            blocks.chart({
              id: "trend",
              kind: "line",
              label: "日活 · 近 4 周",
              emphasis: "high",
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
                id: "check",
                label: "这个数据对不对？",
                required: true,
                options: [
                  { id: "ok", label: "对" },
                  { id: "wrong", label: "有问题" },
                ],
                deliver: "onSubmit",
                events: ["choice.change"],
              }),
              blocks.input({ id: "note", kind: "textarea", label: "补充一句（可选）", required: false, maxLength: 300 }),
            ]),
          ],
          { weights: [2, 1] },
        ),
        blocks.ref({ id: "full", emphasis: "low", label: "10 月数据月报 v3", target: { kind: "artifact", artifactId: "01JC0ART00000000000000OCT3", version: 3 } }),
      ],
      actions: [{ id: "send", label: "发给 Ledger", style: "primary", kind: "submit", inputs: ["check", "note"] }],
    });
    expect(JSON.parse(JSON.stringify(card))).toEqual(report());
  });

  test("every 2.1 container, variant, buttons, diagram and embed builds a valid card", () => {
    const card = composeCard({
      intent: "ask",
      title: "4.0 怎么排",
      ask: { from: ["owners"] },
      blocks: [
        blocks.tabs({
          id: "tabs",
          tabs: [
            { id: "t1", label: "方案", children: [blocks.compare({ id: "cmp", options: [{ id: "A", label: "按活动", children: [blocks.text("a_why", "找照片最快")] }, { id: "B", label: "按时间" }] })] },
            { id: "t2", label: "流程", children: [blocks.diagram({ id: "flow", source: "flowchart LR\n a[发图] --> b[整理]" })] },
          ],
        }),
        blocks.group({ id: "g", title: "其他", children: [blocks.buttons({ id: "btn", buttons: [{ id: "split", label: "按渠道拆开" }], events: ["button.press"] }), blocks.embed({ id: "page", artifactId: "01JC0ART0000000000000000P1", version: 1, height: 360 })] }),
      ],
      actions: [{ id: "go", label: "就这样", style: "primary" }],
    });
    expect(card.catalog).toBe("tc.card/2.1");
    expect(card.blocks.map((b) => b.id)).toEqual(["tabs", "cmp", "a_why", "flow", "g", "btn", "page"]);
    expect(card.blocks.find((b) => b.id === "flow")?.fallbackText).toBe("图：发图、整理");
    const variants = [
      blocks.rank({ id: "rank", label: "排个序", items: [{ id: "a", label: "照片" }, { id: "b", label: "分享" }] }),
      blocks.rating({ id: "rating", label: "清楚吗" }),
      blocks.scale({ id: "scale", label: "会推荐吗", min: 0, max: 10, ends: ["不会", "一定会"] }),
      blocks.confirm({ id: "confirm", label: "可以发吗", yes: "可以", no: "不行" }),
      blocks.imageChoice({ id: "img", label: "哪张", options: [{ id: "A", label: "A", fileId: "f1" }, { id: "B", label: "B", fileId: "f2" }] }),
      blocks.matrix({ id: "mx", label: "放哪版", rows: [{ id: "photos", label: "照片" }], columns: [{ id: "v40", label: "4.0" }, { id: "v41", label: "4.1" }] }),
      blocks.slots({ id: "slots", label: "什么时候", tz: "Asia/Shanghai", slots: [{ id: "t1", start: 1, end: 2 }] }),
      blocks.poll({ id: "poll", label: "午饭", options: [{ id: "a", label: "面" }, { id: "b", label: "饭" }], voters: "group", closesAt: 9_999_999_999_999 }),
      blocks.slider({ id: "budget", label: "预算", min: 0, max: 100, step: 5, unit: "%" }),
      blocks.decision({ id: "who", label: "谁值班", options: [{ id: "a", label: "Alex", recommended: true }, { id: "b", label: "Ben" }], multiple: true, minSelect: 1, maxSelect: 2, allowOther: { label: "其他人" } }),
    ];
    for (const v of variants) {
      const c = composeCard({ intent: "ask", title: "问一下", ask: { from: ["owners"] }, blocks: [v], actions: [{ id: "s", label: "提交", style: "primary", inputs: [v.id] }] });
      expect(c.catalog).toBe(v.type === "input" || v.type === "decision" ? undefined : "tc.card/2.1");
    }
    const q = composeCard({ intent: "ask", title: "反馈", ask: { from: ["owners"] }, blocks: [blocks.questionnaire({ id: "qs", children: variants.slice(0, 3) })] });
    expect(q.blocks).toHaveLength(4);
  });

  test("write rules still apply: a row of 4, a style key and a bad diagram throw", () => {
    const m = (id: string) => blocks.metric({ id, label: id, value: 1 });
    expect(() => composeCard({ intent: "info", title: "x", blocks: [blocks.row("r", [m("a"), m("b"), m("c"), m("d")])] })).toThrow(ChatError);
    expect(() => composeCard({ intent: "info", title: "x", blocks: [{ ...m("a"), color: "red" } as never] })).toThrow(/style/);
    expect(() => composeCard({ intent: "info", title: "x", blocks: [blocks.diagram({ id: "d", source: "flowchart LR\n click a call x()" })] })).toThrow(ChatError);
    expect(checkDiagram("pie\n a: 1")).toEqual({ ok: false, reason: "unsupported:pie" });
    expect(checkDiagram("sequenceDiagram\n A->>B: hi")).toEqual({ ok: true, type: "sequenceDiagram" });
    expect(mermaid("flowchart LR\n a --> b\n")).toBe("```mermaid\nflowchart LR\n a --> b\n```");
  });

  test("CardDraft inserts a container with its nested children", () => {
    const d = new CardDraft(report());
    d.insert(blocks.group({ id: "drill", title: "拆开", children: [blocks.text("t", "x")] }), { after: "main" }).ask({ id: "round2", from: ["requester"] });
    expect(d.ops).toEqual([
      { op: "insert", block: { type: "group", id: "drill", title: "拆开", children: ["t"] }, after: "main" },
      { op: "insert", block: { type: "text", id: "t", text: "x" } },
      { op: "card", props: { ask: { id: "round2", from: ["requester"] } } },
    ]);
    expect(opsFit(report(), d.ops)).toBe(true);
  });
});

// The server's rule, loaded at run time (its file is outside this package's typecheck).
const serverPattern = (await import(join(import.meta.dir, "../../../server/src/files/pattern.ts"))) as {
  patternProblem(p: string): string | undefined;
  MAX_PATTERN_INPUT: number;
};

describe("page event declarations", () => {
  const PATTERNS = [
    "^[a-z]+$",
    "^(a+)+$",
    "^\\d{3}-\\d{4}$",
    "(?:ab|cd)*e?",
    "(?<year>\\d{4})",
    "\\p{L}+",
    "[\\]a]",
    "\\bword\\b",
    "(a)\\1",
    "(?<y>a)\\k<y>",
    "a(?=b)",
    "a(?!b)",
    "(?<=a)b",
    "(?<!a)b",
    "a{1001}",
    "(a{0,500}){0,500}",
    "(abc",
    "a\\",
  ];

  test("the SDK refuses the patterns the server refuses, with the same message", () => {
    for (const p of PATTERNS) expect([p, patternProblem(p)]).toEqual([p, serverPattern.patternProblem(p)]);
    expect(patternProblem("(a)\\1")).toBe("pattern: backreferences are not supported");
    expect(patternProblem("a(?=b)")).toBe("pattern: lookaround is not supported");
    expect(patternProblem("^(a+)+$")).toBeUndefined();
    expect(MAX_PATTERN_INPUT).toBe(serverPattern.MAX_PATTERN_INPUT);
  });

  test("the SDK and the server use one pattern module, not two copies", () => {
    expect(patternProblem).toBe(serverPattern.patternProblem);
  });

  test("a declaration with a refused pattern fails before any request", async () => {
    const decl = [{ name: "form.submit", schema: { type: "object", properties: { code: { type: "string", pattern: "(?<!x)y" } } } }];
    expect(() => checkArtifactEvents(decl as never)).toThrow(ChatError);
    let calls = 0;
    const http = new HttpClient({ url: "http://127.0.0.1:1", token: "t", fetch: (async () => {
      calls++;
      return new Response("{}", { status: 500 });
    }) as never });
    const err = (await http
      .publishHtml({ groupId: GROUP_ID, topicId: TOPIC_ID, title: "t", html: "<p>x</p>", events: decl as never })
      .catch((e) => e)) as ChatError;
    expect(err).toBeInstanceOf(ChatError);
    expect(err.code).toBe("invalid");
    expect(err.reason).toBe("artifact.bad_schema");
    const err2 = (await http
      .createArtifact({ groupId: GROUP_ID, topicId: TOPIC_ID, title: "t", kind: "html", entry: "index.html", files: [], events: decl } as never)
      .catch((e) => e)) as ChatError;
    expect(err2.reason).toBe("artifact.bad_schema");
    const err3 = (await http.addArtifactVersion("01JC0ART000000000000000001", { entry: "index.html", files: [], events: decl } as never).catch((e) => e)) as ChatError;
    expect(err3.reason).toBe("artifact.bad_schema");
    expect(calls).toBe(0);
    expect(() => checkArtifactEvents([{ name: "form.submit", schema: { type: "string", pattern: "^[a-z]{1,8}$" } }])).not.toThrow();
  });
});
