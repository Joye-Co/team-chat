// Review fixes of the 1.1 interaction loop in the group room: audit redaction, private
// polls and anonymous questionnaires, concurrent card writes, refs, edit marks, requester
// asks, the retract window and the topic status of blocking asks.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import { Client, clientMsgId, createGroup, createMember, protocolViolations, type TestMember } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
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
