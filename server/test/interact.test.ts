// The 1.1 interaction loop in the group room (V11-INTERACT §8.4 server items 1-7) and refs to
// parts (V11-REFS §C3 server items): live events, answers and card state, idempotency,
// patches, blocking asks, 1.0.x readers, send/edit/delete with refs, diagram blocks.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, type TestMember } from "./helpers";

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

/** INTERACT §7.1 rev 1, shortened: a row with a chart and a question, and a non-blocking ask. */
function report(from: string[], extra: Partial<CardV2> = {}): CardV2 {
  return {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "done",
    title: "10 月数据月报",
    fallback: "Ledger 发了 10 月数据月报：新增群 38。",
    ask: { id: "verify", from, blocking: false },
    blocks: [
      { id: "main", type: "row", weights: [2, 1], children: ["trend", "check"] },
      {
        id: "trend",
        type: "chart",
        kind: "bar",
        label: "新增群",
        x: ["10-06", "10-13"],
        series: [{ id: "oct", name: "10 月", values: [9, 12] }],
        summary: "10 月每周都在涨。",
        events: ["chart.select"],
      },
      {
        id: "check",
        type: "decision",
        label: "这个数据对不对？",
        required: true,
        options: [
          { id: "ok", label: "对" },
          { id: "wrong", label: "有问题" },
        ],
        events: ["choice.change"],
      },
      { id: "m", type: "metric", label: "日活", value: 2340, delta: 90 },
      { id: "btns", type: "buttons", buttons: [{ id: "split", label: "按渠道拆开" }], emitBy: "owners", events: ["button.press"], fallbackText: "按渠道拆开" },
    ],
    actions: [{ id: "send", label: "发给 Ledger", style: "primary", inputs: ["check"] }],
    ...extra,
  };
}

/** A blocking yes/no ask with an immediate choice (v1 buttons can answer it). */
function quick(from: string[], extra: Partial<CardV2> = {}): CardV2 {
  return {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "ask",
    title: "今天发版吗？",
    fallback: "今天发版吗？",
    ask: { from },
    blocks: [
      { id: "pick", type: "decision", deliver: "immediate", options: [{ id: "yes", label: "发" }, { id: "no", label: "不发" }] },
      { id: "info", type: "text", text: "改动都测过了。" },
    ],
    ...extra,
  };
}

const send = (c: Client, gid: string, cardV2: CardV2, extra: Record<string, unknown> = {}) =>
  c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2, ...extra });
const ev = (c: Client, gid: string, messageId: string, f: Record<string, unknown>) =>
  c.request({ t: "card.event", groupId: gid, messageId, rev: 1, clientEventId: crypto.randomUUID(), ...f });

function roomOf(gid: string) {
  return env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid));
}

