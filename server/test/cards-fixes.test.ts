// Review fixes of the 1.1 interaction loop in the group room: audit redaction, private
// polls and anonymous questionnaires, concurrent card writes, refs, edit marks, requester
// asks, the retract window and the topic status of blocking asks.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import type { BotsEnv } from "../src/bots/env";
import { registerRequesterResolver } from "../src/cards2/delivery";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, clientMsgId, createGroup, createMember, protocolViolations, type TestMember, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const FEATURES = ["interact/1", "refs/1"];

async function setup() {
  const owner = await createMember({ level: "owner", name: "Joye" });
  const alice = await createMember({ name: "Alice" });
  const bob = await createMember({ name: "Bob" });
  const bot = await createMember({ kind: "bot", name: "Ledger" });
  const gid = await createGroup(owner, [alice.id, bob.id, bot.id]);
  const connect = async (m: TestMember, features: string[] = FEATURES) => (await Client.connect(m.token, { features })).c;
  const [co, ca, cb, cbot] = await Promise.all([connect(owner), connect(alice), connect(bob), connect(bot)]);
  return { owner, alice, bob, bot, gid, co, ca, cb, cbot, connect };
}

const base = (extra: Partial<CardV2>): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "info",
  title: "卡片",
  fallback: "卡片",
  blocks: [],
  ...extra,
});

const send = (c: Client, gid: string, cardV2: CardV2, extra: Record<string, unknown> = {}) =>
  c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2, ...extra });
const ev = (c: Client, gid: string, messageId: string, f: Record<string, unknown>) =>
  c.request({ t: "card.event", groupId: gid, messageId, rev: 1, clientEventId: crypto.randomUUID(), ...f });
const roomOf = (gid: string) => env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("card event audit", () => {
  it("redacts the value before it cuts it to 1 KiB of UTF-8", async () => {
    const { alice, gid, ca, cbot } = await setup();
    const card = base({ blocks: [{ id: "note", type: "input", kind: "text", label: "备注", events: ["input.change"] }] });
    const m = (await send(cbot, gid, card)).message;
    // A token that crosses the 1 KiB cut of the JSON text.
    const token = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
    const r1 = await ev(ca, gid, m.id, { blockId: "note", event: "input.change", value: { value: `${"x".repeat(985)} ${token}` } });
    expect(r1, JSON.stringify(r1)).toMatchObject({ ok: true });
    await sleep(1100);
    const r2 = await ev(ca, gid, m.id, { blockId: "note", event: "input.change", value: { value: "汉".repeat(1000) } });
    expect(r2).toMatchObject({ ok: true });
    const rows = async () =>
      (await env.DB.prepare("SELECT details FROM audit WHERE action = 'card.event' AND target = ? ORDER BY rowid").bind(m.id).all<{ details: string }>()).results;
    for (let i = 0; i < 50 && (await rows()).length < 2; i++) await sleep(100);
    const values = (await rows()).map((r) => (JSON.parse(r.details) as { value: string }).value);
    expect(values).toHaveLength(2);
    expect(values[0]).not.toContain("ghp_");
    expect(values[0]).toContain("<redacted>");
    for (const v of values) expect(new TextEncoder().encode(v).length).toBeLessThanOrEqual(1024);
    void alice;
    for (const c of [ca, cbot]) c.close();
  });
});

/** A blocking yes/no ask with an immediate choice and a chart outside its decision surface. */
const quick = (from: string[], extra: Partial<CardV2> = {}): CardV2 =>
  base({
    intent: "ask",
    title: "今天发版吗？",
    fallback: "今天发版吗？",
    ask: { from },
    blocks: [
      { id: "pick", type: "decision", deliver: "immediate", options: [{ id: "yes", label: "发" }, { id: "no", label: "不发" }] },
      { id: "info", type: "text", text: "改动都测过了。" },
    ],
    ...extra,
  });

