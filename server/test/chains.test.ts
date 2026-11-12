// Usable (design-relay R2): chains of hand-overs (转交) and split-offs (分出去) between AI members,
// counted in one place in the room, with one way to stop a run, a whole piece of work and what a
// run gave out.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { CardV2 } from "@chat/protocol";
import type { BotsEnv } from "../src/bots/env";
import { MAX_DEPTH, MAX_HANDOFFS, confirmStep, reserveStep, stepReport, stopChain } from "../src/bots/chains";
import type { RoomContext, RoomTx } from "../src/core/registry";
import { closedCard, handoverCard, lastWords, workOrderText } from "../src/bots/desk/delegate_card";
import { topicStateText } from "../src/context/read";
import { TEXT } from "../src/bots/dispatch";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createMember, protocolViolations } from "./helpers";

const benv = env as unknown as BotsEnv;

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

/** Another runner-backed AI member in the world's group, on the world's machine. */
async function addBot(w: World, name: string): Promise<TestMember> {
  const bot = await createMember({ kind: "bot", name });
  const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
  expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: runner.body.labels } })).status).toBe(200);
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: bot.id } })).status).toBe(200);
  return bot;
}

function room(gid: string) {
  return benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(gid));
}

async function inRoom<T>(gid: string, fn: (sql: SqlStorage) => T): Promise<T> {
  return runInDurableObject(room(gid), (_o, state: DurableObjectState) => fn(state.storage.sql)) as Promise<T>;
}

/** Just enough of the room for split-off steps (no routing owner moves, no alarm). */
function fakeRoom(sql: SqlStorage): RoomContext {
  return { sql, wakeAt: () => {} } as unknown as RoomContext;
}
const noTx = {} as RoomTx;

function outbox(sql: SqlStorage, kind: string): any[] {
  return sql
    .exec<{ payload: string }>("SELECT payload FROM bot_outbox WHERE kind = ? ORDER BY id", kind)
    .toArray()
    .map((r) => JSON.parse(r.payload));
}

/** Sends a chat-run output from a run and waits for its answer. */
async function output(d: Daemon, runId: string, outputId: string, op: string, body: unknown): Promise<any> {
  d.event(runId, { type: "output", outputId, op, body });
  return d.next((f) => f.t === "run.output.result" && f.outputId === outputId);
}

async function started(d: Daemon, botId: string, after: string[] = []): Promise<any> {
  const a = await d.next((f) => f.t === "run.assign" && f.botMemberId === botId && !after.includes(f.runId));
  d.send({ t: "run.accept", runId: a.runId });
  return a;
}

