// 盯活 (agent/supervise.ts, bots/desk/stall.ts): the desk nudges a silent run, restarts a
// stalled one elsewhere, reminds a looping one, continues a promise without work and answers
// progress questions itself, all before it tells people.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { CONTINUE_TEXT, LOOP_LIMIT, NUDGE_MS, isProgressQuestion, isPromiseOnly, progressAnswer, stepKey } from "../src/agent/supervise";
import { STALL_MS, TEXT, workTime } from "../src/bots/dispatch";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const benv = env as unknown as { BOT_DESK: DurableObjectNamespace };
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const silence = (botId: string, runId: string, ms: number) =>
  runInDurableObject(deskOf(botId), (_o, state: DurableObjectState) => {
    state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - ms, runId);
  });

describe("supervise rules", () => {
  it("knows promises, progress questions and how to answer them", () => {
    expect(isPromiseOnly("好的，我来看看。", 0, true)).toBe(true);
    expect(isPromiseOnly("Let me look into it.", 0, true)).toBe(true);
    expect(isPromiseOnly("我来看看", 2, true)).toBe(false);
    expect(isPromiseOnly("修好了：测试都通过了。", 0, true)).toBe(false);
    // 盯 1.10: a short answer to a request without a work order is an answer, not a promise.
    expect(isPromiseOnly("我会在周会上提", 0, false)).toBe(false);
    expect(isPromiseOnly("我会在周会上提", 0, true)).toBe(true);
    // 盯 1.11: the same step is the same title and the same output.
    expect(stepKey("gh pr checks 12", "pending")).toBe(stepKey("gh  pr checks 12", " pending\n"));
    expect(stepKey("gh pr checks 12", "pending")).not.toBe(stepKey("gh pr checks 12", "pass"));
    expect(stepKey("", "x")).toBe("");
    expect([workTime(45), workTime(120), workTime(150), workTime(0.2)]).toEqual(["45 分钟", "2 小时", "2.5 小时", "1 分钟"]);
    expect(isProgressQuestion("@dev 进展怎么样了？")).toBe(true);
    expect(isProgressQuestion("好了吗")).toBe(true);
    expect(isProgressQuestion("把进度条的颜色改成蓝色，然后把按钮放大一点再提交")).toBe(false);
    const text = progressAnswer({ startedAt: 0, now: 12 * 60_000, status: "running", plan: [{ text: "写代码", state: "done" }, { text: "跑测试", state: "current" }], lastTool: "cargo test" });
    expect(text).toBe("还在做，已经 12 分钟。\n计划 1/2 完成，正在：跑测试。\n最近一步：cargo test");
  });
});

