// 活动记录 events the server notices on its own (docs/protocol.md "Activity"): a task that did not
// finish, a machine that went offline and came back, and an AI member reaching its weekly budget.
// Unlike the admin events, which the audit trigger copies, these are written here; `audit_id` is 0.
import type { UsageRun } from "../bots/desk/external";
import type { Env } from "../core/env";
import { teamZone } from "../memory/consolidate";
import { loadPrices, priceRun } from "../usage/prices";

/** A machine counts as offline in 活动记录 only after this long without a connection. */
export const OFFLINE_GRACE_MS = 5 * 60_000;

export interface ActivityEvent {
  at: number;
  kind: "run" | "machine" | "budget";
  action: "run.failed" | "machine.offline" | "machine.online" | "budget.reached";
  actorId?: string | null;
  target?: string | null;
  machineId?: string | null;
  details?: Record<string, unknown>;
}

export async function recordActivity(db: D1Database, e: ActivityEvent): Promise<void> {
  await db
    .prepare("INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details) VALUES (0, ?, ?, ?, ?, ?, ?, ?)")
    .bind(e.at, e.kind, e.action, e.actorId ?? null, e.target ?? null, e.machineId ?? null, JSON.stringify(e.details ?? {}))
    .run();
}

/** Monday 00:00 of the week `now` is in, in `zone`, as epoch ms (用量 counts weeks the same way). */
export function weekStart(now: number, zone: string): number {
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat("en-CA", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", weekday: "short", hourCycle: "h23" })
      .formatToParts(new Date(now))
      .map((p) => [p.type, p.value]),
  ) as Record<string, string>;
  const [y, mo, d, h, mi, s] = [parts.year, parts.month, parts.day, parts.hour, parts.minute, parts.second].map(Number);
  // The zone's offset at `now`: local wall time read as UTC, minus the instant.
  const offset = Date.UTC(y, mo - 1, d, h % 24, mi, s) - Math.floor(now / 1000) * 1000;
  const sinceMonday = (["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].indexOf(parts.weekday) + 7) % 7;
  return Date.UTC(y, mo - 1, d - sinceMonday) - offset;
}

/**
 * After a run of an AI member ended: 没跑成 when it failed, and 到了预算 the first time this week's
 * spend crosses the member's weekly budget. Never throws.
 */
export async function activityAfterRun(
  env: Env,
  input: {
    botId: string;
    runId: string;
    status: string;
    requestedBy: string;
    machineId: string | null;
    groupId: string;
    topicId: string;
    title: () => Promise<string>;
    /** This bot's ended runs since a time, for this week's spend. */
    runsSince: (from: number) => Promise<UsageRun[]>;
    now?: number;
  },
): Promise<void> {
  const now = input.now ?? Date.now();
  if (input.status === "failed") {
    const title = await input.title().catch(() => "");
    await recordActivity(env.DB, {
      at: now,
      kind: "run",
      action: "run.failed",
      actorId: input.botId,
      target: input.topicId,
      machineId: input.machineId,
      details: { runId: input.runId, groupId: input.groupId, requestedBy: input.requestedBy, ...(title ? { title: title.slice(0, 200) } : {}) },
    });
  }
  const row = await env.DB.prepare(
    "SELECT m.weekly_budget_cents AS budget, r.runtime AS runtime, r.model AS model FROM members m LEFT JOIN bot_runners r ON r.member_id = m.id WHERE m.id = ?",
  )
    .bind(input.botId)
    .first<{ budget: number | null; runtime: string | null; model: string | null }>();
  const budget = row?.budget;
  if (typeof budget !== "number" || budget <= 0) return;
  const from = weekStart(now, teamZone(env));
  const runs = await input.runsSince(from);
  const runner = { runtime: row?.runtime ?? null, model: row?.model ?? null };
  const prices = await loadPrices(env);
  const priced = runs.map((r) => priceRun(r, prices, runner));
  const spent = priced.reduce((s, r) => s + r.costUsd, 0) * 100;
  if (spent < budget) return;
  const seen = await env.DB.prepare("SELECT 1 AS x FROM activity WHERE action = 'budget.reached' AND target = ? AND at >= ? LIMIT 1")
    .bind(input.botId, from)
    .first();
  if (seen) return;
  await recordActivity(env.DB, {
    at: now,
    kind: "budget",
    action: "budget.reached",
    actorId: input.botId,
    target: input.botId,
    details: { budgetCents: budget, spentCents: Math.round(spent) },
  });
}