describe("the chain rules (room)", () => {
  it("counts hand-overs per chain: depth, at most 8, no cycles, nothing after a stop", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const root = reserveStep(sql, "A", { fromRun: "rA", requesterId: "h", kind: "assign", to: "B", topicId: "t1" });
      expect(root).toMatchObject({ seq: 1, depth: 1, path: ["A", "B"], root: { seq: 0, depth: 0, path: ["A"] } });
      const chain = { chainId: root.chainId, seq: 1 };
      // Back to a member on the path.
      expect(() => reserveStep(sql, "B", { fromRun: "rB", chain, requesterId: "h", kind: "handoff", to: "A", topicId: "t2" })).toThrow(
        expect.objectContaining({ reason: "delegation.cycle" }),
      );
      // Only the member of a step extends it.
      expect(() => reserveStep(sql, "C", { fromRun: "rB", chain, requesterId: "h", kind: "handoff", to: "D", topicId: "t2" })).toThrow(
        expect.objectContaining({ code: "forbidden" }),
      );
      // Depth: B → C → D is 3 deep; D cannot go on.
      const c = reserveStep(sql, "B", { fromRun: "rB", chain, requesterId: "h", kind: "handoff", to: "C", topicId: "t2" });
      const d = reserveStep(sql, "C", { fromRun: "rC", chain: { chainId: root.chainId, seq: c.seq }, requesterId: "h", kind: "assign", to: "D", topicId: "t2" });
      expect(d.depth).toBe(MAX_DEPTH);
      expect(() => reserveStep(sql, "D", { fromRun: "rD", chain: { chainId: root.chainId, seq: d.seq }, requesterId: "h", kind: "assign", to: "E", topicId: "t3" })).toThrow(
        expect.objectContaining({ reason: "delegation.too_deep" }),
      );
      // A hand-over continues where its step reports; a split-off reports to its giver.
      const steps = sql.exec<{ seq: number; parent_seq: number | null }>("SELECT seq, parent_seq FROM chain_steps WHERE chain_id = ? ORDER BY seq", root.chainId).toArray();
      expect(steps).toEqual([
        { seq: 0, parent_seq: null },
        { seq: 1, parent_seq: 0 },
        { seq: 2, parent_seq: 0 },
        { seq: 3, parent_seq: 2 },
      ]);
      // At most 8 started by members in one chain, wherever they start.
      for (let i = 0; i < MAX_HANDOFFS - 3; i++) reserveStep(sql, "A", { fromRun: "rA", chain: { chainId: root.chainId, seq: 0 }, requesterId: "h", kind: "assign", to: `X${i}`, topicId: "t1" });
      expect(() => reserveStep(sql, "A", { fromRun: "rA", chain: { chainId: root.chainId, seq: 0 }, requesterId: "h", kind: "assign", to: "Y", topicId: "t1" })).toThrow(
        expect.objectContaining({ reason: "delegation.too_many" }),
      );
      // A stopped chain refuses every later hand-over, before anything else.
      const other = reserveStep(sql, "A", { fromRun: "rZ", requesterId: "h", kind: "assign", to: "B", topicId: "t9" });
      stopChain(sql, other.chainId, "h");
      expect(() => reserveStep(sql, "A", { fromRun: "rZ", chain: { chainId: other.chainId, seq: 0 }, requesterId: "h", kind: "assign", to: "C", topicId: "t9" })).toThrow(
        expect.objectContaining({ reason: "delegation.stopped" }),
      );
    });
  });

  it("confirms a step whose run reported first: the part's own topic, and a stop in between stops it", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const room = fakeRoom(sql);
      sql.exec("DELETE FROM bot_outbox");
      const s = reserveStep(sql, "A", { fromRun: "rA", requesterId: "h", kind: "assign", to: "B", topicId: "t1" });
      // B's run starts and reports before A confirms: the step stays reserved, its run is named.
      stepReport(room, "B", { runId: "rB", ended: false, chain: { chainId: s.chainId, seq: s.seq } });
      expect(sql.exec("SELECT state, run_id, topic_id FROM chain_steps WHERE chain_id = ? AND seq = 1", s.chainId).one()).toEqual({ state: "reserved", run_id: "rB", topic_id: "t1" });
      // A person stops the whole piece of work now: only A's run is working yet.
      expect(stopChain(sql, s.chainId, "h")).toBe(1);
      confirmStep(room, noTx, "A", { chainId: s.chainId, seq: s.seq, runId: "rB", topicId: "t2" });
      expect(sql.exec("SELECT state, run_id, topic_id FROM chain_steps WHERE chain_id = ? AND seq = 1", s.chainId).one()).toEqual({ state: "working", run_id: "rB", topic_id: "t2" });
      expect(outbox(sql, "cancel").map((c) => c.runId)).toEqual(["rA", "rB"]);
      sql.exec("DELETE FROM bot_outbox");
    });
  });

  it("a retried part works again: its new result goes back, and a stop stops the retry", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const room = fakeRoom(sql);
      sql.exec("DELETE FROM bot_outbox");
      const s = reserveStep(sql, "A", { fromRun: "rA", requesterId: "h", kind: "assign", to: "B", topicId: "t1" });
      const chain = { chainId: s.chainId, seq: s.seq };
      confirmStep(room, noTx, "A", { ...chain, runId: "rB", topicId: "t2" });
      stepReport(room, "B", { runId: "rB", ended: true, chain, outcome: "failed", result: "没过" });
      // A person presses 重试 on B's run: the same step works again with the new run.
      stepReport(room, "B", { runId: "rB2", ended: false, chain });
      expect(sql.exec("SELECT state, run_id FROM chain_steps WHERE chain_id = ? AND seq = 1", s.chainId).one()).toEqual({ state: "working", run_id: "rB2" });
      stepReport(room, "B", { runId: "rB2", ended: true, chain, outcome: "done", result: "过了" });
      expect(outbox(sql, "part").map((p) => [p.runId, p.state, p.result, p.partTopicId])).toEqual([
        ["rB", "failed", "没过", "t2"],
        ["rB2", "done", "过了", "t2"],
      ]);
      // Retried again and stopped: the retry is stopped too.
      stepReport(room, "B", { runId: "rB3", ended: false, chain });
      stopChain(sql, s.chainId, "h");
      expect(outbox(sql, "cancel").map((c) => c.runId)).toContain("rB3");
      sql.exec("DELETE FROM bot_outbox");
    });
  });

  it("a part of a giver that handed its work over goes to the member that continued it", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const room = fakeRoom(sql);
      sql.exec("DELETE FROM bot_outbox");
      // A gives a part to C, then hands the work over to B.
      const c = reserveStep(sql, "A", { fromRun: "rA", requesterId: "h", kind: "assign", to: "C", topicId: "t1" });
      confirmStep(room, noTx, "A", { chainId: c.chainId, seq: c.seq, runId: "rC", topicId: "t2" });
      const root = { chainId: c.chainId, seq: 0 };
      const b = reserveStep(sql, "A", { fromRun: "rA", chain: root, requesterId: "h", kind: "handoff", to: "B", topicId: "t1" });
      sql.exec("UPDATE chain_steps SET state = 'working', run_id = 'rB' WHERE chain_id = ? AND seq = ?", b.chainId, b.seq);
      stepReport(room, "A", { runId: "rA", ended: true, chain: root, outcome: "done", handedOver: "B" });
      stepReport(room, "C", { runId: "rC", ended: true, chain: { chainId: c.chainId, seq: c.seq }, outcome: "done", result: "好了" });
      const parts = outbox(sql, "part");
      expect(parts).toHaveLength(2);
      expect(parts[0]).toMatchObject({ botId: "B", parentRun: "rB", topicId: "t1", givenBy: "A", result: "好了" });
      expect(parts[0].cardOnly).toBeUndefined();
      // A only gets its split-off card updated.
      expect(parts[1]).toMatchObject({ botId: "A", cardOnly: true });
      sql.exec("DELETE FROM bot_outbox");
    });
  });

  it("builds a valid hand-over card and a work order that carries the request and the code", () => {
    const card = handoverCard({ kind: "assign", chainId: "01J0000000000000000000000A", fromName: "小码", toName: "小窗", goal: "在 Windows 上验证", doneWhen: ["测试通过"], part: { groupId: "g1", topicId: "t2" } });
    expect(CardV2.safeParse(card).success).toBe(true);
    expect(card.title).toBe("小码 把一部分分给 小窗");
    const closed = closedCard(card, "小窗 做完了，结果已交回 小码");
    expect(CardV2.safeParse(closed).success).toBe(true);
    expect(closed.rev).toBe(2);
    expect(closed.actions).toEqual([]);
    const order = workOrderText({
      kind: "handoff",
      fromName: "小码",
      requesterName: "Joye",
      goal: "补测试",
      doneWhen: [],
      seq: 2,
      depth: 1,
      path: ["小码", "小审"],
      rootRequest: "给设置页加深色模式\n  图片 a.png",
      rootTopicId: "t1",
      fromTopicId: "t1",
      plan: [{ text: "写代码", state: "done" }],
      artifacts: [{ artifactId: "a1", version: 2, title: "设计稿" }],
      lastWords: "写完了",
      code: { branch: "dark-mode", head: "abcdef1234567890", pushed: false },
    });
    expect(order).toContain("Work order from 小码, an AI member, on behalf of Joye");
    expect(order).toContain("给设置页加深色模式");
    expect(order).toContain("a a1 v2");
    expect(order).toContain("branch dark-mode at abcdef123456 was not pushed");
    expect(order).toContain("小码 → 小审");
    expect(lastWords("想一想\n· Bash ls\n做完了")).toBe("想一想\n做完了");
  });

  it("shows the participants and the hand-overs on the state card", () => {
    const text = topicStateText(
      {
        title: "深色模式",
        messages: 3,
        authors: [],
        artifacts: [],
        openCards: [],
        decisions: [],
        participants: [{ memberId: "a", working: true, report: "in_progress", at: 0 }],
        handovers: [{ kind: "assign", from: "a", to: "b", state: "working", topicId: "t2", at: 0 }],
      },
      (id) => ({ a: "小码", b: "小窗" })[id] ?? id,
    );
    expect(text).toContain("AI members in this topic:");
    expect(text).toContain("1. 小码 →(分出去) 小窗 · working · topic t2");
  });
});

