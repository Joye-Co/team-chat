// 用量 of 团队管理 (docs/protocol.md "Team usage"): what AI members' runs cost in a span, by AI
// member, by the person who asked, by machine and by runtime, per day, against the members' weekly
// budgets (`weeklyBudgetCents`). Read from every runner bot's BotDesk; nothing is stored.
import { type TeamUsage, UsageQuery } from "@chat/protocol";
import { deskStub } from "../bots/env";
import type { UsageRun } from "../bots/desk/external";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { type Priced, priceRun } from "./prices";

export { priceRun } from "./prices";

const DAY_MS = 86_400_000;

type Spend = TeamUsage["total"];
const zero = (): Spend => ({ costUsd: 0, inputTokens: 0, outputTokens: 0, runs: 0, topics: 0 });

/** Adds runs to a spend; topics are counted once per spend. */
class Tally {
  private readonly spend = new Map<string, Spend & { topicSet: Set<string>; est: number }>();
  add(key: string, r: Priced): void {
    let s = this.spend.get(key);
    if (!s) {
      s = { ...zero(), topicSet: new Set(), est: 0 };
      this.spend.set(key, s);
    }
    s.costUsd += r.costUsd;
    s.est += r.estimated;
    s.inputTokens += r.inputTokens;
    s.outputTokens += r.outputTokens;
    s.runs += 1;
    s.topicSet.add(r.topicId);
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
}): TeamUsage {
  const { from, to } = input;
  const span = to - from;
  const priced = input.runs.map((x) => ({ botId: x.botId, run: priceRun(x.run, input.runners?.get(x.botId)) }));
  const inSpan = priced.filter((x) => x.run.endedAt >= from && x.run.endedAt < to);
  const before = priced.filter((x) => x.run.endedAt >= from - span && x.run.endedAt < from);
  const total = new Tally();
  const bots = new Tally();
  const people = new Tally();
  const machines = new Tally();
  const runtimes = new Tally();
  const pairs = new Tally();
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
  }
  const prevTopics = new Set(before.map((x) => x.run.topicId));
  const budgets = [...input.budgets.values()];
  const out: TeamUsage = {
    from,
    to,
    total: total.entries()[0]?.[1] ?? zero(),
    previous: { costUsd: round(before.reduce((s, x) => s + x.run.costUsd, 0)), topics: prevTopics.size },
    bots: bots.entries().map(([memberId, s]) => ({
      memberId,
      ...s,
      ...(input.budgets.has(memberId) ? { budgetCents: input.budgets.get(memberId) } : {}),
    })),
    initiators: people.entries().map(([memberId, s]) => ({ memberId, ...s })),
    machines: machines.entries().map(([machineId, s]) => ({ machineId, ...s })),
    pairs: pairs.entries().map(([key, s]) => {
      const [initiatorId, memberId] = key.split("\u0000");
      return { initiatorId, memberId, ...s };
    }),
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
  async handler({ env, url }) {
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
    return buildUsage({ from, to, runs, budgets, runners });
  },
});
