// 盯 1.4, 1.6, 1.8, 5.5 (usable work line 3, acceptance path 4): long work is not killed by the
// clock, waits do not count toward the time limit, and what the desk did is kept with the run.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { TEXT, workTime } from "../src/bots/dispatch";
import { Daemon, HEALTH, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const benv = env as unknown as { BOT_DESK: DurableObjectNamespace };
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const sql = <T>(botId: string, fn: (s: SqlStorage) => T) => runInDurableObject(deskOf(botId), (_o, state: DurableObjectState) => fn(state.storage.sql));

type LimitRow = { status: string; ended_at: number | null; worked_ms: number; limit_ms: number | null; limit_at: number | null; limit_rounds: number; progress: number; stalled: number };
const limitRow = (botId: string, runId: string) =>
  sql(botId, (s) =>
    s.exec<LimitRow>("SELECT status, ended_at, worked_ms, limit_ms, limit_at, limit_rounds, progress, stalled FROM runs WHERE id = ?", runId).one(),
  );
/** The run has worked `ms` already (its counted time), as of now. */
const workedFor = (botId: string, runId: string, ms: number) =>
  sql(botId, (s) => {
    s.exec("UPDATE runs SET worked_ms = ?, worked_at = ?, last_event_at = ? WHERE id = ?", ms, Date.now(), Date.now(), runId);
  });
const actsOfRun = (botId: string, runId: string) =>
  sql(botId, (s) => {
    const raw = s.exec<{ acts: string | null }>("SELECT acts FROM runs WHERE id = ?", runId).one().acts;
    return (raw ? JSON.parse(raw) : []) as { action: string; reason: string; minutes?: number }[];
  });
const botNotes = (c: Client, topicId: string, botId: string): string[] =>
  c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.authorId === botId && e.data.message.topicId === topicId)
    .map((e) => String(e.data.message.body));
const limitInput = (f: { t: string; runId: string; inputId?: string }, runId: string) => f.t === "run.input" && f.runId === runId && String(f.inputId).startsWith("desk-limit-");

describe("a long step whose processes work (盯 1.4)", () => {
  it("is not stalled past 30 minutes while the daemon says it is working, and is once it no longer does", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "tool.call", itemId: "t1", tool: "bash", title: "cargo build --release" }));

    // 40 minutes into the build its processes still use CPU, and the heartbeat says so.
    await sql(w.bot.id, (s) => s.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - 40 * 60_000, a.runId));
    d.send({ t: "heartbeat", at: Date.now(), health: HEALTH, runs: [{ runId: a.runId, lastSeq: 1, done: false, active: true }] });
    await waitUntil(async () => (await sql(w.bot.id, (s) => s.exec<{ alive_at: number | null }>("SELECT alive_at FROM runs WHERE id = ?", a.runId).one().alive_at)) !== null, 5000, "alive");
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => (f.t === "run.cancel" || f.t === "run.input") && f.runId === a.runId)).toHaveLength(0);
    expect(await limitRow(w.bot.id, a.runId)).toMatchObject({ status: "running", stalled: 0 });

    // Its processes went quiet 31 minutes ago: now it is a stuck step, and the desk retries it.
    await sql(w.bot.id, (s) => s.exec("UPDATE runs SET alive_at = ? WHERE id = ?", Date.now() - 31 * 60_000, a.runId));
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    // 盯 5.5: the retry is kept with the run, with its reason and the minutes of silence.
    expect(await actsOfRun(w.bot.id, a.runId)).toEqual([expect.objectContaining({ action: "retry", reason: "silent", minutes: 31 })]);
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("does not keep a quiet CLI between steps awake: only a step it waits for counts", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "message", itemId: "m1", role: "assistant", text: "thinking" }));
    d.send({ t: "heartbeat", at: Date.now(), health: HEALTH, runs: [{ runId: a.runId, lastSeq: 1, done: false, active: true }] });
    await new Promise((r) => setTimeout(r, 300));
    expect((await sql(w.bot.id, (s) => s.exec<{ alive_at: number | null }>("SELECT alive_at FROM runs WHERE id = ?", a.runId).one())).alive_at).toBeNull();
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });
});