describe("hand-overs between desks", () => {
  it("a refused hand-over changes neither the count nor the routing owner", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    // Both work in the topic: the hand-over to qa is refused (it is busy there).
    const res = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev @qa 一起看看", mentions: [w.bot.id, qa.id] });
    const topicId = res.message.topicId as string;
    const a = await started(d, w.bot.id);
    await started(d, qa.id);
    for (let i = 0; i < 200; i++) {
      const n = await inRoom(w.gid, (sql) => sql.exec<{ n: number }>("SELECT count(*) AS n FROM topic_bots WHERE topic_id = ? AND working = 1", topicId).one().n);
      if (n === 2) break;
      await new Promise((r) => setTimeout(r, 20));
    }
    const owner = await inRoom(w.gid, (sql) => sql.exec<{ owner_id: string }>("SELECT owner_id FROM topics WHERE id = ?", topicId).one().owner_id);
    const r = await output(d, a.runId, "o1", "member.handoff", { member: "qa", goal: "你来" });
    expect(r).toMatchObject({ ok: false, error: { reason: "delegation.refused" } });
    const after = await inRoom(w.gid, (sql) => ({
      owner: sql.exec<{ owner_id: string }>("SELECT owner_id FROM topics WHERE id = ?", topicId).one().owner_id,
      handoffs: sql.exec<{ n: number }>("SELECT coalesce(sum(handoffs), 0) AS n FROM work_chains WHERE topic_id = ?", topicId).one().n,
      steps: sql.exec<{ n: number }>("SELECT count(*) AS n FROM chain_steps WHERE topic_id = ? AND kind != 'root'", topicId).one().n,
    }));
    expect(after).toEqual({ owner, handoffs: 0, steps: 0 });
    // No card in the topic for a refused hand-over.
    expect(c.events().some((e) => e.type === "message.created" && e.data.message.cardV2?.title?.includes("转交"))).toBe(false);
    d.close();
    c.close();
  });

  it("a hand-over moves the routing owner, the giver posts no result, and B cannot hand it back", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await started(d, w.bot.id);
    const r = await output(d, a.runId, "o1", "member.handoff", { member: "qa", goal: "补测试" });
    expect(r.ok, JSON.stringify(r)).toBe(true);
    const b = await started(d, qa.id);
    expect(await inRoom(w.gid, (sql) => sql.exec<{ owner_id: string }>("SELECT owner_id FROM topics WHERE id = ?", topicId).one().owner_id)).toBe(qa.id);
    // qa cannot hand it back to dev.
    const back = await output(d, b.runId, "o2", "member.handoff", { member: "dev", goal: "还给你" });
    expect(back).toMatchObject({ ok: false, error: { reason: "delegation.cycle" } });
    // dev's run ends: done, no result card, the summary says who took it.
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "交给 qa 了" }));
    const end = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.endedAt !== undefined);
    expect(end.data.run.status).toBe("done");
    expect(end.data.run.summary).toBe("已转交给 qa");
    expect(c.events().some((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body === "交给 qa 了")).toBe(false);
    // A follow-up without an @ goes to qa now.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "再加一个用例" });
    await d.next((f) => f.t === "run.input" && f.runId === b.runId);
    d.close();
    c.close();
  });

  it("a part a person stopped still reports back, as stopped", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    expect((await api(`/v1/runs/${b.runId}/cancel`, { method: "POST", token: w.human.token })).status).toBe(200);
    const back = await d.next((f) => f.t === "run.input" && f.runId === a.runId && String(f.inputId).startsWith("desk-part-"));
    expect(back.text).toContain("was stopped by Alice");
    // The split-off card says so once.
    await c.waitEvent((e) => e.type === "message.updated" && e.data.cardV2?.blocks?.some((x: any) => x.id === "closed" && x.text === "qa 没做完：被 Alice 停止"));
    d.close();
    c.close();
  });

  it("an idle giver gets one follow-up run once every part is back", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const ux = await addBot(w, "ux");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "测 Windows" })).ok).toBe(true);
    expect((await output(d, a.runId, "o2", "member.assign", { member: "ux", goal: "看界面" })).ok).toBe(true);
    const b = await started(d, qa.id);
    const u = await started(d, ux.id);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "等两份结果" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.endedAt !== undefined);
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "Windows 上通过" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.endedAt !== undefined);
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign" && f.botMemberId === w.bot.id)).toHaveLength(1);
    await d.acked(u.runId, d.done(u.runId, { status: "succeeded", resultText: "界面没问题" }));
    const next = await d.next((f) => f.t === "run.assign" && f.botMemberId === w.bot.id && f.runId !== a.runId);
    expect(next.prompt).toContain("Windows 上通过");
    expect(next.prompt).toContain("界面没问题");
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign" && f.botMemberId === w.bot.id)).toHaveLength(2);
    d.close();
    c.close();
  });
});

