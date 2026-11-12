// 用量 of 团队管理 (docs/protocol.md "Team usage"): what AI members' runs cost in a span, by AI
// member, by the person who asked, by machine and by runtime, per day, against the members' weekly
// budgets (`weeklyBudgetCents`). Read from every runner bot's BotDesk; nothing is stored.
import { type TeamUsage, UsageQuery } from "@chat/protocol";
import { serviceUsage } from "../aiservice/uses";
import { deskStub } from "../bots/env";
import type { UsageRun } from "../bots/desk/external";
import { ApiError } from "../core/errors";
import { registerRoute, registerScheduled } from "../core/registry";
import { type PriceList, type Priced, loadPrices, priceRun, syncPrices } from "./prices";

export { priceRun } from "./prices";

const DAY_MS = 86_400_000;

type Spend = TeamUsage["total"];
const zero = (): Spend => ({ costUsd: 0, inputTokens: 0, outputTokens: 0, runs: 0, topics: 0 });

/** What a tally adds up: a whole run, or one model's share of it. */
interface Piece {
  costUsd: number;
  estimated: number;
  unpricedTokens: number;
  inputTokens: number;
  outputTokens: number;
  topicId: string;
}

/** Adds runs to a spend; topics are counted once per spend. */
class Tally {
  private readonly spend = new Map<string, Spend & { topicSet: Set<string>; est: number; unpriced: number }>();
  add(key: string, r: Piece): void {
    let s = this.spend.get(key);
    if (!s) {
      s = { ...zero(), topicSet: new Set(), est: 0, unpriced: 0 };
      this.spend.set(key, s);
    }
    s.costUsd += r.costUsd;
    s.est += r.estimated;
    s.unpriced += r.unpricedTokens;
    s.inputTokens += r.inputTokens;
    s.outputTokens += r.outputTokens;
    s.runs += 1;
    s.topicSet.add(r.topicId);
  }
  keys(): string[] {
    return [...this.spend.keys()];
  }
  entries(): [string, Spend][] {
    return [...this.spend.entries()]
      .map(([k, s]): [string, Spend] => [
        k,
        {
          costUsd: round(s.costUsd),
          inputTokens: s.inputTokens,
          outputTokens: s.outputTokens,
          runs: s.runs,
          topics: s.topicSet.size,
          ...(s.est > 0 ? { estimatedUsd: round(s.est) } : {}),
          ...(s.unpriced > 0 ? { unpricedTokens: s.unpriced } : {}),
        },
      ])
      .sort((a, b) => b[1].costUsd - a[1].costUsd || b[1].runs - a[1].runs || a[0].localeCompare(b[0]));
  }
}

const round = (x: number) => Math.round(x * 100) / 100;

