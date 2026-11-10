// BotDesk: one per runner-backed bot member, the bot's front desk. It never runs commands and
// has no model: fixed rules turn a mention into a run on a machine, run events into topic
// messages and topic status, and later topic messages into run input. The normalized run log
// lives in its SQLite. This class adds runs of external bots, plans over HTTP and reads to the
// feature modules it extends (core, runs, events, stall, input, takeover, interact).
import type { BotRun, ModelUsage, RunPlan, RunStatus, TopicStatus } from "@chat/protocol";
import { activityAfterRun } from "../../activity/events";
import { roomStub } from "../../core/env";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TokenBucket } from "../../core/ratelimit";
import { cut } from "../dispatch";
import { DEFAULT_PAGE, MAX_PAGE, PLAN_RATE, type RunRow, result, sanitizeUsage } from "./core";

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
import { DeskInteract } from "./interact";
import { type DeskRollup, deskRollup } from "../../monitor/rollup";

export class BotDesk extends DeskInteract {
  /** After a run ended: 活动记录's 没跑成 and 到了预算 (activity/events.ts), off the run's turn. */
  protected override async end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string, success = false): Promise<void> {
    await super.end(r, status, body, topicStatus, summary, success);
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
  ): Promise<{ run: BotRun; entries: unknown[]; hasMore: boolean; lastSeq: number } | null> {
    this.bind(botId);
    const r = this.run(runId);
    if (!r) return null;
    const n = Math.max(1, Math.min(limit, MAX_PAGE));
    const rows = this.sql
      .exec<{ frame: string }>("SELECT frame FROM run_log WHERE run_id = ? AND seq > ? ORDER BY seq LIMIT ?", runId, afterSeq, n + 1)
      .toArray();
    return {
      run: this.toRun(r),
      entries: rows.slice(0, n).map((x) => JSON.parse(x.frame)),
      hasMore: rows.length > n,
      lastSeq: r.last_seq,
    };
  }
}