describe("supervise", () => {
  it("nudges a silent run once, then restarts it quietly instead of saying it is stuck", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");

    await silence(w.bot.id, a.runId, NUDGE_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const nudge = await d.next((f) => f.t === "run.input" && f.runId === a.runId);
    expect(nudge.inputId).toMatch(/^desk-nudge-/);
    expect(nudge.text).toContain("No progress for 5 minutes");

    await silence(w.bot.id, a.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.topicId).toBe(topicId);
    expect(b.resumeSessionId).toBeUndefined();
    const notes = c.events().filter((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === topicId);
    expect(notes).toHaveLength(0);
    const carried = await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) =>
      state.storage.sql.exec<{ interventions: number }>("SELECT interventions FROM runs WHERE id = ?", b.runId).one().interventions,
    );
    expect(carried).toBe(2);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("leaves a run alone while it waits for a long step, and nudges it only when it is silent between steps", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "tool.call", itemId: "t1", tool: "bash", title: "bun run test" }));

    // A 12 minute test run: no nudge, no restart, and the daemon's own stall report does not count.
    await silence(w.bot.id, a.runId, STALL_MS + 2 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.acked(a.runId, d.event(a.runId, { type: "status", state: "stalled", message: "no output for 5 minutes" }));
    expect(d.frames.filter((f) => (f.t === "run.input" || f.t === "run.cancel") && f.runId === a.runId)).toHaveLength(0);
    const row = () =>
      runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) =>
        state.storage.sql.exec<{ status: string; stalled: number; open_tools: number }>("SELECT status, stalled, open_tools FROM runs WHERE id = ?", a.runId).one(),
      );
    expect(await row()).toEqual({ status: "running", stalled: 0, open_tools: 1 });

    // The step is over and the CLI says nothing more: now it is silent.
    await d.acked(a.runId, d.event(a.runId, { type: "tool.result", itemId: "t1", ok: true }));
    expect((await row()).open_tools).toBe(0);
    await silence(w.bot.id, a.runId, NUDGE_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.input" && f.runId === a.runId && String(f.inputId).startsWith("desk-nudge-"));
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("reminds a run that repeats a step, continues a promise without work, and answers progress itself", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    // 盯 1.11: polling with an output that changes is not a loop.
    for (let i = 0; i < LOOP_LIMIT + 1; i++) {
      d.event(a.runId, { type: "tool.call", itemId: `p${i}`, tool: "bash", title: "gh pr checks 12" });
      d.event(a.runId, { type: "tool.result", itemId: `p${i}`, ok: true, output: `build pending (${i * 30}s)` });
    }
    // The same step with the same output is.
    let last = 0;
    for (let i = 0; i < LOOP_LIMIT; i++) {
      d.event(a.runId, { type: "tool.call", itemId: `t${i}`, tool: "bash", title: "cargo test -p app" });
      last = d.event(a.runId, { type: "tool.result", itemId: `t${i}`, ok: false, output: "error[E0425]: cannot find value `x`", exitCode: 101 });
    }
    await d.acked(a.runId, last);
    const loop = await d.next((f) => f.t === "run.input" && String(f.inputId).startsWith("desk-loop-"));
    expect(loop.text).toContain("same step 4 times");
    expect(loop.text).toContain("cargo test -p app");
    expect(d.frames.filter((f) => f.t === "run.input" && String(f.inputId).startsWith("desk-loop-"))).toHaveLength(1);
    // 盯 5.5: the reminder is kept with the run, with its reason.
    const looped = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && (e.data.run.supervision ?? []).length > 0);
    expect(looped.data.run.supervision).toEqual([expect.objectContaining({ action: "loop", reason: "same_step" })]);

    // A progress question: the desk answers, the CLI gets nothing.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "进展怎么样了" });
    const answer = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === topicId);
    expect(answer.data.message.body).toMatch(/^(还在做|刚开始做)/);
    expect(answer.data.message.body).toContain("最近一步：cargo test -p app");
    expect(d.frames.filter((f) => f.t === "run.input" && f.runId === a.runId && !String(f.inputId).startsWith("desk-"))).toHaveLength(0);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "测试修好了" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // 盯 1.10: a short answer without a work order is an answer: no push.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 这个要在周会上说吗", mentions: [w.bot.id] });
    const q = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    // Even as a follow-up of work that had a 交代单: this turn has none of its own.
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      const key = `request:${a.runId}`;
      const req = JSON.parse(state.storage.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = ?", key).one().value);
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = ?", JSON.stringify({ ...req, brief: { goal: "修测试", doneWhen: [] } }), key);
      state.storage.sql.exec("UPDATE runs SET follow_up_of = ? WHERE id = ?", a.runId, q.runId);
    });
    d.send({ t: "run.accept", runId: q.runId });
    await d.acked(q.runId, d.done(q.runId, { status: "succeeded", resultText: "我会在周会上提" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === q.runId && e.data.run.status === "done");
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
    expect(await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => state.storage.sql.exec<{ acts: string | null }>("SELECT acts FROM runs WHERE id = ?", q.runId).one().acts)).toBeNull();

    // Only a promise, no tool call, and a work order (交代单): the desk continues the session.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再看下文档", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== q.runId);
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      const key = `request:${b.runId}`;
      const req = JSON.parse(state.storage.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = ?", key).one().value);
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = ?", JSON.stringify({ ...req, brief: { goal: "检查文档", doneWhen: [] } }), key);
    });
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "好的，我来看看。" }));
    const cont = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== b.runId && f.runId !== q.runId);
    expect(cont.prompt).toContain(CONTINUE_TEXT);
    const continued = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && (e.data.run.supervision ?? []).some((x: { action: string }) => x.action === "continue"));
    expect(continued.data.run.supervision).toEqual([expect.objectContaining({ action: "continue", reason: "promise" })]);
    d.send({ t: "run.accept", runId: cont.runId });
    await d.acked(cont.runId, d.done(cont.runId, { status: "succeeded", resultText: "文档也改好了" }));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(4);
    d.close();
    c.close();
  });
});