export function buildUsage(input: {
  from: number;
  to: number;
  runs: { botId: string; run: UsageRun }[];
  budgets: Map<string, number>;
  runners?: Map<string, { runtime: string | null; model: string | null }>;
  prices: PriceList;
}): TeamUsage {
  const { from, to } = input;
  const span = to - from;
  const priced = input.runs.map((x) => ({ botId: x.botId, run: priceRun(x.run, input.prices, input.runners?.get(x.botId)) }));
  const inSpan = priced.filter((x) => x.run.endedAt >= from && x.run.endedAt < to);
  const before = priced.filter((x) => x.run.endedAt >= from - span && x.run.endedAt < from);
  const total = new Tally();
  const bots = new Tally();
  const people = new Tally();
  const machines = new Tally();
  const runtimes = new Tally();
  const pairs = new Tally();
  /** By model: each model's share of a run (its cost, tokens and whether it is priced). */
  const models = new Tally();
  const modelNames = new Map<string, string>();
  const modelRuntimes = new Map<string, Set<string>>();
  /** Per AI member: the models it used, with what each cost. */
  const botModels = new Map<string, Map<string, number>>();
  const days = Math.max(1, Math.ceil(span / DAY_MS));
  const daily = Array.from({ length: Math.min(days, 62) }, (_, i) => ({ start: from + i * DAY_MS, bots: new Map<string, number>() }));
  for (const { botId, run } of inSpan) {
    total.add("all", run);
    bots.add(botId, run);
    people.add(run.requestedBy, run);
    pairs.add(`${run.requestedBy}\u0000${botId}`, run);
    if (run.machineId) machines.add(run.machineId, run);
    runtimes.add(`${run.runtime ?? "unknown"}\u0000${run.machineId ?? ""}`, run);
    const d = daily[Math.floor((run.endedAt - from) / DAY_MS)];
    if (d) d.bots.set(botId, (d.bots.get(botId) ?? 0) + run.costUsd);
    for (const m of run.priced) {
      models.add(m.model, {
        costUsd: m.costUsd ?? 0,
        estimated: m.estimated ? (m.costUsd ?? 0) : 0,
        unpricedTokens: m.costUsd === null ? m.inputTokens + m.outputTokens : 0,
        inputTokens: m.inputTokens,
        outputTokens: m.outputTokens,
        topicId: run.topicId,
      });
      modelNames.set(m.model, m.name);
      if (run.runtime) modelRuntimes.set(m.model, (modelRuntimes.get(m.model) ?? new Set()).add(run.runtime));
      const used = botModels.get(botId) ?? new Map<string, number>();
      used.set(m.model, (used.get(m.model) ?? 0) + (m.costUsd ?? 0) + 1e-9);
      botModels.set(botId, used);
    }
  }
  const prevTopics = new Set(before.map((x) => x.run.topicId));
  const budgets = [...input.budgets.values()];
  const out: TeamUsage = {
    from,
    to,
    total: total.entries()[0]?.[1] ?? zero(),
    previous: { costUsd: round(before.reduce((s, x) => s + x.run.costUsd, 0)), topics: prevTopics.size },
    bots: bots.entries().map(([memberId, s]) => {
      const used = [...(botModels.get(memberId) ?? new Map<string, number>()).entries()].sort((a, b) => b[1] - a[1]).map(([m]) => m);
      return {
        memberId,
        ...s,
        ...(input.budgets.has(memberId) ? { budgetCents: input.budgets.get(memberId) } : {}),
        ...(used.length > 0 ? { models: used.slice(0, 10) } : {}),
      };
    }),
    initiators: people.entries().map(([memberId, s]) => ({ memberId, ...s })),
    machines: machines.entries().map(([machineId, s]) => ({ machineId, ...s })),
    pairs: pairs.entries().map(([key, s]) => {
      const [initiatorId, memberId] = key.split("\u0000");
      return { initiatorId, memberId, ...s };
    }),
    models: models.entries().map(([model, s]) => ({
      model,
      name: modelNames.get(model) ?? model,
      runtimes: [...(modelRuntimes.get(model) ?? [])].sort(),
      ...s,
    })),
    runtimes: runtimes.entries().map(([key, s]) => {
      const [runtime, machineId] = key.split("\u0000");
      return { runtime, ...(machineId ? { machineId } : {}), ...s };
    }),
    daily: daily.map((d) => ({
      start: d.start,
      costUsd: round([...d.bots.values()].reduce((a, b) => a + b, 0)),
      bots: [...d.bots.entries()].map(([memberId, c]) => ({ memberId, costUsd: round(c) })).sort((a, b) => b.costUsd - a.costUsd),
    })),
  };
  if (budgets.length > 0) out.teamBudgetCents = budgets.reduce((a, b) => a + b, 0);
  return out;
}

registerRoute({
  method: "GET",
  path: "/v1/usage",
  auth: "owner",
  async handler({ env, url, ctx }) {
    const q = UsageQuery.safeParse(Object.fromEntries(url.searchParams));
    if (!q.success) throw new ApiError("invalid", "from and to are times in ms, from < to, at most 62 days apart");
    const { from, to } = q.data;
    const bots = await env.DB.prepare(
      "SELECT m.id AS id, m.weekly_budget_cents AS budget, r.runtime AS runtime, r.model AS model FROM bot_runners r JOIN members m ON m.id = r.member_id ORDER BY m.id",
    ).all<{ id: string; budget: number | null; runtime: string | null; model: string | null }>();
    const budgets = new Map<string, number>();
    const runners = new Map<string, { runtime: string | null; model: string | null }>();
    for (const b of bots.results) {
      if (typeof b.budget === "number" && b.budget >= 0) budgets.set(b.id, b.budget);
      runners.set(b.id, { runtime: b.runtime, model: b.model });
    }
    const runs = (
      await Promise.all(
        bots.results.map(async (b) => {
          try {
            return (await deskStub(env, b.id).usageRuns(b.id, from - (to - from), to)).map((run) => ({ botId: b.id, run }));
          } catch {
            return [];
          }
        }),
      )
    ).flat();
    const prices = await loadPrices(env, (p) => ctx.waitUntil(p));
    const out = buildUsage({ from, to, runs, budgets, runners, prices });
    // 模 1.9: the AI 服务's own calls, one line apart from the runs.
    const service = await serviceUsage(env.DB, from, to, prices).catch(() => undefined);
    if (service) out.service = service;
    // 到了预算: how many runs of each AI member with a budget wait for an owner.
    await Promise.all(
      out.bots
        .filter((b) => b.budgetCents !== undefined)
        .map(async (b) => {
          const n = await deskStub(env, b.memberId)
            .budgetHeld(b.memberId)
            .catch(() => 0);
          if (n > 0) b.heldRuns = n;
        }),
    );
    return out;
  },
});

// Per-model prices from models.dev, once a day (the 03:00 UTC cron, ARCHIVE_CRON; the other cron
// is hourly); a failure keeps the last copy.
registerScheduled("0 3 * * *", async ({ env, log }) => {
  try {
    const n = await syncPrices(env);
    log.info("model prices synced", { models: n });
  } catch (err) {
    log.warn("model price sync failed", { err });
  }
});