describe("retract window", () => {
  it("stays open for an answer accepted at an older rev with the same decision surface", async () => {
    const { alice, gid, ca, cbot } = await setup();
    const m = (await send(cbot, gid, quick([alice.id]))).message;
    await cbot.ok({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 1, ops: [{ op: "set", id: "info", props: { text: "又测了一遍。" } }] });
    const a = await ev(ca, gid, m.id, { rev: 1, blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    expect(a, JSON.stringify(a)).toMatchObject({ ok: true, result: { stored: true } });
    const back = await ev(ca, gid, m.id, { rev: 2, event: "answer.retract", value: { answerId: a.result.answerId } });
    expect(back, JSON.stringify(back)).toMatchObject({ ok: true, result: { stored: true } });
    // A new rev after the answer still closes it.
    await sleep(1100);
    const again = await ev(ca, gid, m.id, { rev: 2, blockId: "pick", event: "choice.submit", value: { value: "no" } });
    expect(again).toMatchObject({ ok: true });
    await cbot.ok({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 2, ops: [{ op: "set", id: "info", props: { text: "好的。" } }] });
    await sleep(1100);
    expect(await ev(ca, gid, m.id, { rev: 3, event: "answer.retract", value: { answerId: again.result.answerId } })).toMatchObject({
      ok: false,
      error: { reason: "card.retract_closed" },
    });
    for (const c of [ca, cbot]) c.close();
  });
});

describe("poll answer cap", () => {
  it("keeps every voter's newest vote when one voter changes a vote many times", async () => {
    const { owner, alice, bob, gid, ca, cb, cbot } = await setup();
    const poll = base({
      intent: "ask",
      title: "周五团建去哪？",
      ask: { from: ["owners"], blocking: false },
      blocks: [
        {
          id: "where",
          type: "poll",
          label: "去哪",
          options: [{ id: "a", label: "爬山" }, { id: "b", label: "火锅" }],
          voters: [alice.id, bob.id, owner.id],
          closesAt: Date.now() + 60_000,
          public: true,
          changeable: true,
          fallbackText: "投票：爬山或火锅",
        },
      ],
    });
    const m = (await send(cbot, gid, poll)).message;
    await ev(ca, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "a" } });
    const b1 = await ev(cb, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    expect(b1).toMatchObject({ ok: true });
    // Bob changed his vote 499 times (the rate limit makes that take minutes; write it directly).
    await runInDurableObject(roomOf(gid), async (_o, state) => {
      const row = state.storage.sql.exec<{ ext: string }>("SELECT ext FROM messages WHERE id = ?", m.id).one();
      const ext = JSON.parse(row.ext);
      const mine = ext.cardState.answers.find((x: { actorId: string }) => x.actorId === bob.id);
      for (let i = 0; i < 498; i++) ext.cardState.answers.push({ ...mine, answerId: `${mine.answerId}${i}`, value: { value: i % 2 ? "b" : "a" } });
      state.storage.sql.exec("UPDATE messages SET ext = ? WHERE id = ?", JSON.stringify(ext), m.id);
    });
    await sleep(1100);
    await ev(cb, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    const u = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState?.answers?.length === 500);
    expect(u.data.cardState.tally).toEqual({ a: 1, b: 1 });
    expect(u.data.cardState.tallyBy).toEqual({ a: [alice.id], b: [bob.id] });
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("refs to a deleted source", () => {
  it("lose the label and value in earlier message.updated events of the referencing message too", async () => {
    const { owner, gid, co, ca, cbot } = await setup();
    const t1 = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "| 姓名 | 工资 |\n| - | - |\n| 张三 | 12000 |" });
    const t2 = await co.ok({ t: "send", groupId: gid, topicId: t1.message.topicId, clientMsgId: clientMsgId(), body: "| 部门 |\n| - |\n| 研发 |" });
    const label = "工资表 · 张三 · 12000";
    const refs = [
      { messageId: t1.message.id, blockId: "md:0", label, value: { cell: 12000 } },
      { messageId: t2.message.id, blockId: "md:0", label: "部门表 · 研发", value: { cell: "研发" } },
    ];
    const q = await ca.ok({ t: "send", groupId: gid, topicId: t1.message.topicId, clientMsgId: clientMsgId(), body: "这两个对得上吗？", blockRefs: refs });
    // The second source goes first (its clear stores blockRefs with the first ref's value), then an edit.
    await co.ok({ t: "delete", groupId: gid, messageId: t2.message.id });
    await ca.ok({ t: "edit", groupId: gid, messageId: q.message.id, body: "对得上吗？" });
    await co.ok({ t: "delete", groupId: gid, messageId: t1.message.id });
    await ca.waitEvent((e) => e.type === "message.updated" && e.data.messageId === q.message.id && e.data.blockRefs?.every((r: { deleted?: boolean }) => r.deleted));
    const page = await ca.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const mine = page.events.filter((e: any) => e.type === "message.updated" && e.data.messageId === q.message.id);
    expect(mine.length).toBeGreaterThanOrEqual(3);
    const text = JSON.stringify(mine);
    expect(text).not.toContain("张三");
    expect(text).not.toContain("12000");
    expect(text).not.toContain("研发");
    for (const e of mine) expect(e.data.body.startsWith("> ↳ 原消息已删除\n> ↳ 原消息已删除")).toBe(true);
    expect(mine.find((e: any) => e.data.body.endsWith("对得上吗？") && !e.data.body.endsWith("这两个对得上吗？"))).toBeDefined();
    void owner;
    for (const c of [co, ca, cbot]) c.close();
  });
});

describe("ask.from requester", () => {
  it("resolves once per ask id and keeps the stored id on later writes of the same round", async () => {
    const { alice, bob, bot, gid, ca, cb, cbot } = await setup();
    const t = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Ledger 发版吗？", mentions: [bot.id] });
    const topicId = t.message.topicId;
    const m = (await send(cbot, gid, quick(["requester"], { ask: { id: "go", from: ["requester"], blocking: false } }), { topicId })).message;
    expect(m.cardState.ask.from).toEqual([alice.id]);
    // Bob now asks the bot in the topic too; the bot changes the card and keeps the ask.
    await cb.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "@Ledger 我也想知道", mentions: [bot.id] });
    const p = await cbot.ok({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 1, ops: [{ op: "set", id: "info", props: { text: "再看看。" } }] });
    expect(p.message.cardState.ask.from).toEqual([alice.id]);
    const e = await cbot.ok({ t: "edit", groupId: gid, messageId: m.id, body: "", baseRev: 2, cardV2: { ...quick(["requester"]), ask: { id: "go", from: ["requester"], blocking: false } } });
    expect(e.message.cardState.ask.from).toEqual([alice.id]);
    expect(await ev(cb, gid, m.id, { rev: 3, blockId: "pick", event: "choice.submit", value: { value: "yes" } })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    // A new round resolves again.
    const r2 = await cbot.ok({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 3, ops: [{ op: "card", props: { ask: { id: "go2", from: ["requester"], blocking: false } } }] });
    expect(r2.message.cardState.ask.from).toEqual([bob.id]);
    for (const c of [ca, cb, cbot]) c.close();
  });
});

/**
 * Holds the requester lookup of one group's card writes until the test lets it go. Plain
 * flags: a promise made in the test cannot be awaited inside the room.
 */
const gate = { groupId: "", held: false, inside: false };
registerRequesterResolver(async (room) => {
  if (room.groupId !== gate.groupId || !gate.held) return undefined;
  gate.inside = true;
  while (gate.held) await new Promise((r) => setTimeout(r, 10));
  return undefined;
});
function hold(gid: string) {
  Object.assign(gate, { groupId: gid, held: true, inside: false });
  return {
    inside: waitUntil(() => gate.inside, 5000, "the write waits on the lookup"),
    open: () => {
      gate.held = false;
    },
  };
}

describe("concurrent card writes", () => {
  /** The card with one more block whose emitters are resolved on write (so the write waits on the lookup). */
  const more = (card: CardV2, id: string): CardV2 => ({
    ...card,
    blocks: [...card.blocks, { id, type: "buttons", buttons: [{ id: "b", label: "看看" }], emitBy: "requester", events: ["button.press"], fallbackText: "看看" }],
  });

  it("an edit or a patch that waits keeps an answer stored meanwhile", async () => {
    const { alice, gid, ca, cbot } = await setup();
    const card = quick([alice.id]);
    // Edit.
    const m = (await send(cbot, gid, card)).message;
    let h = hold(gid);
    const edit = cbot.request({ t: "edit", groupId: gid, messageId: m.id, body: "", baseRev: 1, cardV2: more(card, "x1") });
    await h.inside;
    const a = ev(ca, gid, m.id, { blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    await sleep(100);
    h.open();
    expect(await edit).toMatchObject({ ok: true });
    expect(await a).toMatchObject({ ok: true, result: { stored: true } });
    let page = await ca.request({ t: "history", groupId: gid, topicId: m.topicId });
    let now = page.messages.find((x: any) => x.id === m.id);
    expect(now.cardState.ask.status).toBe("answered");
    expect(now.cardState.answers).toHaveLength(1);
    // Patch.
    const p = (await send(cbot, gid, card)).message;
    h = hold(gid);
    const patch = cbot.request({ t: "card.patch", groupId: gid, messageId: p.id, baseRev: 1, ops: [{ op: "insert", block: more(card, "x2").blocks[2] }] });
    await h.inside;
    await sleep(1100);
    const b = ev(ca, gid, p.id, { blockId: "pick", event: "choice.submit", value: { value: "no" } });
    await sleep(100);
    h.open();
    expect(await patch).toMatchObject({ ok: true });
    expect(await b).toMatchObject({ ok: true, result: { stored: true } });
    page = await ca.request({ t: "history", groupId: gid, topicId: p.topicId });
    now = page.messages.find((x: any) => x.id === p.id);
    expect(now.cardState.ask.status).toBe("answered");
    expect(now.cardState.answers).toHaveLength(1);
    for (const c of [ca, cbot]) c.close();
  });

  it("an edit or a patch of a message deleted while it waits fails and writes nothing", async () => {
    const { alice, gid, co, ca, cbot } = await setup();
    const card = quick([alice.id]);
    for (const kind of ["edit", "patch"]) {
      const m = (await send(cbot, gid, card)).message;
      const h = hold(gid);
      const write =
        kind === "edit"
          ? cbot.request({ t: "edit", groupId: gid, messageId: m.id, body: "", baseRev: 1, cardV2: more(card, "x1") })
          : cbot.request({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 1, ops: [{ op: "insert", block: more(card, "x2").blocks[2] }] });
      await h.inside;
      // An owner deletes it (the author's own writes keep their socket's order).
      const del = co.request({ t: "delete", groupId: gid, messageId: m.id });
      await sleep(100);
      h.open();
      expect((await write).ok, kind).toBe(false);
      expect(await del).toMatchObject({ ok: true });
      const page = await ca.request({ t: "history", groupId: gid, topicId: m.topicId });
      const now = page.messages.find((x: any) => x.id === m.id);
      expect(now.deletedAt).not.toBeNull();
      expect(now.cardV2).toBeUndefined();
      const log = await ca.request({ t: "fetch", groupId: gid, afterSeq: m.createdSeq });
      const mine = log.events.filter((e: any) => e.data?.messageId === m.id);
      expect(mine.map((e: any) => e.type), kind).toEqual(["message.deleted"]);
    }
    for (const c of [co, ca, cbot]) c.close();
  });
});

describe("private answers", () => {
  /** Whether a frame names this member as the actor of something (ask.from may still list them). */
  const names = (x: unknown, id: string) => {
    const t = JSON.stringify(x);
    return t.includes(`"actorId":"${id}"`) || t.includes(`"answeredBy":"${id}"`);
  };
  const privatePoll = (voters: string[]) =>
    base({
      intent: "ask",
      title: "周五团建去哪？",
      ask: { from: ["owners"], blocking: false },
      blocks: [
        {
          id: "where",
          type: "poll",
          label: "去哪",
          options: [{ id: "a", label: "爬山" }, { id: "b", label: "火锅" }],
          voters,
          closesAt: Date.now() + 60_000,
          public: false,
          changeable: true,
          fallbackText: "投票：爬山或火锅",
        },
      ],
    });

  it("a non-public poll gives other members and the author counts only, and the voter their own vote", async () => {
    const { owner, alice, bob, bot, gid, co, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, privatePoll([alice.id, bob.id, owner.id]))).message;
    const r = await ev(ca, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "a" } });
    expect(r).toMatchObject({ ok: true, result: { stored: true } });
    const upd = (c: Client) => c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    const ans = (c: Client) => c.waitEvent((e) => e.type === "card.answered" && e.data.messageId === m.id);
    // Live.
    for (const c of [cb, co, cbot]) {
      const a = await ans(c);
      expect(a.actorId).toBe("anonymous");
      const u = await upd(c);
      expect(u.data.cardState.answers).toEqual([]);
      expect(u.data.cardState.tally).toEqual({ a: 1, b: 0 });
      expect(u.data.cardState.tallyBy).toBeUndefined();
      expect(names([a, u], alice.id)).toBe(false);
    }
    expect((await ans(cb)).data.value).toBeUndefined();
    expect((await ans(cbot)).data.value).toEqual({ value: "a" });
    const mine = await upd(ca);
    expect(mine.data.cardState.answers).toHaveLength(1);
    expect(mine.data.cardState.answers[0]).toMatchObject({ actorId: alice.id, value: { value: "a" } });
    expect((await ans(ca)).actorId).toBe(alice.id);
    // Pages and history.
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: m.createdSeq });
    expect(names(page.events, alice.id)).toBe(false);
    const hist = await cb.request({ t: "history", groupId: gid, topicId: m.topicId });
    const seen = hist.messages.find((x: any) => x.id === m.id);
    expect(seen.cardState.answers).toEqual([]);
    expect(names(seen, alice.id)).toBe(false);
    // Everyone voted: the v1 status names nobody.
    await ev(cb, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    await ev(co, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    const done = await ca.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState.ask.status === "answered");
    expect(done.data.cardState.ask.answeredBy).toBeUndefined();
    expect(done.data.card.status.label).toBe("已回答");
    void bot;
    for (const c of [co, ca, cb, cbot]) c.close();
  });

  it("an anonymous questionnaire gives the author the answers without voters and other members nothing", async () => {
    const { alice, bob, gid, co, ca, cb, cbot } = await setup();
    const card = base({
      intent: "ask",
      title: "实习反馈",
      ask: { from: [alice.id, bob.id], blocking: false },
      blocks: [
        { id: "q", type: "questionnaire", children: ["mood", "next"], anonymous: true, fallbackText: "实习反馈" },
        { id: "mood", type: "input", kind: "text", label: "感受" },
        { id: "next", type: "input", kind: "text", label: "建议" },
      ],
    });
    const m = (await send(cbot, gid, card)).message;
    const r = await ev(ca, gid, m.id, { blockId: "q", event: "action.press", inputs: { mood: { value: "还行" } } });
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true, result: { stored: true } });
    const au = await cbot.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    expect(au.data.cardState.answers).toHaveLength(1);
    expect(au.data.cardState.answers[0]).toMatchObject({ actorId: "anonymous", inputs: { mood: { value: "还行" } } });
    const aa = await cbot.waitEvent((e) => e.type === "card.answered" && e.data.messageId === m.id);
    expect(aa).toMatchObject({ actorId: "anonymous", data: { inputs: { mood: { value: "还行" } } } });
    for (const c of [cb, co]) {
      const u = await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
      const a = await c.waitEvent((e) => e.type === "card.answered" && e.data.messageId === m.id);
      expect(u.data.cardState.answers).toEqual([]);
      expect(a.data.inputs).toBeUndefined();
      expect(names([u, a], alice.id)).toBe(false);
      expect(JSON.stringify([u, a])).not.toContain("还行");
    }
    const page = await cbot.request({ t: "fetch", groupId: gid, afterSeq: m.createdSeq });
    expect(names(page.events, alice.id)).toBe(false);
    for (const c of [co, ca, cb, cbot]) c.close();
  });
});