describe("the time limit (盯 1.6, 1.8)", () => {
  it("is a team setting administrators change, and the daemon's deadline lies past it", async () => {
    const w = await world();
    expect((await api("/v1/settings/runs", { token: w.human.token })).body).toMatchObject({ limitMinutes: 120 });
    expect((await api("/v1/settings/runs", { method: "PATCH", token: w.human.token, body: { limitMinutes: 240 } })).status).toBe(403);
    expect((await api("/v1/settings/runs", { method: "PATCH", token: w.owner.token, body: { limitMinutes: 10 } })).status).toBe(400);
    const set = await api("/v1/settings/runs", { method: "PATCH", token: w.owner.token, body: { limitMinutes: 240 } });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    expect(set.body.limitMinutes).toBe(240);
    expect((await api("/v1/settings/runs", { token: w.human.token })).body.limitMinutes).toBe(240);
    try {
      const { d } = await Daemon.connect(w.machineToken, {});
      const { c } = await Client.connect(w.human.token);
      await mention(c, w);
      const a = await d.next((f) => f.t === "run.assign");
      expect(a.maxDurationMs).toBe(240 * 60_000 + 30 * 60_000);
      expect((await limitRow(w.bot.id, a.runId)).limit_ms).toBe(240 * 60_000);
      d.send({ t: "run.accept", runId: a.runId });
      await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
      d.close();
      c.close();
    } finally {
      await api("/v1/settings/runs", { method: "PATCH", token: w.owner.token, body: { limitMinutes: 120 } });
    }
  });

  it("does not count the wait for an approval", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");
    await workedFor(w.bot.id, a.runId, 60 * 60_000);
    await d.acked(a.runId, d.event(a.runId, { type: "approval.request", approvalId: "ap1", title: "git push origin main" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "waiting");
    // A night of waiting for the answer: the clock stands still.
    await sql(w.bot.id, (s) => s.exec("UPDATE runs SET worked_at = ? WHERE id = ?", Date.now() - 10 * 60 * 60_000, a.runId));
    await runDurableObjectAlarm(deskOf(w.bot.id));
    expect(await limitRow(w.bot.id, a.runId)).toMatchObject({ worked_ms: 60 * 60_000, limit_at: null });
    expect(d.frames.filter((f) => f.t === "run.input" && f.runId === a.runId)).toHaveLength(0);
    // Running again: the clock goes on from here, not from before the wait.
    await d.acked(a.runId, d.event(a.runId, { type: "approval.resolved", approvalId: "ap1", decision: "approve" }));
    await sql(w.bot.id, (s) => s.exec("UPDATE runs SET worked_at = ? WHERE id = ?", Date.now() - 60_000, a.runId));
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const row = await limitRow(w.bot.id, a.runId);
    expect(row.worked_ms).toBeGreaterThanOrEqual(61 * 60_000);
    expect(row.worked_ms).toBeLessThan(62 * 60_000);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("asks the run to save and report, goes on in the same session, and stops after two turns without progress", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "session", sessionId: "sess-long" }));
    await d.acked(a.runId, d.event(a.runId, { type: "tool.call", itemId: "t1", tool: "bash", title: "cargo test" }));
    await d.acked(a.runId, d.event(a.runId, { type: "tool.result", itemId: "t1", ok: true, output: "ok" }));

    // Two hours of work: the run is told to save its state and end its turn, not stopped.
    await workedFor(w.bot.id, a.runId, 120 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const told = await d.next((f) => limitInput(f, a.runId));
    expect(told.text).toContain("120 minutes");
    expect(told.text).toContain("Save your state");
    expect(d.frames.filter((f) => f.t === "run.cancel" && f.runId === a.runId)).toHaveLength(0);
    const limited = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && (e.data.run.supervision ?? []).some((x: { action: string }) => x.action === "limit"));
    expect(limited.data.run.status).toBe("running");
    expect(limited.data.run.supervision).toEqual([expect.objectContaining({ action: "limit", reason: "time_limit", minutes: 120 })]);

    // It took the message, saved and ended its turn: a new turn goes on in the same session.
    await d.acked(a.runId, d.event(a.runId, { type: "input.accepted", inputId: told.inputId }));
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "保存好了：做完了 3 步，还剩 2 步。" }));
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBe("sess-long");
    expect(b.prompt).toContain("time limit");
    expect(await limitRow(w.bot.id, a.runId)).toMatchObject({ status: "cancelled" });
    expect(await limitRow(w.bot.id, b.runId)).toMatchObject({ limit_rounds: 0 });
    expect((await actsOfRun(w.bot.id, a.runId)).map((x) => x.action)).toEqual(["limit", "limit_continue"]);
    // The saved note is not a result: the topic has no message from the member.
    expect(botNotes(c, topicId, w.bot.id)).toEqual([]);

    // This turn makes no progress and does not end its turn: interrupted after the grace time.
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.event(b.runId, { type: "session", sessionId: "sess-long" }));
    await d.acked(b.runId, d.event(b.runId, { type: "message", itemId: "m1", role: "assistant", text: "继续" }));
    await workedFor(w.bot.id, b.runId, 120 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => limitInput(f, b.runId));
    await sql(w.bot.id, (s) => s.exec("UPDATE runs SET limit_at = ? WHERE id = ?", Date.now() - 16 * 60_000, b.runId));
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === b.runId);
    const c3 = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== b.runId);
    expect(c3.resumeSessionId).toBe("sess-long");
    expect(await limitRow(w.bot.id, c3.runId)).toMatchObject({ limit_rounds: 1 });
    expect((await actsOfRun(w.bot.id, b.runId)).map((x) => [x.action, x.reason])).toEqual([
      ["limit", "time_limit"],
      ["limit_continue", "no_progress"],
    ]);

    // The second turn in a row without progress: it stops and says so once.
    d.send({ t: "run.accept", runId: c3.runId });
    await d.acked(c3.runId, d.event(c3.runId, { type: "message", itemId: "m1", role: "assistant", text: "还在想" }));
    await workedFor(w.bot.id, c3.runId, 120 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const toldC = await d.next((f) => limitInput(f, c3.runId));
    await d.acked(c3.runId, d.event(c3.runId, { type: "input.accepted", inputId: toldC.inputId }));
    await d.acked(c3.runId, d.done(c3.runId, { status: "succeeded", resultText: "还是没想好" }));
    const stop = TEXT.limitStopped(workTime(360), 2);
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(stop), 5000, "stop note");
    expect(await limitRow(w.bot.id, c3.runId)).toMatchObject({ status: "blocked" });
    expect((await actsOfRun(w.bot.id, c3.runId)).map((x) => [x.action, x.reason])).toEqual([
      ["limit", "time_limit"],
      ["limit_stop", "no_progress"],
    ]);
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(3);
    expect(botNotes(c, topicId, w.bot.id)).toEqual([stop]);
    d.close();
    c.close();
  });

  it("finishes normally when the run is done at the limit, and turns a daemon timeout into a new turn, never the English error", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");
    await workedFor(w.bot.id, a.runId, 121 * 60_000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => limitInput(f, a.runId));
    // The CLI ended before it took the message: it finished, and the result is posted as usual.
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "全部做完了" }));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes("全部做完了"), 5000, "result");
    expect(await limitRow(w.bot.id, a.runId)).toMatchObject({ status: "done" });

    // A daemon that stops a run at its own deadline (an older one, or the backstop).
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再做一件", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.event(b.runId, { type: "file.change", itemId: "f1", changes: [{ path: "a.ts", kind: "modify" }] }));
    await d.acked(b.runId, d.done(b.runId, { status: "timeout", error: "run took longer than maxDurationMs" }));
    const next = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== b.runId);
    expect(next.topicId).toBe(topicId);
    expect((await actsOfRun(w.bot.id, b.runId)).map((x) => x.action)).toEqual(["limit_continue"]);
    d.send({ t: "run.accept", runId: next.runId });
    await d.acked(next.runId, d.done(next.runId, { status: "succeeded", resultText: "第二件也做完了" }));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes("第二件也做完了"), 5000, "second result");
    expect(botNotes(c, topicId, w.bot.id).some((x) => x.includes("maxDurationMs"))).toBe(false);
    d.close();
    c.close();
  });
});
