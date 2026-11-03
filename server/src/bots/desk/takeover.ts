// BotDesk take-over (接管 / 交还) and acceptance criteria (docs/protocol.md, "Run fields, pause
// and resume"). A person pauses a run: the machine stops the CLI (run.pause), the stall clock
// stops, and the run shows `pausedBy` until someone resumes it (run.resume, with an optional
// note for the CLI). The run's bot replaces its 验收标准 over HTTP.
import type { AcceptanceItem, BotRun } from "@chat/protocol";
import { ApiError, type Result } from "../../core/errors";
import { TokenBucket } from "../../core/ratelimit";
import { hasFeature } from "../dispatch";
import { linkStub } from "../env";
import { loadMachines } from "../store";
import { PLAN_RATE, type RunRow, result } from "./core";
import { DeskInput } from "./input";

/** Who may pause or resume: a person who is the run's requester, or an owner. */
export interface TakeoverActor {
  id: string;
  kind: "human" | "bot";
  level: "owner" | "member";
}

export abstract class DeskTakeover extends DeskInput {
  protected readonly acceptanceBuckets = new Map<string, TokenBucket>();

  protected mayTakeOver(r: RunRow, a: TakeoverActor): boolean {
    return a.kind === "human" && (a.id === r.requested_by || a.level === "owner");
  }

  /** Pauses an active run on its machine. */
  async pause(botId: string, runId: string, actor: TakeoverActor): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (!this.mayTakeOver(r, actor)) throw new ApiError("forbidden", "only the requester or an owner takes over a run");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        if (r.paused_by) throw new ApiError("conflict", "the run is paused");
        if (r.external || !r.machine_id) throw new ApiError("conflict", "the run has no machine to pause");
        const machine = (await loadMachines(this.env)).find((m) => m.id === r.machine_id);
        if (!machine || !hasFeature(machine, "pause")) throw new ApiError("conflict", "the run's machine cannot pause runs");
        const sent = await linkStub(this.env, r.machine_id).toRun({ t: "run.pause", runId, by: actor.id } as never);
        if (!sent.ok) throw new ApiError("conflict", "the run is not open on its machine");
        const now = Date.now();
        this.sql.exec("UPDATE runs SET paused_by = ?, paused_at = ? WHERE id = ?", actor.id, now, runId);
        r.paused_by = actor.id;
        r.paused_at = now;
        await this.setStatus(r, r.status);
        this.log.info("run paused", { runId, actorId: actor.id });
        return this.toRun(r);
      });
    });
  }

  /** Resumes a paused run; `note` becomes the CLI's next input. The stall clock starts again. */
  async resume(botId: string, runId: string, actor: TakeoverActor, note?: string): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (!this.mayTakeOver(r, actor)) throw new ApiError("forbidden", "only the requester or an owner hands a run back");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        if (!r.paused_by) throw new ApiError("conflict", "the run is not paused");
        if (r.machine_id) {
          const frame = { t: "run.resume", runId, ...(note && note.trim() !== "" ? { note } : {}) };
          await linkStub(this.env, r.machine_id).toRun(frame as never);
        }
        const now = Date.now();
        this.sql.exec("UPDATE runs SET paused_by = NULL, paused_at = NULL, last_event_at = ? WHERE id = ?", now, runId);
        r.paused_by = null;
        r.paused_at = null;
        r.last_event_at = now;
        await this.setStatus(r, r.status);
        await this.scheduleStall();
        this.log.info("run resumed", { runId, actorId: actor.id });
        return this.toRun(r);
      });
    });
  }

  /** Replaces the acceptance criteria of an active run of this bot (HTTP; the caller is the bot). */
  async setAcceptance(botId: string, runId: string, items: AcceptanceItem[]): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      let bucket = this.acceptanceBuckets.get(runId);
      if (!bucket) {
        bucket = new TokenBucket(PLAN_RATE.rate, PLAN_RATE.burst);
        this.acceptanceBuckets.set(runId, bucket);
      }
      if (!bucket.take()) throw new ApiError("rate_limited", "too many acceptance updates for this run");
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        if (this.storeAcceptance(r, items)) await this.setStatus(r, r.status);
        return this.toRun(r);
      });
    });
  }
}
