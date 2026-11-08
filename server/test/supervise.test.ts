// 盯活 (agent/supervise.ts, bots/desk/stall.ts): the desk nudges a silent run, restarts a
// stalled one elsewhere, reminds a looping one, continues a promise without work and answers
// progress questions itself, all before it tells people.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { CONTINUE_TEXT, LOOP_LIMIT, NUDGE_MS, isProgressQuestion, isPromiseOnly, progressAnswer } from "../src/agent/supervise";
import { STALL_MS } from "../src/bots/dispatch";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, clientMsgId, protocolViolations } from "./helpers";

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
    expect(isPromiseOnly("好的，我来看看。", 0)).toBe(true);
    expect(isPromiseOnly("Let me look into it.", 0)).toBe(true);
    expect(isPromiseOnly("我来看看", 2)).toBe(false);
    expect(isPromiseOnly("修好了：测试都通过了。", 0)).toBe(false);
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

  it("reminds a run that repeats a step, continues a promise without work, and answers progress itself", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    for (let i = 0; i < LOOP_LIMIT; i++) d.event(a.runId, { type: "tool.call", itemId: `t${i}`, tool: "bash", title: "cargo test -p app" });
    const loop = await d.next((f) => f.t === "run.input" && String(f.inputId).startsWith("desk-loop-"));
    expect(loop.text).toContain("same step 4 times");

    // A progress question: the desk answers, the CLI gets nothing.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "进展怎么样了" });
    const answer = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === topicId);
    expect(answer.data.message.body).toMatch(/^(还在做|刚开始做)/);
    expect(answer.data.message.body).toContain("最近一步：cargo test -p app");
    expect(d.frames.filter((f) => f.t === "run.input" && f.runId === a.runId && !String(f.inputId).startsWith("desk-"))).toHaveLength(0);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "测试修好了" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // Only a promise, no tool call: the desk continues the session.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再看下文档", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "好的，我来看看。" }));
    const cont = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== b.runId);
    expect(cont.prompt).toContain(CONTINUE_TEXT);
    d.send({ t: "run.accept", runId: cont.runId });
    await d.acked(cont.runId, d.done(cont.runId, { status: "succeeded", resultText: "文档也改好了" }));
    d.close();
    c.close();
  });
});
