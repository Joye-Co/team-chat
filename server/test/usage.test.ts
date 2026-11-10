// 用量: AI members' run costs by member, initiator, machine, runtime and day (owners only).
import { TeamUsage } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { buildUsage, priceRun } from "../src/usage";
import { FALLBACK, type PriceList, costOf, loadPrices, matchModel, parseModelsDev, resetPriceCache, syncPrices } from "../src/usage/prices";
import { env } from "cloudflare:test";
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
      prices: new Map(),
    });
    expect(u.total).toEqual({ costUsd: 6, inputTokens: 3, outputTokens: 3, runs: 3, topics: 2 });
    expect(u.previous).toEqual({ costUsd: 4, topics: 1 });
    expect(u.daily.map((d) => d.costUsd)).toEqual([0, 3, 0, 0, 0, 0, 3]);
    expect(u.runtimes).toEqual([{ runtime: "claude", costUsd: 6, inputTokens: 3, outputTokens: 3, runs: 3, topics: 2 }]);
    expect(TeamUsage.safeParse(u).success).toBe(true);
  });

  const LIST: PriceList = new Map([
    ["claude-opus-5-5", { name: "Claude Opus 5.5", input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 }],
    ["gpt-5.1-codex", { name: "GPT-5.1 Codex", input: 1.25, output: 10, cacheRead: 0.125 }],
  ]);
  const base = { endedAt: 8 * DAY, requestedBy: "p1", machineId: null, topicId: "t1" };

  it("matches the model a run used, by its own id only", () => {
    expect(matchModel(LIST, "claude-opus-5-5")?.id).toBe("claude-opus-5-5");
    expect(matchModel(LIST, "Claude-Opus-5-5-20261001")?.id).toBe("claude-opus-5-5");
    expect(matchModel(LIST, "openai/gpt-5.1-codex")?.id).toBe("gpt-5.1-codex");
    // Never another model's price: a longer id is a different model.
    expect(matchModel(LIST, "gpt-5.1-codex-max")).toBeNull();
    expect(matchModel(LIST, "claude-opus-5")).toBeNull();
  });

  it("prices each kind of token, cache reads and writes at their own price", () => {
    const p = LIST.get("claude-opus-5-5")!;
    // 1M input $5 + 1M output $25 + 2M cache reads $1 + 1M cache writes $6.25
    expect(costOf(p, { inputTokens: 1e6, outputTokens: 1e6, cacheReadTokens: 2e6, cacheWriteTokens: 1e6 })).toBeCloseTo(37.25);
    // No cache price: the model's own input price, never another model's.
    expect(costOf({ name: "x", input: 2, output: 4 }, { inputTokens: 0, outputTokens: 0, cacheReadTokens: 1e6 })).toBeCloseTo(2);
  });

  it("keeps a reported cost, estimates the rest and leaves unknown models unpriced", () => {
    const run = {
      ...base,
      runtime: "claude",
      costUsd: 0.4,
      inputTokens: 0,
      outputTokens: 0,
      models: [
        { model: "claude-opus-5-5", inputTokens: 100, outputTokens: 100, cacheReadTokens: 1000, costUsd: 0.3 },
        { model: "gpt-5.1-codex", inputTokens: 1_000_000, outputTokens: 0, cacheReadTokens: 1_000_000 },
        { model: "some-new-model", inputTokens: 500, outputTokens: 50 },
      ],
    };
    const p = priceRun(run, LIST);
    // 0.3 reported + 1.25 + 0.125 estimated; the unknown model adds tokens, not money.
    expect(p.costUsd).toBeCloseTo(1.675);
    expect(p.estimated).toBeCloseTo(1.375);
    expect(p.unpricedTokens).toBe(550);
    expect(p.priced.map((m) => [m.model, m.costUsd === null ? null : Math.round(m.costUsd * 1000) / 1000, m.estimated])).toEqual([
      ["claude-opus-5-5", 0.3, false],
      ["gpt-5.1-codex", 1.375, true],
      ["some-new-model", null, false],
    ]);
    // An older run with tokens only: priced by the member's set model, else unknown.
    const old = { ...base, runtime: "codex", costUsd: 0, inputTokens: 1_000_000, outputTokens: 0 };
    expect(priceRun(old, LIST, { runtime: "codex", model: "gpt-5.1-codex" })).toMatchObject({ costUsd: 1.25, estimated: 1.25 });
    expect(priceRun(old, LIST, { runtime: "codex", model: null })).toMatchObject({ costUsd: 0, unpricedTokens: 1_000_000 });
  });

  it("rolls up by model and lists each AI member's models", () => {
    const u = buildUsage({
      from: 7 * DAY,
      to: 14 * DAY,
      runs: [
        { botId: "b1", run: { ...base, runtime: "codex", costUsd: 0, inputTokens: 0, outputTokens: 0, models: [{ model: "gpt-5.1-codex", inputTokens: 1_000_000, outputTokens: 0 }] } },
        { botId: "b2", run: { ...base, runtime: "claude", costUsd: 1, inputTokens: 0, outputTokens: 0, models: [{ model: "claude-opus-5-5", inputTokens: 10, outputTokens: 10, costUsd: 1 }] } },
        { botId: "b2", run: { ...base, runtime: "claude", costUsd: 0, inputTokens: 0, outputTokens: 0, models: [{ model: "mystery-1", inputTokens: 40, outputTokens: 2 }] } },
      ],
      budgets: new Map(),
      prices: LIST,
    });
    expect(u.total).toMatchObject({ costUsd: 2.25, estimatedUsd: 1.25, unpricedTokens: 42 });
    expect(u.models?.map((m) => [m.model, m.name, m.runtimes, m.costUsd])).toEqual([
      ["gpt-5.1-codex", "GPT-5.1 Codex", ["codex"], 1.25],
      ["claude-opus-5-5", "Claude Opus 5.5", ["claude"], 1],
      ["mystery-1", "mystery-1", ["claude"], 0],
    ]);
    expect(u.models?.find((m) => m.model === "mystery-1")?.unpricedTokens).toBe(42);
    expect(u.bots.find((b) => b.memberId === "b2")?.models).toEqual(["claude-opus-5-5", "mystery-1"]);
    expect(TeamUsage.safeParse(u).success).toBe(true);
  });

  it("syncs prices from models.dev, first-party entries first, and keeps the last good copy", async () => {
    const api = {
      openrouter: { models: { "anthropic/claude-opus-5-5": { id: "anthropic/claude-opus-5-5", name: "Opus via router", cost: { input: 9, output: 9 } } } },
      anthropic: { models: { "claude-opus-5-5": { id: "claude-opus-5-5", name: "Claude Opus 5.5", cost: { input: 5, output: 25, cache_read: 0.5, cache_write: 6.25 } } } },
      openai: {
        models: Object.fromEntries(
          Array.from({ length: 12 }, (_, i) => [`gpt-x-${i}`, { id: `gpt-x-${i}`, name: `GPT X ${i}`, cost: { input: 1, output: 2 } }]),
        ),
      },
      nocost: { models: { free: { id: "free", name: "Free" } } },
    };
    const parsed = parseModelsDev(api);
    expect(parsed.get("claude-opus-5-5")).toEqual({ name: "Claude Opus 5.5", input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 });
    expect(parsed.has("free")).toBe(false);
    resetPriceCache();
    expect((await loadPrices(env)).get("claude-opus-4-5")).toEqual(FALLBACK["claude-opus-4-5"]);
    const ok = (async () => new Response(JSON.stringify(api))) as unknown as typeof fetch;
    expect(await syncPrices(env, ok)).toBe(13);
    resetPriceCache();
    expect((await loadPrices(env)).get("claude-opus-5-5")?.input).toBe(5);
    const down = (async () => new Response("no", { status: 503 })) as unknown as typeof fetch;
    await expect(syncPrices(env, down)).rejects.toThrow();
    resetPriceCache();
    expect((await loadPrices(env)).size).toBe(13);
  });
});