// ---------------------------------------------------------------------------
// 盯 1.1, 1.3, 机 2.1, 2.2: a computer that goes away is waited for, not punished
// ---------------------------------------------------------------------------

const benvLink = env as unknown as { MACHINE_LINK: DurableObjectNamespace };
const linkOf = (machineId: string) => benvLink.MACHINE_LINK.get(benvLink.MACHINE_LINK.idFromName(machineId));
/** The machine's last heartbeat lies `ms` back. */
const staleHeartbeat = (machineId: string, ms: number) =>
  runInDurableObject(linkOf(machineId), (_o, state: DurableObjectState) => {
    state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'last_heartbeat_at'", String(Date.now() - ms));
  });
const deskRow = (botId: string, runId: string) =>
  runInDurableObject(deskOf(botId), (_o, state: DurableObjectState) =>
    state.storage.sql
      .exec<{ status: string; ended_at: number | null; away_since: number | null; stall_step: number; interventions: number; stalled: number }>(
        "SELECT status, ended_at, away_since, stall_step, interventions, stalled FROM runs WHERE id = ?",
        runId,
      )
      .one(),
  );
const botNotes = (c: Client, topicId: string, botId: string): string[] =>
  c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.authorId === botId && e.data.message.topicId === topicId)
    .map((e) => String(e.data.message.body));
/** A second machine with the same labels as the world's. */
async function secondMachine(w: World, name: string): Promise<{ token: string; id: string }> {
  const list = await api("/v1/machines", { token: w.owner.token });
  const labels = (list.body as { id: string; labels: string[] }[]).find((x) => x.id === w.machineId)?.labels ?? [];
  const m = await api("/v1/machines", { token: w.owner.token, body: { name, labels } });
  expect(m.status, JSON.stringify(m.body)).toBe(200);
  return { token: m.body.token, id: m.body.machine.id };
}

