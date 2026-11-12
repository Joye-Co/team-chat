// 到了预算先停下来问: an AI member at or over its weekly budget starts no new run. The run is held
// (status waiting, no machine) and the owners get a card in the topic: 这周再加 $X, 改每周预算…
// (the app opens 用量's editor) or 先停. A run already on a machine finishes. No budget, no limit.
// This week = Monday 00:00 in the team's time zone; spend is priced per model like 用量.
import type { UsageRun } from "../bots/desk/external";
import { deskStub } from "../bots/env";
import { weekStart } from "../activity/events";
import type { Env } from "../core/env";
import { registerMembersChanged } from "../core/registry";
import { teamZone } from "../memory/consolidate";
import { loadPrices, priceRun } from "../usage/prices";
import { type LoginMethodsOf, machineLoginMethods, paidRuns } from "./paid";

export interface BudgetState {
  weekStart: number;
  budgetCents: number;
  /** Added for this week only (这周再加). */
  extraCents: number;
  spentCents: number;
}

/** 这周再加: half the weekly budget, rounded up to whole dollars, at least $1. */
export function raiseCents(budgetCents: number): number {
  return Math.max(100, Math.ceil(budgetCents / 2 / 100) * 100);
}

export function overBudget(s: BudgetState): boolean {
  return s.spentCents >= s.budgetCents + s.extraCents;
}

export { type LoginMethodsOf, paidRuns } from "./paid";

/** The AI member's budget and spend this week, or null when it has no budget (no limit). */
export async function budgetState(
  env: Env,
  botId: string,
  runsSince: (from: number) => Promise<UsageRun[]>,
  now = Date.now(),
  methodsOf: LoginMethodsOf = machineLoginMethods(env),
): Promise<BudgetState | null> {
  const row = await env.DB.prepare(
    "SELECT m.weekly_budget_cents AS budget, r.runtime AS runtime, r.model AS model FROM members m LEFT JOIN bot_runners r ON r.member_id = m.id WHERE m.id = ?",
  )
    .bind(botId)
    .first<{ budget: number | null; runtime: string | null; model: string | null }>();
  if (typeof row?.budget !== "number") return null;
  const from = weekStart(now, teamZone(env));
  const extra = await env.DB.prepare("SELECT coalesce(sum(extra_cents), 0) AS cents FROM budget_grants WHERE member_id = ? AND week_start = ?")
    .bind(botId, from)
    .first<{ cents: number }>();
  const prices = await loadPrices(env);
  const runner = { runtime: row.runtime ?? null, model: row.model ?? null };
  const spent = (await paidRuns(await runsSince(from), methodsOf)).map((r) => priceRun(r, prices, runner)).reduce((s, r) => s + r.costUsd, 0);
  return { weekStart: from, budgetCents: row.budget, extraCents: extra?.cents ?? 0, spentCents: Math.round(spent * 100) };
}

/** 这周再加: adds to this week only. */
export async function grantExtra(env: Env, botId: string, weekStartAt: number, cents: number, by: string): Promise<void> {
  await env.DB.prepare("INSERT INTO budget_grants (member_id, week_start, extra_cents, granted_by, at) VALUES (?, ?, ?, ?, ?)")
    .bind(botId, weekStartAt, cents, by, Date.now())
    .run();
}

// A changed weekly budget (改每周预算… in 用量) lets held runs go when it now covers the spend.
registerMembersChanged(async (env, ids) => {
  for (const id of ids) {
    const bot = await env.DB.prepare("SELECT 1 AS x FROM bot_runners WHERE member_id = ?").bind(id).first();
    if (bot) await deskStub(env, id).budgetRecheck(id);
  }
});