describe("split-offs after a retry and a hand-over", () => {
  it("a part does its work in its own topic, and a retried part's new result reaches the giver", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "测 Windows" })).ok).toBe(true);
    const b = await started(d, qa.id);
    const step = await inRoom(w.gid, (sql) => sql.exec<{ topic_id: string; state: string }>("SELECT topic_id, state FROM chain_steps WHERE kind = 'assign'").one());
    expect(step.state).toBe("working");
    expect(step.topic_id).not.toBe(topicId);
    await d.acked(b.runId, d.done(b.runId, { status: "failed", error: "tests failed" }));
    const first = await d.next((f) => f.t === "run.input" && f.runId === a.runId && String(f.inputId).startsWith("desk-part-"));
    expect(first.text).toContain("did not finish");
    expect(first.text).toContain(`topic ${step.topic_id}`);
    expect((await api(`/v1/runs/${b.runId}/retry`, { token: w.human.token, body: {} })).status).toBe(200);
    const b2 = await started(d, qa.id, [b.runId]);
    await d.acked(b2.runId, d.done(b2.runId, { status: "succeeded", resultText: "Windows 上通过" }));
    const second = await d.next((f) => f.t === "run.input" && f.runId === a.runId && f.inputId !== first.inputId && String(f.inputId).startsWith("desk-part-"));
    expect(second.text).toContain("is done");
    expect(second.text).toContain("Windows 上通过");
    d.close();
    c.close();
  });

  it("a giver that handed its work over is not woken by its part: the member that took it gets it", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const ux = await addBot(w, "ux");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const q = await started(d, qa.id);
    expect((await output(d, a.runId, "o2", "member.handoff", { member: "ux", goal: "你接着做" })).ok).toBe(true);
    const u = await started(d, ux.id, [q.runId]);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "交给 ux 了" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.endedAt !== undefined);
    await d.acked(q.runId, d.done(q.runId, { status: "succeeded", resultText: "文档写好了" }));
    const back = await d.next((f) => f.t === "run.input" && f.runId === u.runId && String(f.inputId).startsWith("desk-part-"));
    expect(back.text).toContain("dev gave to qa before handing the work over to you");
    expect(back.text).toContain("文档写好了");
    // dev's split-off card still says how the part ended.
    await c.waitEvent((e) => e.type === "message.updated" && e.data.cardV2?.blocks?.some((x: any) => x.id === "closed" && x.text === "qa 做完了，结果已交回 dev"));
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign" && f.botMemberId === w.bot.id)).toHaveLength(1);
    expect(await inRoom(w.gid, (sql) => sql.exec<{ owner_id: string }>("SELECT owner_id FROM topics WHERE id = ?", topicId).one().owner_id)).toBe(ux.id);
    d.close();
    c.close();
  });
});