describe("a machine that goes away", () => {
  it("is waited for past the stall time and the lease, and the run goes on when the daemon comes back with it", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    const s1 = d.event(a.runId, { type: "message.delta", itemId: "m1", text: "working" });
    await d.acked(a.runId, s1);
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");

    // No heartbeat for 3 minutes: the topic says the run waits for its computer.
    await staleHeartbeat(w.machineId, 3 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.machineAway("mac-mini")), 5000, "away note");
    expect((await deskRow(w.bot.id, a.runId)).away_since).toBeTypeOf("number");

    // Silent long past the stall time, and the lease runs out: nothing is cancelled or restarted.
    await silence(w.bot.id, a.runId, STALL_MS + 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await runInDurableObject(linkOf(w.machineId), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET lease_until = ? WHERE run_id = ?", Date.now() - 1, a.runId);
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    await new Promise((r) => setTimeout(r, 200));
    expect(await deskRow(w.bot.id, a.runId)).toMatchObject({ status: "running", ended_at: null, stalled: 0 });
    expect(botNotes(c, topicId, w.bot.id).filter((b) => b.startsWith("卡住了"))).toEqual([]);

    // Back with the run: the welcome keeps it, the replayed work goes on, nothing is cancelled.
    const { d: d2, welcome } = await Daemon.connect(w.machineToken, { runs: [{ runId: a.runId, lastSeq: s1 + 1, done: false }] });
    expect(welcome.runs).toEqual([{ runId: a.runId, ackedSeq: s1 }]);
    d2.seq.set(a.runId, s1);
    await d2.acked(a.runId, d2.event(a.runId, { type: "message", role: "assistant", itemId: "m1", text: "working offline" }));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.machineBack("mac-mini")), 5000, "back note");
    expect((await deskRow(w.bot.id, a.runId)).away_since).toBeNull();
    await d2.acked(a.runId, d2.done(a.runId, { status: "succeeded", resultText: "做完了" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");
    expect([...d.frames, ...d2.frames].filter((f) => f.t === "run.cancel" || (f.t === "run.assign" && f.runId !== a.runId))).toEqual([]);
    d2.close();
    c.close();
  });

  it("moves the run to another healthy machine only after 30 minutes", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, {});
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    await staleHeartbeat(w.machineId, 3 * 60_000);

    // Away for a few minutes: the run waits, even with another machine free.
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.machineAway("mac-mini")), 5000, "away note");
    expect(b.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);

    // Away for 31 minutes: it moves, with a note, and starts a new session there.
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET away_since = ? WHERE id = ?", Date.now() - 31 * 60_000, a.runId);
    });
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const moved = await b.next((f) => f.t === "run.assign");
    expect(moved.runId).not.toBe(a.runId);
    expect(moved.topicId).toBe(topicId);
    expect(moved.resumeSessionId).toBeUndefined();
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.machineMoved("mac-mini", 31)), 5000, "moved note");
    expect(await deskRow(w.bot.id, a.runId)).toMatchObject({ status: "cancelled" });
    // A move is not a try against a silent run: the count stays.
    expect((await deskRow(w.bot.id, moved.runId)).interventions).toBe(0);
    b.send({ t: "run.accept", runId: moved.runId });
    await b.acked(moved.runId, b.done(moved.runId, { status: "succeeded", resultText: "ok" }));
    b.close();
    c.close();
  });

  it("escalates a silent run in a fixed order: nudge, restart in the same session, another machine, then tell people", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "session", sessionId: "sess-a" }));
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, { features: ["resume"] });

    // 1. A nudge.
    await silence(w.bot.id, a.runId, NUDGE_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.input" && f.runId === a.runId && String(f.inputId).startsWith("desk-nudge-"));

    // 2. Still silent: interrupted and retried in the same session on the same machine.
    await silence(w.bot.id, a.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    const same = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(same.resumeSessionId).toBe("sess-a");
    expect(b.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    expect(await deskRow(w.bot.id, same.runId)).toMatchObject({ stall_step: 1, interventions: 2 });
    d.send({ t: "run.accept", runId: same.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === same.runId && e.data.run.status === "running");

    // No second nudge for the same piece of work.
    await silence(w.bot.id, same.runId, NUDGE_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    expect(d.frames.filter((f) => f.t === "run.input" && f.runId === same.runId)).toHaveLength(0);

    // 3. Still silent: another machine, fresh.
    await silence(w.bot.id, same.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === same.runId);
    const moved = await b.next((f) => f.t === "run.assign");
    expect(moved.resumeSessionId).toBeUndefined();
    expect(await deskRow(w.bot.id, moved.runId)).toMatchObject({ stall_step: 2, interventions: 3 });
    b.send({ t: "run.accept", runId: moved.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === moved.runId && e.data.run.status === "running");
    expect(botNotes(c, topicId, w.bot.id)).toEqual([]);

    // 4. Still silent: one note that names what was tried.
    await silence(w.bot.id, moved.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const gaveUp = TEXT.gaveUp(10, ["提醒", "打断后重来", "换电脑重来"]);
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(gaveUp), 5000, "gave up note");
    expect(await deskRow(w.bot.id, moved.runId)).toMatchObject({ stalled: 1, ended_at: null });
    await b.acked(moved.runId, b.done(moved.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    b.close();
    c.close();
  });
});

describe("review follow-ups", () => {
  it("retries a silent run in the same session on its own machine even when that machine is full", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"], maxParallelRuns: 1 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "session", sessionId: "sess-a" }));
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, { features: ["resume"] });

    // The cancelled run still holds the only slot until the daemon answers: the retry stays here.
    await silence(w.bot.id, a.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    const same = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(same.resumeSessionId).toBe("sess-a");
    expect(await deskRow(w.bot.id, same.runId)).toMatchObject({ stall_step: 1 });
    await new Promise((r) => setTimeout(r, 200));
    expect(b.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    d.send({ t: "run.accept", runId: same.runId });
    await d.acked(same.runId, d.done(same.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    b.close();
    c.close();
  });

  it("says the computers are busy when the only one turns a run down", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.reject", runId: a.runId, reason: "busy", message: "max_parallel_runs reached" });
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.allBusy), 5000, "busy note");
    expect(botNotes(c, topicId, w.bot.id)).not.toContain(TEXT.noMachine);
    d.close();
    c.close();
  });

  it("names only the tries that happened, and says the computer is back only when it shows the run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");

    // Away, then online again without replaying anything: the wait ends, but no "back" note.
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    await staleHeartbeat(w.machineId, 3 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.machineAway("mac-mini")), 5000, "away note");
    const { d: d2 } = await Daemon.connect(w.machineToken, {});
    await runDurableObjectAlarm(deskOf(w.bot.id));
    expect((await deskRow(w.bot.id, a.runId)).away_since).toBeNull();
    await new Promise((r) => setTimeout(r, 200));
    expect(botNotes(c, topicId, w.bot.id)).not.toContain(TEXT.machineBack("mac-mini"));

    // Loop reminders and restarts raised the count, but no nudge was sent: the note leaves 提醒 out.
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET interventions = 3, stall_step = 2, nudge_sent = 0, last_event_at = ? WHERE id = ?", Date.now() - STALL_MS - 1000, a.runId);
    });
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const gaveUp = TEXT.gaveUp(10, ["打断后重来", "换电脑重来"]);
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(gaveUp), 5000, "gave up note");
    d2.close();
    c.close();
  });
});

