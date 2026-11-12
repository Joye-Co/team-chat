// BotDesk take-over (接管 / 交还) and acceptance criteria (docs/protocol.md, "Run fields, pause
// and resume"). A person pauses a run: the machine stops the CLI (run.pause), the stall clock
// stops, and the run shows `pausedBy` until someone resumes it (run.resume, with an optional
// note for the CLI). The run's bot replaces its 验收标准 over HTTP.
import type { AcceptanceItem, BotRun } from "@chat/protocol";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TokenBucket } from "../../core/ratelimit";
import { hasFeature, personalBlock } from "../dispatch";
import { linkStub } from "../env";
import { loadMachines, loadRunner } from "../store";
import { PLAN_RATE, type RunRow, result } from "./core";
import type { RunRequest } from "./runs";
import { DeskInput } from "./input";

/** Who may pause or resume: a person who is the run's requester, or an owner. */
export interface TakeoverActor {
  id: string;
  kind: "human" | "bot";
  level: "owner" | "member";
}

export abstract class DeskTakeover extends DeskInput {
  protected readonly acceptanceBuckets = new Map<string, TokenBucket>();

  /**
   * Who may pause, hand back or retry: the requester or an admin. design-roles 3.6: on a personal
   * machine (`machineOwner`) the person the run works for and the machine's owner, no admin.
   */
  protected mayTakeOver(r: RunRow, a: TakeoverActor, machineOwner: string | null = null): boolean {
    if (a.kind !== "human") return false;
    if (machineOwner) return a.id === machineOwner || a.id === (r.requested_for ?? r.requested_by);
    return a.id === r.requested_by || a.level === "owner";
  }

  /** Pauses an active run on its machine. */
  async pause(botId: string, runId: string, actor: TakeoverActor): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (!this.mayTakeOver(r, actor, await this.machineOwnerOf(r))) throw new ApiError("forbidden", "only the requester or an owner takes over a run");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        if (r.paused_by) throw new ApiError("conflict", "the run is paused");
        if (r.external || !r.machine_id) throw new ApiError("conflict", "the run has no machine to pause");
        const machine = (await loadMachines(this.env)).find((m) => m.id === r.machine_id);
        if (!machine || !hasFeature(machine, "pause")) throw new ApiError("conflict", "the run's machine cannot pause runs");
        const sent = await linkStub(this.env, r.machine_id).toRun({ t: "run.pause", runId, by: actor.id } as never);
        if (!sent.ok) throw new ApiError("conflict", "the run is not open on its machine");
        const now = Date.now();
        // A new pause generation: a `paused: false` sent before the daemon took this pause does
        // not clear it (events.ts).
        this.sql.exec("UPDATE runs SET paused_by = ?, paused_at = ?, pause_gen = pause_gen + 1 WHERE id = ?", actor.id, now, runId);
        r.paused_by = actor.id;
        r.paused_at = now;
        r.pause_gen += 1;
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
        if (!this.mayTakeOver(r, actor, await this.machineOwnerOf(r))) throw new ApiError("forbidden", "only the requester or an owner hands a run back");
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

  /**
   * 重试: starts a new run with the same request as an ended run (its author, text and
   * follow-ups), linked by `retryOf`. The requester or an owner (a person) may.
   */
  async retry(botId: string, runId: string, actor: TakeoverActor): Promise<Result<BotRun>> {
    return result(async () => {
      this.bind(botId);
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "run not found");
      if (!this.mayTakeOver(r, actor)) throw new ApiError("forbidden", "only the requester or an owner retries a run");
      if (r.ended_at === null) throw new ApiError("conflict", "the run is still active");
      if (r.external) throw new ApiError("conflict", "a run started over HTTP cannot be retried");
      if (!(await this.botActive())) throw new ApiError("conflict", "the member takes no runs");
      if (this.startBlocked(r.topic_id)) throw new ApiError("conflict", "the topic has an active run");
      const request = this.requestOf(r) ?? (await this.requestFromRoom(r));
      if (!request) throw new ApiError("conflict", "the request of this run is gone");
      // design-roles 3.3: a retry works for the same person; refused when no machine may take it.
      const runner = await loadRunner(this.env.DB, botId);
      if (runner && personalBlock(await loadMachines(this.env), runner, await this.placeCtx(r))) {
        throw new ApiError("conflict", "no computer may take this work for the person it is for", "machine.personal_requester");
      }
      // A key of its own: the original message already started a run.
      const key = `retry:${ulid()}`;
      const ack = request.m.ackMessageId ?? (r.trigger_message_id.includes(":") ? undefined : r.trigger_message_id);
      await this.start({ ...request.m, messageId: key, ackMessageId: ack, mentioned: true }, undefined, request.followUps, r.id, request.brief);
      const started = this.sql.exec<RunRow>("SELECT * FROM runs WHERE trigger_message_id = ?", key).toArray()[0] as RunRow | undefined;
      if (!started) throw new ApiError("conflict", "the member takes no runs in this topic");
      this.log.info("run retried", { runId, retryId: started.id, actorId: actor.id });
      return this.toRun(started);
    });
  }


  /** The request of a run made before requests were kept: its trigger message, from the room. */
  protected async requestFromRoom(r: RunRow): Promise<RunRequest | null> {
    if (r.trigger_message_id.includes(":")) return null;
    try {
      const m = unwrap(await this.room(r).rpc(r.group_id, "bots.request", this.actor(), { messageId: r.trigger_message_id })) as {
        authorId: string;
        body: string;
      } | null;
      if (!m) return null;
      return { m: { groupId: r.group_id, topicId: r.topic_id, messageId: r.trigger_message_id, authorId: m.authorId, body: m.body, mentioned: true }, followUps: [] };
    } catch (err) {
      this.log.warn("request lookup failed", { runId: r.id, err });
      return null;
    }
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