describe("a giver that waits for a machine", () => {
  it("keeps a part that comes back while its run waits, and gives it to the run with its prompt", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const ux = await addBot(w, "ux");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 2 });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "测 Windows" })).ok).toBe(true);
    const b = await started(d, qa.id);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "先这样" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.endedAt !== undefined);
    // The machine is full again (qa and ux), so dev's next run in its topic waits (排队).
    await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@ux 看看界面", mentions: [ux.id] });
    await started(d, ux.id);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再补一个说明", mentions: [w.bot.id] });
    const queued = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.botId === w.bot.id && e.data.run.id !== a.runId && e.data.run.queuedAt !== undefined);
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "Windows 上通过" }));
    const next = await d.next((f) => f.t === "run.assign" && f.runId === queued.data.run.id, 25_000);
    expect(next.prompt).toContain("Windows 上通过");
    d.close();
    c.close();
  }, 40_000);
});

describe("a part that cannot be handed back", () => {
  it("is dropped by the room's alarm after the give-up time, and the giver's topic waits for the requester", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    // A part row whose delivery fails (its payload is broken) and that is past the give-up time.
    await inRoom(w.gid, (sql) => {
      const payload = { botId: w.bot.id, chainId: "c1", seq: 1, parentSeq: 0, parentRun: null, topicId, partTopicId: "t2", requesterId: w.human.id, state: "done", result: "x", last: true };
      sql.exec("INSERT INTO bot_outbox (kind, topic_id, payload, next_at, created_at) VALUES ('part', ?, ?, 0, 0)", topicId, JSON.stringify(payload));
    });
    await runInDurableObject(room(w.gid), (_o, state: DurableObjectState) => state.storage.setAlarm(Date.now()));
    await runDurableObjectAlarm(room(w.gid));
    // (Without a computer the run itself ended blocked first; the give-up waits for the requester.)
    const t = await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "blocked" && e.data.waitingOn?.length > 0);
    expect(t.data.waitingOn).toEqual([w.human.id]);
    expect(await inRoom(w.gid, (sql) => sql.exec<{ n: number }>("SELECT count(*) AS n FROM bot_outbox WHERE kind = 'part'").one().n)).toBe(0);
    c.close();
  });
});