describe("live card events", () => {
  it("go to the author only as a transient, are audited and not stored, and ack with the author's presence", async () => {
    const { owner, alice, bot, gid, co, ca, cb, cbot, connect } = await setup();
    const m = (await send(cbot, gid, report([alice.id]))).message;
    expect(m.cardV2.rev).toBe(1);
    const r = await ev(ca, gid, m.id, { blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-13", y: 12, index: 1 }, part: { kind: "bar", keys: { series: "oct", x: "10-13" } } });
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true, result: { stored: false, expect: { by: bot.id, mode: "live" } } });
    const tr = await cbot.next((f) => f.t === "transient" && f.event.type === "card.event");
    expect(tr.event).toMatchObject({
      groupId: gid,
      messageId: m.id,
      eventId: r.result.eventId,
      rev: 1,
      blockId: "trend",
      event: "chart.select",
      part: { kind: "bar", keys: { series: "oct", x: "10-13" } },
      actorId: alice.id,
    });
    await new Promise((res) => setTimeout(res, 200));
    for (const c of [co, ca, cb]) expect(c.frames.some((f) => f.t === "transient" && f.event.type === "card.event")).toBe(false);
    expect(ca.events(gid).some((e) => e.seq > m.createdSeq && e.messageId === m.id)).toBe(false);
    const page = await co.request({ t: "fetch", groupId: gid, afterSeq: m.createdSeq });
    expect(page.events.filter((e: any) => e.type !== "topic.updated")).toEqual([]);
    const audit = await env.DB.prepare("SELECT actor_id, target, details FROM audit WHERE action = 'card.event' AND target = ?").bind(m.id).first<{ actor_id: string; details: string }>();
    expect(audit?.actor_id).toBe(alice.id);
    expect(JSON.parse(audit?.details ?? "{}")).toMatchObject({ groupId: gid, eventId: r.result.eventId, event: "chart.select", blockId: "trend" });

    // A secret-looking value is redacted in the audit log.
    const secret = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "other", other: "x" } });
    expect(secret).toMatchObject({ ok: false, error: { code: "invalid" } }); // no allowOther
    // The author offline: the event is not delivered anywhere and the ack says so.
    cbot.close();
    await new Promise((res) => setTimeout(res, 200));
    const off = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "ok" } });
    expect(off).toMatchObject({ ok: true, result: { stored: false, expect: { by: bot.id, mode: "offline" } } });
    // An SDK member connected without interact/1 (an old SDK) counts as offline.
    const old = await connect(bot, []);
    const r2 = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "wrong" } });
    expect(r2.result.expect.mode).toBe("offline");
    void owner;
    for (const c of [co, ca, cb, old]) c.close();
  });

  it("refuse undeclared events, blocked and bot emitters, removed blocks and newer revs", async () => {
    const { owner, alice, bob, bot, gid, co, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, report([alice.id]))).message;
    // Undeclared: the metric declares no events; the chart does not send chart.clear.
    expect(await ev(ca, gid, m.id, { blockId: "m", event: "metric.press", value: {} })).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.undeclared_event" } });
    expect(await ev(ca, gid, m.id, { blockId: "trend", event: "chart.clear", value: {} })).toMatchObject({ ok: false, error: { reason: "card.undeclared_event" } });
    // emitBy owners: Bob may not press, the owner may.
    expect(await ev(cb, gid, m.id, { blockId: "btns", event: "button.press", value: { buttonId: "split" } })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await ev(co, gid, m.id, { blockId: "btns", event: "button.press", value: { buttonId: "split" } })).toMatchObject({ ok: true });
    // The decision surface's live events default to ask.from: only Alice.
    expect(await ev(cb, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "ok" } })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    // A bot never emits.
    expect(await ev(cbot, gid, m.id, { blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-06", y: 9, index: 0 } })).toMatchObject({
      ok: false,
      error: { code: "forbidden", reason: "interact.bot_emitter" },
    });
    // A rev the server has not made yet.
    expect(await ev(ca, gid, m.id, { rev: 2, blockId: "trend", event: "chart.select", value: { series: "oct", index: 0 } })).toMatchObject({ ok: false, error: { code: "invalid" } });
    // The author removes the buttons; an event on them (seen at rev 1) is gone.
    await cbot.ok({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 1, ops: [{ op: "remove", id: "btns" }] });
    expect(await ev(co, gid, m.id, { blockId: "btns", event: "button.press", value: { buttonId: "split" } })).toMatchObject({
      ok: false,
      error: { code: "not_found", reason: "card.block_gone" },
    });
    // A live event on an older rev is accepted while the block exists.
    expect(await ev(ca, gid, m.id, { rev: 1, blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-06", y: 9, index: 0 } })).toMatchObject({ ok: true });
    void owner;
    void bob;
    void bot;
    for (const c of [co, ca, cb, cbot]) c.close();
  });
});

