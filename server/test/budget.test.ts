// 到了预算先停下来问: an AI member at its weekly budget starts no new run; the owners decide.
import { ActivityResponse, TeamUsage } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { raiseCents } from "../src/budget";
import { resetPriceCache } from "../src/usage/prices";
import { Daemon, mention, world } from "./daemon";
import { Client, api, clientMsgId, waitUntil } from "./helpers";

/** A world whose AI member spent $1.25 this week (1M Codex input tokens at list price). */
async function spent(budgetCents: number | null) {
  resetPriceCache();
  const w = await world();
  await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: w.owner.id } });
  if (budgetCents !== null) await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { weeklyBudgetCents: budgetCents } });
  const { d } = await Daemon.connect(w.machineToken);
  const { c } = await Client.connect(w.human.token);
  const { c: co } = await Client.connect(w.owner.token);
  await mention(c, w, "@dev first job");
  const first = await d.next((f) => f.t === "run.assign");
  d.send({ t: "run.accept", runId: first.runId });
  const done = d.done(first.runId, {
    status: "succeeded",
    resultText: "ok",
    usage: { inputTokens: 1_000_000, outputTokens: 0, models: [{ model: "gpt-5-codex", inputTokens: 1_000_000, outputTokens: 0 }] },
  });
  await d.acked(first.runId, done);
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.endedAt);
  return { w, d, c, co };
}

function budgetCard(c: Client, topicId: string) {
  return waitUntil(
    () =>
      c
        .events()
        .find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card" && e.data.message.card?.actions?.[0]?.id?.startsWith("budget-"))
        ?.data.message,
    5000,
    "budget card",
  );
}

describe("budget pause", () => {
  it("adds half the budget, rounded up to whole dollars", () => {
    expect(raiseCents(10_000)).toBe(5_000);
    expect(raiseCents(150)).toBe(100);
    expect(raiseCents(30)).toBe(100);
  });

  it("holds a new run at the budget, asks the owners, and starts it when they add this week", async () => {
    const { w, d, c, co } = await spent(100);
    const assigned = d.frames.filter((f) => f.t === "run.assign").length;
    const topicId = await mention(c, w, "@dev second job");
    const card = await budgetCard(c, topicId);
    expect(card.card.title).toBe("dev 这周花了 $1.25，到了预算。继续吗？");
    expect(card.card.actions.map((x: { id: string; label: string }) => [x.id.split(":")[0], x.label])).toEqual([
      ["budget-raise", "这周再加 $1"],
      ["budget-edit", "改每周预算…"],
      ["budget-stop", "先停"],
    ]);
    expect(card.mentions).toEqual([w.owner.id]);
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "waiting_answer");
    // Nothing of this topic went to the machine.
    const heldId = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId)).data.run.id;
    expect(d.frames.some((f) => f.t === "run.assign" && f.runId === heldId)).toBe(false);
    const u = TeamUsage.parse((await api(`/v1/usage?from=${Date.now() - 86_400_000}&to=${Date.now() + 1}`, { token: w.owner.token })).body);
    expect(u.bots.find((b) => b.memberId === w.bot.id)?.heldRuns).toBe(1);

    // Only an owner decides.
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: card.card.actions[0].id });
    expect(d.frames.some((f) => f.t === "run.assign" && f.runId === heldId)).toBe(false);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: card.card.actions[0].id });
    // The held run itself starts: the topic has that one run.
    const runs = new Set(c.events().filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run.id));
    expect(runs.size).toBe(1);
    const [held] = [...runs];
    await d.next((f) => f.t === "run.assign" && f.runId === held);
    const note = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && String(e.data.message.body).includes("加了 $1 预算"))?.data.message,
      5000,
      "raised note",
    );
    expect(note.body).toBe("Joye 这周给 dev 加了 $1 预算，接着做。");
    const a = ActivityResponse.parse((await api("/v1/activity?kind=budget", { token: w.owner.token })).body);
    expect(a.entries.map((e) => e.action)).toEqual(expect.arrayContaining(["budget.paused", "budget.raised"]));
    expect(a.entries.find((e) => e.action === "budget.raised")).toMatchObject({ actorId: w.owner.id, target: w.bot.id, details: { extraCents: 100 } });
  });

  it("cancels the held run when the owners choose 先停, and says so in the topic", async () => {
    const { w, d, c, co } = await spent(100);
    const assigned = d.frames.filter((f) => f.t === "run.assign").length;
    const topicId = await mention(c, w, "@dev third job");
    const card = await budgetCard(c, topicId);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: card.card.actions[2].id });
    const run = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.status === "cancelled");
    expect(run.data.run.endedAt).toBeTypeOf("number");
    const said = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && String(e.data.message.body).includes("这件事没有做"))?.data.message,
      5000,
      "stopped note",
    );
    expect(said.body).toBe("这周的预算用完了，Joye 让 dev 先停下，这件事没有做。要继续，在 团队管理 › 用量 里改预算后再 @dev。");
    expect(d.frames.some((f) => f.t === "run.assign" && f.runId === run.data.run.id)).toBe(false);
    // The activity row is written after the run ends.
    const stopped = await waitUntil(
      async () => ActivityResponse.parse((await api("/v1/activity?kind=budget", { token: w.owner.token })).body).entries.find((e) => e.action === "budget.stopped"),
      5000,
      "budget.stopped activity",
    );
    expect(stopped).toMatchObject({ actorId: w.owner.id, target: w.bot.id });
  });

  it("lets held runs go when the weekly budget is raised, and never holds without a budget", async () => {
    const { w, d, c } = await spent(100);
    const assigned = d.frames.filter((f) => f.t === "run.assign").length;
    const topicId = await mention(c, w, "@dev fourth job");
    await budgetCard(c, topicId);
    await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { weeklyBudgetCents: 10_000 } });
    await d.next((f) => f.t === "run.assign" && !d.frames.slice(0, assigned).includes(f));

    const free = await spent(null);
    const before = free.d.frames.filter((f) => f.t === "run.assign").length;
    await free.c.ok({ t: "send", groupId: free.w.gid, clientMsgId: clientMsgId(), body: "@dev more", mentions: [free.w.bot.id] });
    await free.d.next((f) => f.t === "run.assign" && !free.d.frames.slice(0, before).includes(f));
  });
});
