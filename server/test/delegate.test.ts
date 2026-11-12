// 转交 and 分出去 (bots/desk/delegate.ts): a run hands work to another AI member through chat-run
// outputs; the desks pass it on, and a part's result comes back to the member that gave it out.
import { afterAll, describe, expect, it } from "vitest";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

/** A second runner-backed bot in the world's group, on the same machine labels. */
async function secondBot(w: Awaited<ReturnType<typeof world>>, name: string) {
  const bot = await createMember({ kind: "bot", name });
  const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
  expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: runner.body.labels } })).status).toBe(200);
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: bot.id } })).status).toBe(200);
  return bot;
}

describe("delegation", () => {
  it("hands a topic over to another member, which starts with the work order", async () => {
    const w = await world();
    const qa = await secondBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "output", outputId: "o1", op: "member.handoff", body: { member: "qa", goal: "把登录页的测试补全", doneWhen: ["测试通过"] } });
    const res = await d.next((f) => f.t === "run.output.result" && f.outputId === "o1");
    expect(res.ok, JSON.stringify(res)).toBe(true);
    expect(res.result).toMatchObject({ member: qa.id, topicId });
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === qa.id);
    expect(b.topicId).toBe(topicId);
    expect(b.prompt).toContain("Goal: 把登录页的测试补全");
    // The work order: written by code, marked as another AI member's, with the person's request.
    expect(b.prompt).toContain("Work order from dev, an AI member, on behalf of Alice");
    expect(b.prompt).toContain("please fix tests");
    // The topic shows a card with 全部停止, not a plain line.
    const card = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.cardV2);
    expect(card.data.message.cardV2.title).toBe("dev 把这件事转交给 qa");
    expect(card.data.message.cardV2.actions[0]).toMatchObject({ label: "全部停止" });
    expect(card.data.message.cardV2.actions[0].id).toMatch(/^chain\.stop:/);

    // An unknown member: the run hears which members it can name.
    d.event(a.runId, { type: "output", outputId: "o2", op: "member.handoff", body: { member: "nobody", goal: "x" } });
    const bad = await d.next((f) => f.t === "run.output.result" && f.outputId === "o2");
    expect(bad).toMatchObject({ ok: false, error: { reason: "delegation.unknown_member" } });
    expect(bad.error.message).toContain("qa");
    d.close();
    c.close();
  });

  it("gives a part out in a new topic and brings its result back to the giver's run", async () => {
    const w = await world();
    const qa = await secondBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "output", outputId: "o1", op: "member.assign", body: { member: qa.id, goal: "写接口文档" } });
    const res = await d.next((f) => f.t === "run.output.result" && f.outputId === "o1");
    expect(res.ok, JSON.stringify(res)).toBe(true);
    expect(res.result.topicId).not.toBe(topicId);
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === qa.id);
    expect(b.topicId).toBe(res.result.topicId);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "文档写好了：docs/api.md" }));
    const back = await d.next((f) => f.t === "run.input" && f.runId === a.runId && String(f.inputId).startsWith("desk-part-"));
    expect(back.text).toContain("is done");
    expect(back.text).toContain("文档写好了：docs/api.md");
    d.close();
    c.close();
  });
});