describe("stopping", () => {
  it("a person stopping the giver stops what it gave out, and nothing wakes it again", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    expect((await api(`/v1/runs/${a.runId}/cancel`, { method: "POST", token: w.human.token })).status).toBe(200);
    await d.next((f) => f.t === "run.cancel" && f.runId === b.runId);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.status === "cancelled");
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign" && f.botMemberId === w.bot.id)).toHaveLength(1);
    d.close();
    c.close();
  });

  it("any person in the group stops a whole piece of work; later hand-overs in it are refused", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const ux = await addBot(w, "ux");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    const card = await c.waitEvent((e) => e.type === "message.created" && e.data.message.cardV2?.actions?.[0]?.id?.startsWith("chain.stop:"));
    const chainId = card.data.message.cardV2.actions[0].id.slice("chain.stop:".length);
    // Only people of the group.
    expect((await api(`/v1/groups/${w.gid}/chains/${chainId}/stop`, { method: "POST", token: w.bot.token })).status).toBe(403);
    const outsider = await createMember({ name: "Bob" });
    expect((await api(`/v1/groups/${w.gid}/chains/${chainId}/stop`, { method: "POST", token: outsider.token })).status).toBe(404);
    expect((await api(`/v1/groups/${w.gid}/chains/nochain/stop`, { method: "POST", token: w.human.token })).status).toBe(404);
    // qa is about to give a part out when the person stops everything.
    const res = await api(`/v1/groups/${w.gid}/chains/${chainId}/stop`, { method: "POST", token: w.human.token });
    expect(res.status, JSON.stringify(res.body)).toBe(200);
    expect(res.body).toEqual({ stoppedRuns: 2 });
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    await d.next((f) => f.t === "run.cancel" && f.runId === b.runId);
    // Every later hand-over in it is refused (the room's rule; the runs are stopped already).
    const refused = await inRoom(w.gid, (sql) => {
      try {
        reserveStep(sql, qa.id, { fromRun: b.runId, chain: { chainId, seq: 1 }, requesterId: w.human.id, kind: "assign", to: ux.id, topicId: "t" });
        return "taken";
      } catch (err) {
        return (err as { reason?: string }).reason;
      }
    });
    expect(refused).toBe("delegation.stopped");
    d.close();
    c.close();
  });

  // Verification pass 2, problem 5: a stop through the API (not the card's button) still shows in
  // the giver's topic, on the split-off card, and in the part's topic, with who stopped it.
  it("a stop shows on the split-off card and in the part's topic, with who stopped it", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    const card = await c.waitEvent((e) => e.type === "message.created" && e.data.message.cardV2?.actions?.[0]?.id?.startsWith("chain.stop:"));
    const m = card.data.message;
    const chainId = m.cardV2.actions[0].id.slice("chain.stop:".length);
    const res = await api(`/v1/groups/${w.gid}/chains/${chainId}/stop`, { method: "POST", token: w.human.token });
    expect(res.status, JSON.stringify(res.body)).toBe(200);
    const closed = await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2 && (e.data.cardV2.actions ?? []).length === 0);
    expect(closed.data.cardV2.blocks.at(-1).text).toBe("被 Alice 停止");
    const line = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === qa.id && e.data.message.body === TEXT.cancelledBy("Alice"));
    expect(line.data.message.body).toBe("已被 Alice 停止，没有完成。");
    const ended = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.endedAt !== undefined);
    expect(ended.data.run.status).toBe("cancelled");
    d.close();
    c.close();
  });

  it("全部停止 on the card stops the piece of work for the person who pressed it", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    const card = await c.waitEvent((e) => e.type === "message.created" && e.data.message.cardV2?.actions?.[0]?.id?.startsWith("chain.stop:"));
    const m = card.data.message;
    await c.ok({ t: "card.answer", groupId: w.gid, messageId: m.id, rev: m.cardV2.rev, actionId: m.cardV2.actions[0].id });
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    await d.next((f) => f.t === "run.cancel" && f.runId === b.runId);
    const ended = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.endedAt !== undefined);
    expect(ended.data.run.status).toBe("cancelled");
    // The card loses its button and says who stopped it.
    const closed = await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2 && (e.data.cardV2.actions ?? []).length === 0);
    expect(closed.data.cardV2.blocks.at(-1).text).toBe("被 Alice 停止");
    d.close();
    c.close();
  });

  it("the requester leaving the group stops his pieces of work", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await started(d, w.bot.id);
    expect((await output(d, a.runId, "o1", "member.assign", { member: "qa", goal: "写文档" })).ok).toBe(true);
    const b = await started(d, qa.id);
    expect((await api(`/v1/groups/${w.gid}/members/${w.human.id}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    await d.next((f) => f.t === "run.cancel" && f.runId === b.runId);
    const stopped = await inRoom(w.gid, (sql) => sql.exec<{ n: number }>("SELECT count(*) AS n FROM work_chains WHERE requester_id = ? AND stopped_at IS NOT NULL", w.human.id).one().n);
    expect(stopped).toBe(1);
    d.close();
    c.close();
  });
});