describe("retract window of a runner's card", () => {
  it("closes when the answer starts a run or a follow-up run, and a retract inside it reaches the run that has the answer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["interact", "output"] });
    const { c } = await Client.connect(w.human.token, { features: FEATURES });
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    const post = async (run: string, outputId: string, title: string) => {
      d.event(run, { type: "output", outputId, op: "card.post", body: { card: quick([w.human.id], { title, ask: { from: [w.human.id], blocking: false } }) } });
      const posted = await d.next((f) => f.t === "run.output.result" && f.outputId === outputId);
      expect(posted, JSON.stringify(posted)).toMatchObject({ ok: true });
      return posted.result.messageId as string;
    };
    const answer = (messageId: string, value: string) =>
      c.ok({ t: "card.event", groupId: w.gid, messageId, rev: 1, clientEventId: crypto.randomUUID(), blockId: "pick", event: "choice.submit", value: { value } });
    const retract = (messageId: string, answerId: string) => c.request({ t: "card.answer.retract", groupId: w.gid, messageId, answerId });
    const closed = { ok: false, error: { reason: "card.retract_closed" } };

    // The run has the answer but has not taken it: a retract reaches it.
    const m1 = await post(runId, "p1", "第一张");
    const a1 = await answer(m1, "yes");
    expect((await d.next((f) => f.t === "run.answer")).source.answerId).toBe(a1.answerId);
    expect(await retract(m1, a1.answerId)).toMatchObject({ ok: true });
    const rx = await d.next((f) => f.t === "run.interaction" && f.event === "answer.retract");
    expect(rx).toMatchObject({ runId, source: { kind: "card", messageId: m1 }, value: { answerId: a1.answerId } });

    // The run ends without taking the next answer: the follow-up run acts on it.
    await sleep(1100);
    const a2 = await answer(m1, "no");
    await d.next((f) => f.t === "run.answer" && f.source.answerId === a2.answerId);
    d.done(runId, { status: "succeeded", resultText: "好" });
    const follow = await d.next((f) => f.t === "run.assign" && f.runId !== runId);
    expect(await retract(m1, a2.answerId)).toMatchObject(closed);

    // No run: the answer starts one.
    d.send({ t: "run.accept", runId: follow.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === follow.runId && e.data.run.status === "running");
    const m2 = await post(follow.runId, "p2", "第二张");
    d.done(follow.runId, { status: "succeeded", resultText: "好" });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === follow.runId && e.data.run.status === "done");
    const desk = (env as unknown as BotsEnv).BOT_DESK.get((env as unknown as BotsEnv).BOT_DESK.idFromName(w.bot.id));
    for (let i = 0; ; i++) {
      const open = await runInDurableObject(desk, async (_o, state) => state.storage.sql.exec("SELECT 1 FROM runs WHERE ended_at IS NULL").toArray().length);
      if (open === 0) break;
      if (i > 200) throw new Error("the run did not end");
      await sleep(20);
    }
    const a3 = await answer(m2, "yes");
    await d.next((f) => f.t === "run.assign" && f.runId !== runId && f.runId !== follow.runId);
    expect(await retract(m2, a3.answerId)).toMatchObject(closed);
    void topicId;
    for (const x of [c, d]) x.close();
  });
});
