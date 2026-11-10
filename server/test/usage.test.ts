// 用量: AI members' run costs by member, initiator, machine, runtime and day (owners only).
import { TeamUsage } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { buildUsage, priceRun } from "../src/usage";
import { PRICES, priceOf } from "../src/usage/prices";
import { Daemon, mention, world } from "./daemon";
import { Client, api } from "./helpers";

const DAY = 86_400_000;

describe("team usage", () => {
  it("sums ended runs by AI member, by who asked and by machine, with the member's budget", async () => {
    const w = await world();
    await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { weeklyBudgetCents: 10_000 } });
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    d.close();
    await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: [], machineId: w.machineId } });
    const { d: d2 } = await Daemon.connect(w.machineToken, { labels: [] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const assign = await d2.next((f) => f.t === "run.assign");
    d2.send({ t: "run.accept", runId: assign.runId });
    const done = d2.done(assign.runId, { status: "succeeded", usage: { inputTokens: 1000, outputTokens: 200, costUsd: 1.25 }, resultText: "ok" });
    await d2.acked(assign.runId, done);
    const from = Date.now() - DAY;
    let u = TeamUsage.parse((await api(`/v1/usage?from=${from}&to=${from + 2 * DAY}`, { token: w.owner.token })).body);
    for (let i = 0; i < 40 && u.total.runs === 0; i++) {
      await new Promise((r) => setTimeout(r, 100));
      u = TeamUsage.parse((await api(`/v1/usage?from=${from}&to=${from + 2 * DAY}`, { token: w.owner.token })).body);
    }
    expect(u.total).toMatchObject({ costUsd: 1.25, inputTokens: 1000, outputTokens: 200, runs: 1, topics: 1 });
    expect(u.bots).toEqual([expect.objectContaining({ memberId: w.bot.id, costUsd: 1.25, budgetCents: 10_000 })]);
    expect(u.initiators).toEqual([expect.objectContaining({ memberId: w.human.id, runs: 1 })]);
    expect(u.machines).toEqual([expect.objectContaining({ machineId: w.machineId, costUsd: 1.25 })]);
    expect(u.pairs).toEqual([expect.objectContaining({ initiatorId: w.human.id, memberId: w.bot.id, costUsd: 1.25, runs: 1 })]);
    expect(u.runtimes).toEqual([expect.objectContaining({ runtime: "codex", machineId: w.machineId })]);
    expect(u.daily).toHaveLength(2);
    expect(u.daily[0].bots).toEqual([{ memberId: w.bot.id, costUsd: 1.25 }]);
    expect(u.teamBudgetCents).toBeGreaterThanOrEqual(10_000);
    expect((await api(`/v1/usage?from=${from}&to=${from + 2 * DAY}`, { token: w.human.token })).status).toBe(403);
    expect((await api(`/v1/usage?from=${from}&to=${from + 90 * DAY}`, { token: w.owner.token })).status).toBe(400);
    c.close();
    d2.close();
  });

  it("compares with the span before and counts each topic once", () => {
    const run = (endedAt: number, cost: number, topicId: string) => ({
      botId: "b1",
      run: { endedAt, requestedBy: "p1", machineId: null, runtime: "claude", topicId, costUsd: cost, inputTokens: 1, outputTokens: 1 },
    });
    const u = buildUsage({
      from: 7 * DAY,
      to: 14 * DAY,
      runs: [run(8 * DAY, 1, "t1"), run(8 * DAY + 5, 2, "t1"), run(13 * DAY, 3, "t2"), run(2 * DAY, 4, "t0")],
      budgets: new Map([["b1", 5000]]),
    });
    expect(u.total).toEqual({ costUsd: 6, inputTokens: 3, outputTokens: 3, runs: 3, topics: 2 });
    expect(u.previous).toEqual({ costUsd: 4, topics: 1 });
    expect(u.daily.map((d) => d.costUsd)).toEqual([0, 3, 0, 0, 0, 0, 3]);
    expect(u.runtimes).toEqual([{ runtime: "claude", costUsd: 6, inputTokens: 3, outputTokens: 3, runs: 3, topics: 2 }]);
    expect(TeamUsage.safeParse(u).success).toBe(true);
  });

  it("prices runs that report no cost from their tokens and marks the estimate", () => {
    expect(priceOf("codex", null)).toEqual(PRICES["gpt-5-codex"]);
    expect(priceOf("codex", "gpt-5-mini-2025")).toEqual(PRICES["gpt-5-mini"]);
    expect(priceOf("claude", "claude-sonnet-4-5")).toEqual(PRICES["claude-sonnet-4"]);
    expect(priceOf("other", null)).toBeNull();
    const base = { endedAt: 8 * DAY, requestedBy: "p1", machineId: null, topicId: "t1" };
    const codex = { ...base, runtime: "codex", costUsd: 0, inputTokens: 1_000_000, outputTokens: 100_000 };
    expect(priceRun(codex, { runtime: "codex", model: null })).toMatchObject({ costUsd: 2.25, estimated: 2.25 });
    expect(priceRun({ ...codex, costUsd: 0.5 }, { runtime: "codex", model: null })).toMatchObject({ costUsd: 0.5, estimated: 0 });
    const u = buildUsage({
      from: 7 * DAY,
      to: 14 * DAY,
      runs: [
        { botId: "b1", run: codex },
        { botId: "b2", run: { ...base, runtime: "claude", costUsd: 1, inputTokens: 10, outputTokens: 10 } },
      ],
      budgets: new Map(),
      runners: new Map([["b1", { runtime: "codex", model: "gpt-5-mini" }]]),
    });
    expect(u.total).toMatchObject({ costUsd: 1.45, estimatedUsd: 0.45 });
    expect(u.bots.find((b) => b.memberId === "b1")).toMatchObject({ costUsd: 0.45, estimatedUsd: 0.45 });
    expect(u.bots.find((b) => b.memberId === "b2")?.estimatedUsd).toBeUndefined();
    expect(TeamUsage.safeParse(u).success).toBe(true);
  });
});