describe("answers and card state", () => {
  it("store card.answered and message.updated with the state and the v1 card, once, and check expiry and the decision surface", async () => {
    const { alice, bob, bot, gid, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, report([alice.id]))).message;
    expect(m.cardState).toMatchObject({ ask: { id: "verify", status: "open", from: [alice.id], blocking: false }, answers: [] });
    expect(m.card.status.label).toBe("等 Alice 拍板");
    // Required input missing, then a value that names no option.
    expect(await ev(ca, gid, m.id, { actionId: "send", event: "action.press", inputs: {} })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await ev(ca, gid, m.id, { actionId: "send", event: "action.press", inputs: { check: { value: "maybe" } } })).toMatchObject({ ok: false, error: { code: "invalid" } });
    // Bob is not asked.
    expect(await ev(cb, gid, m.id, { actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    const r = await ev(ca, gid, m.id, { actionId: "send", event: "action.press", inputs: { check: { value: "wrong" } } });
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true, result: { stored: true, expect: { by: bot.id, mode: "live" } } });
    const answered = await cb.waitEvent((e) => e.type === "card.answered" && e.data.messageId === m.id);
    expect(answered).toMatchObject({
      actorId: alice.id,
      data: { messageId: m.id, rev: 1, actionId: "send", inputs: { check: { value: "wrong" } }, answerId: r.result.answerId, askId: "verify", eventId: r.result.eventId, event: "action.press" },
    });
    const upd = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    expect(upd.data.cardState.ask).toMatchObject({ status: "answered", answeredBy: alice.id });
    expect(upd.data.cardState.answers).toHaveLength(1);
    expect(upd.data.card.status.label).toBe("Alice 选了 有问题");
    expect(upd.data.cardV2).toBeUndefined(); // the card did not change
    // Second answer.
    await new Promise((res) => setTimeout(res, 1100));
    expect(await ev(ca, gid, m.id, { actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } })).toMatchObject({
      ok: false,
      error: { code: "conflict", reason: "card.already_answered" },
    });

    // Expired.
    const late = (await send(cbot, gid, report([alice.id], { ask: { id: "verify", from: [alice.id], expiresAt: Date.now() + 300 } }))).message;
    await new Promise((res) => setTimeout(res, 400));
    expect(await ev(ca, gid, late.id, { actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } })).toMatchObject({
      ok: false,
      error: { code: "conflict", reason: "card.expired" },
    });
    // The alarm stores the expiry.
    const exp = await ca.waitEvent((e) => e.type === "message.updated" && e.data.messageId === late.id && e.data.cardState?.ask?.status === "expired", 5000);
    expect(exp.data.card.status.label).toBe("已过期");

    // The decision surface changed since the rev the person saw: superseded. Another block changed: accepted.
    const s1 = (await send(cbot, gid, report([alice.id]))).message;
    await cbot.ok({ t: "card.patch", groupId: gid, messageId: s1.id, baseRev: 1, ops: [{ op: "set", id: "check", props: { label: "数据对吗？" } }] });
    expect(await ev(ca, gid, s1.id, { rev: 1, actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } })).toMatchObject({
      ok: false,
      error: { code: "conflict", reason: "card.superseded" },
    });
    const s2 = (await send(cbot, gid, report([alice.id]))).message;
    await cbot.ok({ t: "card.patch", groupId: gid, messageId: s2.id, baseRev: 1, ops: [{ op: "set", id: "m", props: { value: 2400 } }] });
    expect(await ev(ca, gid, s2.id, { rev: 1, actionId: "send", event: "action.press", inputs: { check: { value: "ok" } } })).toMatchObject({ ok: true, result: { stored: true } });
    void bob;
    for (const c of [ca, cb, cbot]) c.close();
  });

  it("take an answer back inside the window and refuse it after the author acted", async () => {
    const { alice, gid, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, quick([alice.id]))).message;
    const a = await ev(ca, gid, m.id, { blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    expect(a).toMatchObject({ ok: true, result: { stored: true } });
    // Short form of the retract.
    const back = await ca.request({ t: "card.answer.retract", groupId: gid, messageId: m.id, answerId: a.result.answerId });
    expect(back, JSON.stringify(back)).toMatchObject({ ok: true, result: { stored: true, answerId: a.result.answerId } });
    const rt = await cb.waitEvent((e) => e.type === "card.retracted");
    expect(rt).toMatchObject({ actorId: alice.id, data: { messageId: m.id, answerId: a.result.answerId, askId: "a1" } });
    const reopened = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState?.ask?.status === "open");
    expect(reopened.data.cardState.answers[0].retractedAt).toBeGreaterThan(0);
    // Answer again; the author replies in the topic; the window is closed.
    await new Promise((res) => setTimeout(res, 1100));
    const again = await ev(ca, gid, m.id, { blockId: "pick", event: "choice.submit", value: { value: "no" } });
    expect(again).toMatchObject({ ok: true });
    await cbot.ok({ t: "send", groupId: gid, topicId: m.topicId, clientMsgId: clientMsgId(), body: "好的，不发了" });
    expect(await ca.request({ t: "card.answer.retract", groupId: gid, messageId: m.id, answerId: again.result.answerId })).toMatchObject({
      ok: false,
      error: { code: "conflict", reason: "card.retract_closed" },
    });
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("idempotency", () => {
  it("returns the first ack for a repeated clientEventId or clientPatchId and stores nothing twice", async () => {
    const { alice, gid, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, quick([alice.id]))).message;
    const f = { blockId: "pick", event: "choice.submit", value: { value: "yes" }, clientEventId: "01JC0CLIENTEVENT0000000001" };
    const first = await ev(ca, gid, m.id, f);
    const second = await ev(ca, gid, m.id, f);
    expect(second).toMatchObject({ ok: true, result: { ...first.result, duplicate: true } });
    await new Promise((res) => setTimeout(res, 200));
    expect(cb.events(gid).filter((e) => e.type === "card.answered")).toHaveLength(1);
    // Live events too: one transient.
    const live = (await send(cbot, gid, report([alice.id]))).message;
    const g = { blockId: "check", event: "choice.change", value: { value: "ok" }, clientEventId: "01JC0CLIENTEVENT0000000002" };
    const l1 = await ev(ca, gid, live.id, g);
    const l2 = await ev(ca, gid, live.id, g);
    expect(l2.result).toMatchObject({ eventId: l1.result.eventId, duplicate: true });
    await new Promise((res) => setTimeout(res, 200));
    expect(cbot.frames.filter((x) => x.t === "transient" && x.event.type === "card.event" && x.event.messageId === live.id)).toHaveLength(1);
    // Patches.
    const p = { t: "card.patch", groupId: gid, messageId: live.id, baseRev: 1, ops: [{ op: "set", id: "m", props: { value: 1 } }], clientPatchId: "p-1" };
    const p1 = await cbot.ok(p);
    const p2 = await cbot.ok(p);
    expect(p1.rev).toBe(2);
    expect(p2).toMatchObject({ rev: 2, duplicate: true });
    await new Promise((res) => setTimeout(res, 200));
    expect(cb.events(gid).filter((e) => e.type === "message.updated" && e.data.messageId === live.id && e.data.cardV2)).toHaveLength(1);
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("card.patch", () => {
  it("applies every op, checks the whole result, the base rev, the cause and the limits, and resolves the cause", async () => {
    const { alice, bob, gid, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, report([alice.id], { data: { weeks: ["10-06"] } }))).message;
    const ev1 = await ev(ca, gid, m.id, { blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-13", y: 12, index: 1 } });
    const patch = (f: Record<string, unknown>) => cbot.request({ t: "card.patch", groupId: gid, messageId: m.id, ...f });
    // Only the author patches.
    expect(await cb.request({ t: "card.patch", groupId: gid, messageId: m.id, baseRev: 1, ops: [{ op: "remove", id: "m" }] })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    const ok = await patch({
      baseRev: 1,
      cause: ev1.result.eventId,
      ops: [
        { op: "set", id: "trend", props: { label: "新增群 · 按周" } },
        { op: "replace", id: "m", block: { id: "m", type: "text", text: "日活 2,340" } },
        { op: "insert", parent: "main", after: "trend", block: { id: "why", type: "text", text: "10-13 有一次活动" } },
        { op: "set", id: "main", props: { weights: [2, 1, 1] } },
        { op: "insert", before: "m", block: { id: "head", type: "text", text: "这周" } },
        { op: "remove", id: "btns" },
        { op: "data", path: "/weeks/1", value: "10-13" },
        { op: "card", props: { title: "10 月数据月报（拆开）", ask: null } },
      ],
    });
    // A row holds at most 3 children: trend, why, check.
    expect(ok, JSON.stringify(ok)).toMatchObject({ ok: true, result: { rev: 2 } });
    const card = ok.result.message.cardV2;
    expect(card.title).toBe("10 月数据月报（拆开）");
    expect(card.ask).toBeUndefined();
    expect(card.blocks.find((b: any) => b.id === "main").children).toEqual(["trend", "why", "check"]);
    expect(card.blocks.find((b: any) => b.id === "trend").label).toBe("新增群 · 按周");
    expect(card.blocks.find((b: any) => b.id === "m")).toEqual({ id: "m", type: "text", text: "日活 2,340" });
    expect(card.blocks.some((b: any) => b.id === "btns")).toBe(false);
    expect(card.data).toEqual({ weeks: ["10-06", "10-13"] });
    expect(card.blocks.map((b: any) => b.id).indexOf("head")).toBe(card.blocks.map((b: any) => b.id).indexOf("m") - 1);
    expect(ok.result.message.cause).toMatchObject({ eventId: ev1.result.eventId, actorId: alice.id, event: "chart.select", blockId: "trend" });
    expect(ok.result.message.cardState.ask.status).toBe("closed");
    const upd = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2?.rev === 2);
    expect(upd.data).toMatchObject({ body: card.fallback, streaming: false, cause: { eventId: ev1.result.eventId } });
    expect(upd.data.card.title).toBe("10 月数据月报（拆开）");
    // The emitter learns that the event was answered.
    const res = await ca.next((f) => f.t === "transient" && f.event.type === "interaction.resolved");
    expect(res.event).toMatchObject({ groupId: gid, eventId: ev1.result.eventId, result: "patched", messageId: m.id });
    expect(cb.frames.some((f) => f.t === "transient" && f.event.type === "interaction.resolved")).toBe(false);

    // Stale base rev.
    expect(await patch({ baseRev: 1, ops: [{ op: "remove", id: "why" }] })).toMatchObject({ ok: false, error: { code: "conflict", reason: "card.stale_rev" } });
    // The result is checked as a whole; nothing is applied on failure.
    await new Promise((res) => setTimeout(res, 1100));
    const four = await patch({ baseRev: 2, ops: [{ op: "insert", parent: "main", block: { id: "x4", type: "text", text: "4" } }] });
    expect(four).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    const style = await patch({ baseRev: 2, ops: [{ op: "set", id: "why", props: { color: "red" } }] });
    expect(style).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.style_property" } });
    const deep = await patch({
      baseRev: 2,
      ops: [
        { op: "insert", block: { id: "g1", type: "group", title: "一", children: ["g2"] } },
        { op: "insert", block: { id: "g2", type: "group", title: "二", children: ["g3"] } },
        { op: "insert", block: { id: "g3", type: "group", title: "三", children: ["g4"] } },
        { op: "insert", block: { id: "g4", type: "stack", children: ["leaf"] } },
        { op: "insert", block: { id: "leaf", type: "text", text: "深" } },
      ],
    });
    expect(deep).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.depth" } });
    expect(await patch({ baseRev: 2, ops: [{ op: "remove", id: "nope" }] })).toMatchObject({ ok: false, error: { code: "invalid" } });
    const now = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const msgs = now.events.filter((e: any) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2);
    expect(msgs).toHaveLength(1);
    // A cause from another card is refused.
    const other = (await send(cbot, gid, report([alice.id]))).message;
    const ev2 = await ev(ca, gid, other.id, { blockId: "trend", event: "chart.select", value: { series: "oct", index: 0 } });
    expect(await patch({ baseRev: 2, cause: ev2.result.eventId, ops: [{ op: "remove", id: "why" }] })).toMatchObject({
      ok: false,
      error: { code: "invalid", reason: "interact.bad_cause" },
    });
    // Rate: bursts of 3, then 1 per second.
    let rev = 2;
    const codes: string[] = [];
    for (let i = 0; i < 4; i++) {
      const r = await patch({ baseRev: rev, ops: [{ op: "set", id: "why", props: { text: `第 ${i} 次` } }] });
      if (r.ok) rev = r.result.rev;
      codes.push(r.ok ? "ok" : r.error.code);
    }
    expect(codes.slice(-1)).toEqual(["rate_limited"]);
    // 300 revs per card.
    await runInDurableObject(roomOf(gid), async (_o, state) => {
      const row = state.storage.sql.exec<{ ext: string }>("SELECT ext FROM messages WHERE id = ?", other.id).one();
      const ext = JSON.parse(row.ext);
      ext.cardV2.rev = 300;
      state.storage.sql.exec("UPDATE messages SET ext = ? WHERE id = ?", JSON.stringify(ext), other.id);
    });
    expect(await cbot.request({ t: "card.patch", groupId: gid, messageId: other.id, baseRev: 300, ops: [{ op: "remove", id: "m" }] })).toMatchObject({
      ok: false,
      error: { code: "too_large", reason: "card.rev_limit" },
    });
    void bob;
    for (const c of [ca, cb, cbot]) c.close();
  });

  it("resolve an event once, from the author only, and resolve a cause on send and edit", async () => {
    const { alice, gid, ca, cb, cbot } = await setup();
    const m = (await send(cbot, gid, report([alice.id]))).message;
    const e1 = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "ok" } });
    expect(await cb.request({ t: "interaction.resolve", groupId: gid, eventId: e1.result.eventId, result: "ignored" })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await cbot.request({ t: "interaction.resolve", groupId: gid, eventId: e1.result.eventId, result: "failed", note: "没有 10-14 之前的数" })).toMatchObject({ ok: true });
    const r = await ca.next((f) => f.t === "transient" && f.event.type === "interaction.resolved");
    expect(r.event).toMatchObject({ eventId: e1.result.eventId, result: "failed", note: "没有 10-14 之前的数", messageId: m.id });
    expect(await cbot.request({ t: "interaction.resolve", groupId: gid, eventId: e1.result.eventId, result: "ignored" })).toMatchObject({ ok: false, error: { code: "conflict" } });
    // A new message with causeEventId.
    const e2 = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "wrong" } });
    const posted = await cbot.ok({ t: "send", groupId: gid, topicId: m.topicId, clientMsgId: clientMsgId(), body: "哪里不对？", causeEventId: e2.result.eventId });
    expect(posted.message.cause).toMatchObject({ eventId: e2.result.eventId, actorId: alice.id, event: "choice.change" });
    const pr = await ca.next((f) => f.t === "transient" && f.event.type === "interaction.resolved" && f.event.eventId === e2.result.eventId);
    expect(pr.event).toMatchObject({ result: "posted", messageId: posted.message.id });
    // Someone else cannot name another author's event as a cause.
    const e3 = await ev(ca, gid, m.id, { blockId: "check", event: "choice.change", value: { value: "ok" } });
    expect(await cb.request({ t: "send", groupId: gid, topicId: m.topicId, clientMsgId: clientMsgId(), body: "x", causeEventId: e3.result.eventId })).toMatchObject({
      ok: false,
      error: { reason: "interact.bad_cause" },
    });
    // An edit with cardV2, baseRev and causeEventId.
    const next = { ...report([alice.id]), title: "改过" };
    expect(await cbot.request({ t: "edit", groupId: gid, messageId: m.id, body: "", cardV2: next, baseRev: 3 })).toMatchObject({ ok: false, error: { code: "conflict", reason: "card.stale_rev" } });
    const edited = await cbot.ok({ t: "edit", groupId: gid, messageId: m.id, body: "", cardV2: next, baseRev: 1, causeEventId: e3.result.eventId });
    expect(edited.message.cardV2).toMatchObject({ rev: 2, title: "改过" });
    expect(edited.message.cause.eventId).toBe(e3.result.eventId);
    const er = await ca.next((f) => f.t === "transient" && f.event.type === "interaction.resolved" && f.event.eventId === e3.result.eventId);
    expect(er.event.result).toBe("patched");
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("blocking asks and rounds", () => {
  it("a blocking ask sets the topic waiting on its people; a non-blocking one does not; a new round supersedes the old one", async () => {
    const { owner, alice, gid, co, ca, cbot } = await setup();
    const root = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Ledger 发版", mentions: [] });
    const topicId = root.topic.id;
    const nb = (await send(cbot, gid, report([alice.id]), { topicId })).message;
    await new Promise((res) => setTimeout(res, 200));
    expect(co.events(gid).some((e) => e.type === "topic.updated" && e.data.status === "waiting_answer")).toBe(false);
    void nb;
    // A blocking ask from "requester": the topic's creator here (nobody mentioned the bot).
    const b = (await send(cbot, gid, quick(["requester"]), { topicId })).message;
    expect(b.cardState.ask).toMatchObject({ status: "open", from: [owner.id], blocking: true });
    const waiting = await co.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "waiting_answer");
    expect(waiting.data.waitingOn).toEqual([owner.id]);
    // A new round (another ask id) closes the open one as superseded and stays blocking.
    const round = await cbot.ok({
      t: "card.patch",
      groupId: gid,
      messageId: b.id,
      baseRev: 1,
      ops: [{ op: "card", props: { ask: { id: "a2", from: [alice.id] }, title: "那明天发吗？" } }],
    });
    expect(round.message.cardState.ask).toMatchObject({ id: "a2", status: "open", from: [alice.id] });
    const moved = await co.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.waitingOn?.[0] === alice.id);
    expect(moved.data.status).toBe("waiting_answer");
    // An answer from the old round's person is refused; Alice answers; the topic leaves waiting_answer.
    expect(await ev(co, gid, b.id, { rev: 2, blockId: "pick", event: "choice.submit", value: { value: "yes" } })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await ev(ca, gid, b.id, { rev: 2, blockId: "pick", event: "choice.submit", value: { value: "yes" } })).toMatchObject({ ok: true, result: { stored: true } });
    const done = await co.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "in_progress");
    expect(done.data.waitingOn).toEqual([]);
    for (const c of [co, ca, cbot]) c.close();
  });
});

describe("1.0.x readers", () => {
  it("get no 1.1 transients, a v1 card on every rev, and press derived v1 buttons as action.press", async () => {
    const { alice, bob, gid, cbot, connect } = await setup();
    const oldAlice = await connect(alice, []);
    const oldBot = await connect(bob, []);
    const m = (await send(cbot, gid, quick([alice.id]))).message;
    // The v1 card: pick buttons for the immediate choice, the status line, the marked fallback.
    expect(m.card.actions.map((a: any) => a.label)).toEqual(["选 发", "选 不发"]);
    expect(m.card.status.label).toBe("等 Alice 拍板");
    expect(m.card.blocks[0].text).toBe("这部分需要新版本：今天发版吗？");
    // A v1 press answers the block with that option.
    expect(await oldAlice.request({ t: "card.action", groupId: gid, messageId: m.id, actionId: m.card.actions[1].id })).toMatchObject({ ok: true });
    const answered = await oldAlice.waitEvent((e) => e.type === "card.answered" && e.data.messageId === m.id);
    expect(answered.data).toMatchObject({ blockId: "pick", event: "choice.submit", value: { value: "no" } });
    const upd = await oldAlice.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    expect(upd.data.card.status.label).toBe("Alice 选了 不发");
    expect(upd.data.card.actions).toBeUndefined();
    // Live events and resolves never reach a 1.0.x socket.
    const r = (await send(cbot, gid, report([bob.id]))).message;
    expect(r.card.blocks).toContainEqual({ type: "fields", fields: [{ label: "日活", value: "2340 (+90)" }] });
    expect(r.card.blocks).toContainEqual({ type: "text", text: "**新增群**：10 月每周都在涨。" });
    const live = await ev(oldBot, gid, r.id, { blockId: "check", event: "choice.change", value: { value: "ok" } });
    expect(live).toMatchObject({ ok: true });
    await cbot.ok({ t: "interaction.resolve", groupId: gid, eventId: live.result.eventId, result: "ignored" });
    const p = await cbot.ok({ t: "card.patch", groupId: gid, messageId: r.id, baseRev: 1, ops: [{ op: "set", id: "m", props: { value: 3 } }] });
    const u2 = await oldBot.waitEvent((e) => e.type === "message.updated" && e.data.messageId === r.id && e.data.cardV2?.rev === 2);
    expect(u2.data.card).toEqual(p.message.card);
    await new Promise((res) => setTimeout(res, 200));
    for (const c of [oldAlice, oldBot]) expect(c.frames.some((f) => f.t === "transient" && ["card.event", "interaction.resolved"].includes(f.event.type))).toBe(false);
    for (const c of [oldAlice, oldBot, cbot]) c.close();
  });
});

describe("group decisions of 2.1 asks", () => {
  it("record an immediate pick with the option as its label", async () => {
    const { owner, alice, gid, ca, cbot } = await setup();
    const m = (await send(cbot, gid, quick([alice.id], { ask: { from: [alice.id], record: true } }))).message;
    await ev(ca, gid, m.id, { blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    const r = await api(`/v1/groups/${gid}/decisions`, { token: owner.token });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body.decisions[0]).toMatchObject({ messageId: m.id, actionId: "pick", actionLabel: "发", choices: ["发"], answeredBy: alice.id });
    for (const c of [ca, cbot]) c.close();
  });
});

describe("polls", () => {
  it("take one vote per voter, tally them, and replace a vote when changeable", async () => {
    const { alice, bob, gid, ca, cb, cbot } = await setup();
    const poll: CardV2 = {
      v: 2,
      catalog: "tc.card/2.1",
      rev: 1,
      intent: "ask",
      title: "周五团建去哪？",
      fallback: "周五团建去哪？",
      ask: { from: ["owners"], blocking: false },
      blocks: [
        {
          id: "where",
          type: "poll",
          label: "去哪",
          options: [{ id: "a", label: "爬山" }, { id: "b", label: "火锅" }],
          voters: [alice.id, bob.id],
          closesAt: Date.now() + 60_000,
          public: true,
          changeable: true,
          fallbackText: "投票：爬山或火锅",
        },
      ],
    };
    const m = (await send(cbot, gid, poll)).message;
    expect(m.cardState.ask.from.sort()).toEqual([alice.id, bob.id].sort());
    expect(m.cardState.tally).toEqual({ a: 0, b: 0 });
    await ev(ca, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "a" } });
    const u1 = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    expect(u1.data.cardState).toMatchObject({ tally: { a: 1, b: 0 }, tallyBy: { a: [alice.id] }, ask: { status: "open" } });
    await new Promise((res) => setTimeout(res, 1100));
    await ev(ca, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    await ev(cb, gid, m.id, { blockId: "where", event: "choice.submit", value: { value: "b" } });
    const u3 = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState?.ask?.status === "answered");
    expect(u3.data.cardState.tally).toEqual({ a: 0, b: 2 });
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("refs to parts", () => {
  it("store refs with their sources, write the quote lines, keep them on edit and clear them when the source is deleted", async () => {
    const { owner, alice, bot, gid, co, ca, cbot } = await setup();
    const card = (await send(cbot, gid, report([alice.id]))).message;
    const ref = { messageId: card.id, cardRev: 1, blockId: "trend", part: { kind: "bar", keys: { series: "oct", x: "10-13" } }, label: "10 月数据月报 · 新增群 · 10 月 · 10-13 · 12", value: { series: "oct", x: "10-13", value: 12 } };
    const sent = await ca.ok({ t: "send", groupId: gid, topicId: card.topicId, clientMsgId: clientMsgId(), body: "为什么比上月少了 1 个？", blockRefs: [ref] });
    expect(sent.message.body).toBe("> ↳ 10 月数据月报 · 新增群 · 10 月 · 10-13 · 12\n\n为什么比上月少了 1 个？");
    expect(sent.message.blockRefs).toEqual([{ ...ref, sourceAuthorId: bot.id, sourceTopicId: card.topicId }]);
    // An empty sender body: the quote line only.
    const bare = await ca.ok({ t: "send", groupId: gid, topicId: card.topicId, clientMsgId: clientMsgId(), body: "", blockRefs: [{ messageId: card.id, blockId: "card", label: "10 月数据月报" }] });
    expect(bare.message.body).toBe("> ↳ 10 月数据月报");
    // Length limit counts the quote lines.
    expect(await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "啊".repeat(32_760), blockRefs: [ref] })).toMatchObject({ ok: false, error: { code: "invalid" } });
    // cardRev newer than the card; a 产出 version that does not exist.
    expect(await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", blockRefs: [{ ...ref, cardRev: 5 }] })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(
      await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", blockRefs: [{ artifact: { artifactId: "01JC0ART000000000000000NOP", version: 1 }, blockId: "page", label: "页面" }] }),
    ).toMatchObject({ ok: false, error: { code: "not_found" } });
    // A source in another group.
    const other = await createGroup(owner, [alice.id], "other");
    const elsewhere = await co.ok({ t: "send", groupId: other, clientMsgId: clientMsgId(), body: "别处" });
    expect(await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", blockRefs: [{ messageId: elsewhere.message.id, blockId: "md:0", label: "别处" }] })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });

    // Edit: a new device sends the text only, an old one the full body; the prefix is written once.
    const e1 = await ca.ok({ t: "edit", groupId: gid, messageId: sent.message.id, body: "为什么少了？" });
    expect(e1.message.body).toBe("> ↳ 10 月数据月报 · 新增群 · 10 月 · 10-13 · 12\n\n为什么少了？");
    const e2 = await ca.ok({ t: "edit", groupId: gid, messageId: sent.message.id, body: e1.message.body });
    expect(e2.message.body).toBe(e1.message.body);

    // Delete the source: the ref keeps its address but loses label and value.
    const md = await co.ok({ t: "send", groupId: gid, topicId: card.topicId, clientMsgId: clientMsgId(), body: "| a |\n| - |\n| 1 |" });
    const q2 = await ca.ok({ t: "send", groupId: gid, topicId: card.topicId, clientMsgId: clientMsgId(), body: "这个呢？", blockRefs: [{ messageId: md.message.id, blockId: "md:0", label: "Joye · 表格 · 第 1 行" }] });
    await co.ok({ t: "delete", groupId: gid, messageId: md.message.id });
    const upd = await ca.waitEvent((e) => e.type === "message.updated" && e.data.messageId === q2.message.id);
    expect(upd.data.body).toBe("> ↳ 原消息已删除\n\n这个呢？");
    expect(upd.data.blockRefs).toEqual([{ messageId: md.message.id, blockId: "md:0", label: "", deleted: true, sourceAuthorId: owner.id, sourceTopicId: card.topicId }]);
    // A deleted source is not_found for a new ref.
    expect(await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", blockRefs: [{ messageId: md.message.id, blockId: "md:0", label: "表格" }] })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    // The stored message.created is rewritten too (catch-up readers see the same).
    const page = await ca.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const created = page.events.find((e: any) => e.type === "message.created" && e.data.message.id === q2.message.id);
    expect(created.data.message.body).toBe("> ↳ 原消息已删除\n\n这个呢？");
    expect(created.data.message.blockRefs[0]).toMatchObject({ label: "", deleted: true });
    for (const c of [co, ca, cbot]) c.close();
  });

  it("name card.limits for a poll with more than 200 voters", async () => {
    const { gid, co, ca, cb, cbot } = await setup();
    const poll = (voters: unknown) => ({
      v: 2,
      catalog: "tc.card/2.1",
      rev: 1,
      intent: "info",
      title: "投票",
      fallback: "投票",
      blocks: [{ id: "p", type: "poll", label: "选哪个", voters, closesAt: Date.now() + 3_600_000, options: [{ id: "a", label: "A" }, { id: "b", label: "B" }], fallbackText: "投票" }],
    });
    const many = await cbot.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: poll(Array.from({ length: 201 }, (_, i) => `m${i}`)) });
    expect(many).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    const none = await cbot.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: poll([]) });
    expect(none).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    for (const c of [co, ca, cb, cbot]) c.close();
  });

  it("accept a diagram block, refuse forbidden constructs, unknown types and a third diagram, and show the source to v1 readers", async () => {
    const { alice, gid, cbot } = await setup();
    const diagram = (id: string, source: string) => ({ id, type: "diagram" as const, syntax: "mermaid", source, title: "发版流程", fallbackText: "发版流程图" });
    const base: CardV2 = { v: 2, catalog: "tc.card/2.1", rev: 1, intent: "info", title: "流程", fallback: "发版流程", blocks: [diagram("d1", "flowchart TD\n  A[提交] --> B[测试]")] };
    const m = (await send(cbot, gid, base)).message;
    expect(m.card.blocks).toContainEqual({ type: "text", text: "**发版流程**" });
    expect(m.card.blocks).toContainEqual({ type: "code", lang: "mermaid", text: "flowchart TD\n  A[提交] --> B[测试]" });
    const bad = async (source: string) =>
      cbot.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: { ...base, blocks: [diagram("d1", source)] } });
    for (const s of ["flowchart TD\n  A --> B\n  click A callback", "%%{init: {}}%%\nflowchart TD\n A-->B", "---\ntitle: x\n---\nflowchart TD\n A-->B", "flowchart TD\n A[<script>x</script>]"]) {
      expect(await bad(s), s).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    }
    expect(await bad("pie\n  \"a\": 1")).toMatchObject({ ok: false, error: { code: "invalid" } });
    const three = await cbot.request({
      t: "send",
      groupId: gid,
      clientMsgId: clientMsgId(),
      body: "",
      cardV2: { ...base, blocks: [diagram("d1", "graph TD\n A-->B"), diagram("d2", "graph TD\n A-->B"), diagram("d3", "graph TD\n A-->B")] },
    });
    expect(three).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    // An embed names a 产出 version of this group, and only page events it declares.
    const ART = "01JC0ART00000000000000EMB1";
    await runInDurableObject(roomOf(gid), async (_o, state) => {
      const now = Date.now();
      state.storage.sql.exec(
        "INSERT INTO artifacts (id, topic_id, title, kind, current_version, created_by, created_at, updated_at) VALUES (?, ?, '预算', 'html', 1, ?, ?, ?)",
        ART,
        m.topicId,
        m.authorId,
        now,
        now,
      );
      state.storage.sql.exec(
        "INSERT INTO artifact_versions (artifact_id, version, entry, files, note, created_by, created_at, run_id, events) VALUES (?, 1, 'index.html', '[]', NULL, ?, ?, NULL, ?)",
        ART,
        m.authorId,
        now,
        JSON.stringify([{ name: "budget.submit", kind: "emit" }]),
      );
    });
    const embed = (events: string[], version = 1) =>
      cbot.request({
        t: "send",
        groupId: gid,
        clientMsgId: clientMsgId(),
        body: "",
        cardV2: { ...base, blocks: [{ id: "page", type: "embed", artifactId: ART, version, height: 360, events, fallbackText: "预算页面" }] },
      });
    expect(await embed(["budget.submit"])).toMatchObject({ ok: true });
    expect(await embed(["budget.delete"])).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.undeclared_event" } });
    expect(await embed([], 2)).toMatchObject({ ok: false, error: { code: "invalid" } });
    // A secret input is refused with its own reason.
    const secret = await cbot.request({
      t: "send",
      groupId: gid,
      clientMsgId: clientMsgId(),
      body: "",
      cardV2: { ...base, ask: { from: [alice.id] }, blocks: [{ id: "k", type: "input", kind: "secret", label: "密钥" }] },
    });
    expect(secret).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.secret_deferred" } });
    cbot.close();
  });
});