describe("a member pinned to a computer that is offline (机 2.2)", () => {
  it("queues with the computer's name, can be stopped, says why when it waited too long, and starts when the computer is back", async () => {
    const w = await world();
    const r = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", machineId: w.machineId } });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    // The computer was set up once, then closed.
    const { d } = await Daemon.connect(w.machineToken, {});
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    await staleHeartbeat(w.machineId, 3 * 60_000);

    const { c } = await Client.connect(w.human.token);
    const queued = async (body: string) => {
      const topicId = await mention(c, w, body);
      const run = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.queuedAt !== undefined)).data.run;
      await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.waitOffline(["mac-mini"])), 5000, "wait note");
      expect(run.status).toBe("created");
      return { topicId, runId: run.id as string };
    };
    const first = await queued("@dev 第一件事");
    const stopped = await queued("@dev 第二件事");
    const late = await queued("@dev 第三件事");

    // A person stops one.
    const cancel = await api(`/v1/runs/${stopped.runId}/cancel`, { method: "POST", token: w.human.token });
    expect(cancel.status, JSON.stringify(cancel.body)).toBe(200);
    expect(cancel.body.status).toBe("cancelled");

    // One waited past the limit: the note says it waited for the computer.
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET queued_at = ? WHERE id = ?", Date.now() - 25 * 60 * 60_000, late.runId);
    });
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await waitUntil(() => botNotes(c, late.topicId, w.bot.id).includes(TEXT.queueExpired(24, ["mac-mini"])), 5000, "expired note");

    // The computer comes back: the first one starts by itself; the stopped one does not.
    const { d: back } = await Daemon.connect(w.machineToken, {});
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const a = await back.next((f) => f.t === "run.assign");
    expect(a.runId).toBe(first.runId);
    await new Promise((res) => setTimeout(res, 200));
    expect(back.frames.filter((f) => f.t === "run.assign").map((f) => f.runId)).toEqual([first.runId]);
    back.send({ t: "run.accept", runId: a.runId });
    await back.acked(a.runId, back.done(a.runId, { status: "succeeded", resultText: "ok" }));
    back.close();
    c.close();
  });
});
