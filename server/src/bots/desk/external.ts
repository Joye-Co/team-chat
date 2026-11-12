// BotDesk: one per runner-backed bot member, the bot's front desk. It never runs commands and
// has no model: fixed rules turn a mention into a run on a machine, run events into topic
// messages and topic status, and later topic messages into run input. The normalized run log
// lives in its SQLite. This class adds runs of external bots, plans over HTTP and reads to the
// feature modules it extends (core, runs, events, stall, input, takeover, interact, github).
import { type BotRun, type Card, type ModelUsage, RUN_KIND_ASIDE, type RunPlan, type RunStatus, type TopicStatus } from "@chat/protocol";
import { activityAfterRun, recordActivity } from "../../activity/events";
import { type BudgetState, budgetState, grantExtra, overBudget, raiseCents } from "../../budget";
import { roomStub } from "../../core/env";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TokenBucket } from "../../core/ratelimit";
import { cut } from "../dispatch";
import { DEFAULT_PAGE, MAX_PAGE, PLAN_RATE, type RunRow, result, sanitizeUsage } from "./core";
import type { CardPress } from "../room";

/** One ended run for 用量. */
export interface UsageRun {
  endedAt: number;
  requestedBy: string;
  machineId: string | null;
  runtime: string | null;
  topicId: string;
  costUsd: number;
  inputTokens: number;
  outputTokens: number;
  /** Per model the run used (daemons from 1.3). */
  models?: ModelUsage[];
}
import { DeskGithub } from "./github";
import type { RunViewer } from "./hand";
import { type DeskRollup, deskRollup } from "../../monitor/rollup";

/** A run held at the budget (meta `budget_hold:{runId}`): the state when it was held. */
interface Hold extends BudgetState {
  at: number;
  raiseCents: number;
}

const usd = (cents: number) => `$${Number.isInteger(cents / 100) ? cents / 100 : (cents / 100).toFixed(2)}`;

/** The words of 到了预算 (budget/index.ts). */
export const BUDGET_TEXT = {
  title: (bot: string, spent: number) => `${bot} 这周花了 ${usd(spent)}，到了预算。继续吗？`,
  body: (bot: string, spent: number, budget: number, extra: number) =>
    `${bot} 这周花了 ${usd(spent)}，每周预算 ${usd(budget)}${extra > 0 ? `（这周已加 ${usd(extra)}）` : ""}。这件事先停下来等你决定，还没开始做。`,
  raise: (cents: number) => `这周再加 ${usd(cents)}`,
  edit: "改每周预算…",
  stop: "先停",
  raised: (owner: string, bot: string, cents: number) => `${owner} 这周给 ${bot} 加了 ${usd(cents)} 预算，接着做。`,
  stopped: (owner: string, bot: string) => `这周的预算用完了，${owner} 让 ${bot} 先停下，这件事没有做。要继续，在 团队管理 › 用量 里改预算后再 @${bot}。`,
};

export class BotDesk extends DeskGithub {
  // -------------------------------------------------------------------------
  // 到了预算先停下来问 (budget/index.ts)
  // -------------------------------------------------------------------------

  /** Holds a run that would start at or over the AI member's budget and asks the owners. */
  protected override async budgetGate(r: RunRow, prompt: string): Promise<boolean> {
    const botId = this.botId;
    if (!botId || r.external || r.kind === RUN_KIND_ASIDE) return false;
    const state = await budgetState(this.env, botId, (from) => this.usageRuns(botId, from, Number.MAX_SAFE_INTEGER));
    if (!state || !overBudget(state)) return false;
    const hold: Hold = { ...state, at: Date.now(), raiseCents: raiseCents(state.budgetCents) };
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `budget_hold:${r.id}`, JSON.stringify(hold));
    this.sql.exec("UPDATE runs SET queued_at = NULL WHERE id = ?", r.id);
    r.queued_at = null;
    const names = await this.names([botId]);
    const bot = names.get(botId) ?? "AI 成员";
    const card: Card = {
      v: 1,
      title: cut(BUDGET_TEXT.title(bot, state.spentCents), 200),
      tone: "warning",
      blocks: [],
      actions: [
        { id: `budget-raise:${r.id}`, label: BUDGET_TEXT.raise(hold.raiseCents), style: "primary" },
        { id: `budget-edit:${r.id}`, label: BUDGET_TEXT.edit, value: { memberId: botId } },
        { id: `budget-stop:${r.id}`, label: BUDGET_TEXT.stop, style: "danger" },
      ],
    };
    const owners = await this.owners(r.group_id);
    const body = BUDGET_TEXT.body(bot, state.spentCents, state.budgetCents, state.extraCents);
    try {
      await this.post(r, `run:${r.id}:budget`, body, { card, mentions: owners });
    } catch (err) {
      this.log.warn("budget card failed", { runId: r.id, err });
      await this.post(r, `run:${r.id}:budget:text`, body, { mentions: owners }).catch(() => "");
    }
    await this.setStatus(r, "waiting");
    await this.topic(r, "waiting_answer", owners);
    const title = await this.topicTitle(r).catch(() => "");
    await recordActivity(this.env.DB, {
      at: hold.at,
      kind: "budget",
      action: "budget.paused",
      actorId: botId,
      target: botId,
      details: {
        runId: r.id,
        topicId: r.topic_id,
        budgetCents: state.budgetCents + state.extraCents,
        spentCents: state.spentCents,
        ...(title ? { title: cut(title, 200) } : {}),
      },
    }).catch((err) => this.log.warn("activity paused failed", { err }));
    return true;
  }

  /** The team's active human owners in the run's group, who decide at the budget. */
  protected async owners(groupId: string): Promise<string[]> {
    const rows = await this.env.DB.prepare(
      `SELECT m.id FROM members m JOIN group_members g ON g.member_id = m.id AND g.group_id = ?
       WHERE m.level = 'owner' AND m.kind = 'human' AND m.deactivated = 0 ORDER BY m.id LIMIT 20`,
    )
      .bind(groupId)
      .all<{ id: string }>()
      .catch(() => ({ results: [] as { id: string }[] }));
    return rows.results.map((x) => x.id);
  }

  /** The held runs of this AI member, oldest first. */
  protected heldRuns(): { run: RunRow; hold: Hold }[] {
    const rows = this.sql.exec<{ key: string; value: string }>("SELECT key, value FROM meta WHERE key LIKE 'budget_hold:%'").toArray();
    const out: { run: RunRow; hold: Hold }[] = [];
    for (const row of rows) {
      const run = this.run(row.key.slice("budget_hold:".length));
      if (!run || run.ended_at !== null) {
        this.sql.exec("DELETE FROM meta WHERE key = ?", row.key);
        continue;
      }
      out.push({ run, hold: JSON.parse(row.value) as Hold });
    }
    return out.sort((a, b) => a.hold.at - b.hold.at);
  }

  /** Lets held runs go: back in the queue, where the budget is checked again before they start. */
  protected async releaseHeld(note?: string): Promise<void> {
    for (const { run } of this.heldRuns()) {
      await this.serial(run.id, async () => {
        const r = this.run(run.id);
        if (!r || r.ended_at !== null) return;
        this.sql.exec("DELETE FROM meta WHERE key = ?", `budget_hold:${r.id}`);
        const now = Date.now();
        this.sql.exec("UPDATE runs SET queued_at = ? WHERE id = ?", now, r.id);
        r.queued_at = now;
        if (note) await this.post(r, `run:${r.id}:budget:go:${now}`, note).catch(() => "");
        await this.setStatus(r, "created");
        await this.topic(r, "in_progress");
      });
    }
    await this.dispatchQueued();
  }

  /** How many runs wait at the budget (用量 › 要注意). */
  async budgetHeld(botId: string): Promise<number> {
    this.bind(botId);
    return this.heldRuns().length;
  }

  /** The weekly budget changed: held runs go when it now covers this week's spend. */
  async budgetRecheck(botId: string): Promise<void> {
    this.bind(botId);
    if (this.heldRuns().length === 0) return;
    const state = await budgetState(this.env, botId, (from) => this.usageRuns(botId, from, Number.MAX_SAFE_INTEGER));
    if (!state || !overBudget(state)) await this.releaseHeld();
  }

  /** 这周再加 and 先停 on the budget card; 改每周预算… is the app's (it opens 用量). Owners only. */
  override async cardAction(botId: string, a: CardPress): Promise<void> {
    const m = /^budget-(raise|stop):(.+)$/.exec(a.actionId);
    if (!m) {
      if (a.actionId.startsWith("budget-")) return;
      return super.cardAction(botId, a);
    }
    this.bind(botId);
    if (a.actorLevel !== "owner" || a.actorKind !== "human") {
      this.log.info("budget press ignored", { actorId: a.actorId });
      return;
    }
    const held = this.heldRuns().find((x) => x.run.id === m[2]);
    if (!held) return;
    const names = await this.names([botId, a.actorId]);
    const bot = names.get(botId) ?? "AI 成员";
    const owner = names.get(a.actorId) ?? "负责人";
    if (m[1] === "raise") {
      await grantExtra(this.env, botId, held.hold.weekStart, held.hold.raiseCents, a.actorId);
      await recordActivity(this.env.DB, {
        at: Date.now(),
        kind: "budget",
        action: "budget.raised",
        actorId: a.actorId,
        target: botId,
        details: { extraCents: held.hold.raiseCents, runId: held.run.id },
      }).catch((err) => this.log.warn("activity raised failed", { err }));
      await this.releaseHeld(BUDGET_TEXT.raised(owner, bot, held.hold.raiseCents));
      return;
    }
    const r = held.run;
    await this.serial(r.id, async () => {
      this.sql.exec("DELETE FROM meta WHERE key IN (?, ?)", `budget_hold:${r.id}`, `prompt:${r.id}`);
      const text = BUDGET_TEXT.stopped(owner, bot);
      await this.end(r, "cancelled", text, "open", text);
    });
    const title = await this.topicTitle(r).catch(() => "");
    await recordActivity(this.env.DB, {
      at: Date.now(),
      kind: "budget",
      action: "budget.stopped",
      actorId: a.actorId,
      target: botId,
      details: { runId: r.id, topicId: r.topic_id, ...(title ? { title: cut(title, 200) } : {}) },
    }).catch((err) => this.log.warn("activity stopped failed", { err }));
  }

  /** After a run ended: 活动记录's 没跑成 and 到了预算 (activity/events.ts), off the run's turn. */
  protected override async end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string, success = false, waitingOn: string[] = []): Promise<void> {
    await super.end(r, status, body, topicStatus, summary, success, waitingOn);
    const botId = this.botId;
    if (r.external || !botId) return;
    this.ctx.waitUntil(
      activityAfterRun(this.env, {
        botId,
        runId: r.id,
        status,
        requestedBy: r.requested_by,
        machineId: r.machine_id,
        groupId: r.group_id,
        topicId: r.topic_id,
        title: () => this.topicTitle(r),
        runsSince: async (from) => this.usageRuns(botId, from, Number.MAX_SAFE_INTEGER),
      }).catch((err) => this.log.warn("activity after run failed", { runId: r.id, err })),
    );
  }

  // -------------------------------------------------------------------------
  // Runs of external bots and plans over HTTP (docs/protocol.md, "Run plans")
  // -------------------------------------------------------------------------

  /**
   * Starts a run of this (external) bot in the topic, or returns its active run there. The API
   * Worker checked that the caller is this bot, not runner-backed, and a member of the group.
   */
  async startExternal(botId: string, groupId: string, topicId: string, plan?: RunPlan): Promise<Result<BotRun>> {
    return result(() => this.doStartExternal(botId, groupId, topicId, plan));
  }

  protected async doStartExternal(botId: string, groupId: string, topicId: string, plan?: RunPlan): Promise<BotRun> {
    this.bind(botId);
    const active = this.activeRun(topicId);
    if (active) {
      if (plan) return this.doSetPlan(botId, active.id, plan);
      return this.toRun(active);
    }
    // The topic must exist in the group (the room also checks membership).
    unwrap(await roomStub(this.env, groupId).rpc(groupId, "bots.topicExists", this.actor(), { topicId }));
    const now = Date.now();
    const runId = ulid(now);
    return this.serial(runId, async () => {
      if (this.activeRun(topicId)) {
        const again = this.activeRun(topicId) as RunRow;
        return this.toRun(again);
      }
      this.sql.exec(
        `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, started_at, updated_at, last_event_at, external, plan)
         VALUES (?, ?, ?, 'running', '', '', ?, ?, ?, ?, 1, ?)`,
        runId,
        groupId,
        topicId,
        now,
        now,
        now,
        now,
        plan && plan.length > 0 ? JSON.stringify(plan) : null,
      );
      await this.env.DB.prepare(
        "INSERT INTO bot_runs (id, bot_id, group_id, topic_id, machine_id, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
      )
        .bind(runId, botId, groupId, topicId, now)
        .run();
      const r = this.run(runId) as RunRow;
      await this.setStatus(r, "running");
      return this.toRun(r);
    });
  }

  /** Replaces the plan of an active run of this bot (HTTP; the caller is the bot). */
  async setPlan(botId: string, runId: string, plan: RunPlan): Promise<Result<BotRun>> {
    return result(() => this.doSetPlan(botId, runId, plan));
  }

  protected async doSetPlan(botId: string, runId: string, plan: RunPlan): Promise<BotRun> {
    this.bind(botId);
    let bucket = this.planBuckets.get(runId);
    if (!bucket) {
      bucket = new TokenBucket(PLAN_RATE.rate, PLAN_RATE.burst);
      this.planBuckets.set(runId, bucket);
    }
    if (!bucket.take()) throw new ApiError("rate_limited", "too many plan updates for this run");
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "run not found");
      if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
      if (this.storePlan(r, plan)) await this.setStatus(r, r.status);
      return this.toRun(r);
    });
  }

  /** Ends a run that this external bot started. */
  async endExternal(botId: string, runId: string, status: "done" | "failed" | "cancelled", summary?: string): Promise<Result<BotRun>> {
    return result(() => this.doEndExternal(botId, runId, status, summary));
  }

  protected async doEndExternal(botId: string, runId: string, status: "done" | "failed" | "cancelled", summary?: string): Promise<BotRun> {
    this.bind(botId);
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "run not found");
      if (!r.external) throw new ApiError("forbidden", "runs of a runner end on the machine");
      if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
      this.planBuckets.delete(runId);
      await this.setStatus(r, status, { ended_at: Date.now(), summary: summary ? cut(summary, 280) : r.summary });
      return this.toRun(r);
    });
  }

  // -------------------------------------------------------------------------
  // Reads (API Worker)
  // -------------------------------------------------------------------------

  /** This bot's share of a machine for the owner's monitor (monitor/rollup.ts), with the title of its last failure today. */
  async monitorRollup(botId: string, machineId: string, dayStart: number): Promise<DeskRollup & { lastFailureTitle?: string }> {
    this.bind(botId);
    const r = deskRollup(this.sql, machineId, dayStart);
    if (!r.lastFailure) return r;
    const row = this.run(r.lastFailure.runId);
    const title = row ? await this.topicTitle(row) : "";
    return title ? { ...r, lastFailureTitle: title } : r;
  }

  /**
   * 用量 (usage/index.ts): this bot's runs that ended in [from, to), with what each cost.
   * Side chats count too: they run on the same subscriptions and keys.
   */
  async usageRuns(botId: string, from: number, to: number): Promise<UsageRun[]> {
    this.bind(botId);
    return this.sql
      .exec<{ ended_at: number; requested_by: string; machine_id: string | null; runtime: string | null; topic_id: string; usage: string | null }>(
        "SELECT ended_at, requested_by, machine_id, runtime, topic_id, usage FROM runs WHERE ended_at >= ? AND ended_at < ? AND external = 0",
        from,
        to,
      )
      .toArray()
      .map((r) => {
        let u: ReturnType<typeof sanitizeUsage> = null;
        try {
          u = r.usage ? sanitizeUsage(JSON.parse(r.usage)) : null;
        } catch {
          u = null;
        }
        return {
          endedAt: r.ended_at,
          requestedBy: r.requested_by,
          machineId: r.machine_id,
          runtime: r.runtime,
          topicId: r.topic_id,
          costUsd: u?.costUsd ?? 0,
          inputTokens: u?.inputTokens ?? 0,
          outputTokens: u?.outputTokens ?? 0,
          ...(u?.models?.length ? { models: u.models } : {}),
        };
      });
  }

  async getRun(botId: string, runId: string): Promise<BotRun | null> {
    this.bind(botId);
    const r = this.run(runId);
    return r ? this.toRun(r) : null;
  }

  async events(
    botId: string,
    runId: string,
    afterSeq = 0,
    limit = DEFAULT_PAGE,
    viewer?: RunViewer,
  ): Promise<{ run: BotRun; entries: unknown[]; hasMore: boolean; lastSeq: number } | null> {
    this.bind(botId);
    const r = this.run(runId);
    if (!r) return null;
    const n = Math.max(1, Math.min(limit, MAX_PAGE));
    // design-takeover 3.6: entries the viewer may not see are left out; a page is never empty
    // while more follow (a client pages from the seq of the last entry it got).
    const entries: unknown[] = [];
    let cursor = afterSeq;
    let hasMore = false;
    // Scans on until an entry is found or the log ends; after the first page in larger steps.
    for (let size = n; ; size = MAX_PAGE) {
      const rows = this.sql
        .exec<{ seq: number; frame: string }>("SELECT seq, frame FROM run_log WHERE run_id = ? AND seq > ? ORDER BY seq LIMIT ?", runId, cursor, size + 1)
        .toArray();
      const page = rows.slice(0, size);
      entries.push(...this.visibleEntries(runId, page.map((x) => JSON.parse(x.frame)), viewer));
      hasMore = rows.length > size;
      if (page.length > 0) cursor = page[page.length - 1].seq;
      if (!hasMore || entries.length > 0) break;
    }
    return {
      run: this.toRun(r),
      entries: entries.slice(0, n),
      hasMore: hasMore || entries.length > n,
      lastSeq: r.last_seq,
    };
  }
}
