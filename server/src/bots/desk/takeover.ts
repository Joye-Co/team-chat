// BotDesk take-over (接管 / 交还) and acceptance criteria (docs/protocol.md, "Run fields, pause
// and resume" and "Run phase and take-over"; design-takeover 3.2, 3.3, 3.7). A person holds a
// run: a pause stops the CLI (run.pause), a take-over also lets the person run commands in the
// work folder (run.pause with hold "takeover", hand.ts). While held, the stall clock stops, the
// run shows `pausedBy` and `holdKind`, MachineLink keeps its lease (setHeld) and reminders go out
// after 2 and 24 hours. Hand-back (run.resume, with an optional note for the CLI) clears the hold
// at once. The run's bot replaces its 验收标准 over HTTP.
import type { AcceptanceItem, BotRun } from "@chat/protocol";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TokenBucket } from "../../core/ratelimit";
import { type MachineView, hasFeature } from "../dispatch";
import { linkStub } from "../env";
import { loadMachines } from "../store";
import { ACTIVE, PLAN_RATE, type RunRow, result } from "./core";
import type { RunRequest } from "./runs";
import { DeskInput } from "./input";
import { HOLD_CLI_TEXT, HOLD_REMIND_MS, HOLD_TEXT, type HoldActor, mayPause, mayResume, mayTakeOver, requesterOf, takeoverOf } from "./hold";
import { HOLDABLE_PHASES, holdKindOf, phaseOf } from "./phase";

/** Who pauses, takes over, hands back or retries: a person; `level: "owner"` is an admin. */
export type TakeoverActor = HoldActor;

export abstract class DeskTakeover extends DeskInput {
  protected readonly acceptanceBuckets = new Map<string, TokenBucket>();

  /** Retry (重试): the requester or an admin, a person. */
  protected mayRetry(r: RunRow, a: TakeoverActor): boolean {
    return a.kind === "human" && (a.id === r.requested_by || a.level === "owner");
  }

  /** The run's machine (undefined when it is gone), with the run's `takeover` kept current. */
  protected async holdMachine(r: RunRow): Promise<MachineView | undefined> {
    const m = r.machine_id ? (await loadMachines(this.env)).find((x) => x.id === r.machine_id) : undefined;
    if (m) {
      const how = takeoverOf(m);
      if (how !== r.takeover) {
        r.takeover = how;
        this.sql.exec("UPDATE runs SET takeover = ? WHERE id = ?", how, r.id);
      }
    }
    return m;
  }

  /** The checks every hold starts with: the run exists, is active, runs on a machine and that machine is ready. */
  protected holdable(r: RunRow | undefined): RunRow {
    if (!r) throw new ApiError("not_found", "run not found");
    if (r.ended_at !== null) throw new ApiError("conflict", "the run ended", "run.ended");
    if (r.external || !r.machine_id) throw new ApiError("conflict", "the run has no machine to pause", "run.no_machine");
    if (r.paused_by === null && !HOLDABLE_PHASES.has(phaseOf(r).phase)) throw new ApiError("conflict", "the run's computer is not ready", "run.not_ready");
    return r;
  }

  /** Marks the run held on its machine (its lease does not run out). Never throws. */
  protected async markHeld(r: RunRow): Promise<void> {
    if (!r.machine_id) return;
    try {
      await linkStub(this.env, r.machine_id).setHeld(r.id, true);
    } catch (err) {
      this.log.warn("hold on the machine failed", { runId: r.id, err });
    }
  }

  /** Pauses an active run on its machine (design-takeover 3.2). */
  async pause(botId: string, runId: string, actor: TakeoverActor): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        const m = await this.holdMachine(r);
        if (!mayPause(actor, requesterOf(r), m)) throw new ApiError("forbidden", "only the requester, or the machine's owner or an admin, pauses a run");
        this.holdable(r);
        if (r.paused_by) throw new ApiError("conflict", "the run is paused", "run.held");
        if (!m || !hasFeature(m, "pause")) throw new ApiError("conflict", "the run's machine cannot pause runs", "run.no_pause");
        const sent = await linkStub(this.env, r.machine_id as string).toRun({ t: "run.pause", runId, by: actor.id } as never);
        if (!sent.ok) throw new ApiError("conflict", "the run is not open on its machine");
        const now = Date.now();
        // A new pause generation: a `paused: false` sent before the daemon took this pause does
        // not clear it (events.ts).
        this.sql.exec("UPDATE runs SET paused_by = ?, paused_at = ?, pause_gen = pause_gen + 1, hold_kind = 'pause', hold_reminded = 0 WHERE id = ?", actor.id, now, runId);
        r.paused_by = actor.id;
        r.paused_at = now;
        r.pause_gen += 1;
        r.hold_kind = "pause";
        r.hold_reminded = 0;
        await this.markHeld(r);
        await this.setStatus(r, r.status);
        await this.scheduleStall();
        this.log.info("run paused", { runId, actorId: actor.id });
        return this.toRun(r);
      });
    });
  }

  /**
   * Takes over an active run (design-takeover 3.2): the CLI stops and the person may run
   * commands in its work folder (hand.ts). From a pause it upgrades the hold. Taking over a run
   * one already holds this way answers the run as it is.
   */
  async takeover(botId: string, runId: string, actor: TakeoverActor): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        const m = await this.holdMachine(r);
        this.holdable(r);
        if (r.paused_by && holdKindOf(r) === "takeover") {
          if (r.paused_by === actor.id) return this.toRun(r);
          throw new ApiError("conflict", "someone else took the run over", "run.held");
        }
        if (takeoverOf(m) === "none") throw new ApiError("conflict", "the run's machine cannot be taken over", "run.no_takeover");
        if (!mayTakeOver(actor, requesterOf(r), m)) throw new ApiError("forbidden", "only the owner of a personal machine, or the requester on an isolated team machine, takes over a run");
        const sent = await linkStub(this.env, r.machine_id as string).toRun({ t: "run.pause", runId, by: actor.id, hold: "takeover" } as never);
        if (!sent.ok) throw new ApiError("conflict", "the run is not open on its machine");
        const now = Date.now();
        // From a pause the CLI has been stopped since then: the hold keeps its start and reminders.
        const upgrade = r.paused_by !== null;
        const at = upgrade && r.paused_at !== null ? r.paused_at : now;
        const reminded = upgrade ? r.hold_reminded : 0;
        this.sql.exec(
          "UPDATE runs SET paused_by = ?, paused_at = ?, pause_gen = pause_gen + 1, hold_kind = 'takeover', hold_reminded = ? WHERE id = ?",
          actor.id,
          at,
          reminded,
          runId,
        );
        r.paused_by = actor.id;
        r.paused_at = at;
        r.pause_gen += 1;
        r.hold_kind = "takeover";
        r.hold_reminded = reminded;
        await this.markHeld(r);
        await this.setStatus(r, r.status);
        await this.scheduleStall();
        this.log.info("run taken over", { runId, actorId: actor.id, upgrade });
        return this.toRun(r);
      });
    });
  }

  /**
   * Resumes a paused run or takes a take-over back (交还, design-takeover 3.2); `note` becomes
   * the CLI's next input. A command the person still runs ends first. The hold clears at once
   * (the resume is queued for a machine that is away); the stall clock starts again.
   */
  async resume(botId: string, runId: string, actor: TakeoverActor, note?: string): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (!r.paused_by) {
          if (!this.mayRetry(r, actor) && !mayPause(actor, requesterOf(r), await this.holdMachine(r))) throw new ApiError("forbidden", "only the person holding the run hands it back");
          if (r.ended_at !== null) throw new ApiError("conflict", "the run ended", "run.ended");
          throw new ApiError("conflict", "the run is not paused", "run.not_held");
        }
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended", "run.ended");
        const m = await this.holdMachine(r);
        if (!mayResume(actor, { by: r.paused_by, kind: holdKindOf(r) }, requesterOf(r), m)) {
          throw new ApiError("forbidden", "only the person holding the run, the requester or the machine's owner or an admin hands it back");
        }
        await this.handBack(r, note);
        this.log.info("run resumed", { runId, actorId: actor.id });
        return this.toRun(r);
      });
    });
  }

  /** Ends the hold of a held run: stops the person's command, resumes the CLI, clears the hold. */
  protected async handBack(r: RunRow, note?: string): Promise<void> {
    if (r.machine_id) {
      const link = linkStub(this.env, r.machine_id);
      if (r.hand_command) await link.toRun({ t: "run.command.cancel", runId: r.id, commandId: r.hand_command } as never, { noQueue: true }).catch(() => undefined);
      const frame = { t: "run.resume", runId: r.id, ...(note && note.trim() !== "" ? { note } : {}) };
      await link.toRun(frame as never);
    }
    const now = Date.now();
    await this.clearHold(r);
    // A machine that is away is waited for from now on (it is no longer held).
    const away = r.away_since !== null ? now : null;
    this.sql.exec("UPDATE runs SET last_event_at = ?, handed_back_at = ?, away_since = ? WHERE id = ?", now, now, away, r.id);
    r.last_event_at = now;
    r.handed_back_at = now;
    r.away_since = away;
    await this.setStatus(r, r.status);
    await this.scheduleStall();
  }

  /**
   * design-takeover 3.2: the person holding runs left the team or a 群 (`groupId`): each of their
   * holds there is handed back, and the topic says so. Returns how many.
   */
  async releaseHolds(botId: string, memberId: string, groupId?: string): Promise<number> {
    this.bind(botId);
    const rows = this.sql
      .exec<{ id: string }>(`SELECT id FROM runs WHERE ${ACTIVE} AND paused_by = ?${groupId ? " AND group_id = ?" : ""}`, ...(groupId ? [memberId, groupId] : [memberId]))
      .toArray();
    if (rows.length === 0) return 0;
    const who = (await this.names([memberId]).catch(() => new Map<string, string>())).get(memberId) ?? "";
    let n = 0;
    for (const { id } of rows) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.paused_by !== memberId) return;
        await this.handBack(r, HOLD_CLI_TEXT.left);
        await this.note(r, HOLD_TEXT.left(who)).catch((err) => this.log.warn("left note failed", { runId: id, err }));
        this.log.info("hold released", { runId: id, memberId });
        n++;
      });
    }
    return n;
  }

  /**
   * MachineLink: the machine of a held run went away (`online` false) or is back. The run keeps
   * its phase; it shows `waitingFor: "offline"` while the machine is away.
   */
  async heldMachine(runId: string, machineId: string, online: boolean): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId || !r.paused_by) return;
      const away = online ? null : (r.away_since ?? Date.now());
      if (away === r.away_since) return;
      this.sql.exec("UPDATE runs SET away_since = ? WHERE id = ?", away, r.id);
      r.away_since = away;
      await this.setStatus(r, r.status);
    });
  }

  // -------------------------------------------------------------------------
  // design-takeover 3.3: reminders while a person holds a run
  // -------------------------------------------------------------------------

  protected holdDueAt(): number | null {
    const rows = this.sql
      .exec<{ paused_at: number; hold_reminded: number }>(
        `SELECT paused_at, hold_reminded FROM runs WHERE ${ACTIVE} AND paused_by IS NOT NULL AND paused_at IS NOT NULL AND hold_reminded < ?`,
        HOLD_REMIND_MS.length,
      )
      .toArray();
    let at: number | null = null;
    for (const x of rows) {
      const due = x.paused_at + HOLD_REMIND_MS[x.hold_reminded];
      if (at === null || due < at) at = due;
    }
    return at;
  }

  /** After 2 hours the holder is reminded; after 24 hours the holder, the requester and the admins. */
  protected async remindHolds(now: number): Promise<void> {
    const due = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND paused_by IS NOT NULL AND paused_at IS NOT NULL AND hold_reminded < ? AND paused_at + CASE hold_reminded WHEN 0 THEN ? ELSE ? END <= ?`,
        HOLD_REMIND_MS.length,
        HOLD_REMIND_MS[0],
        HOLD_REMIND_MS[1],
        now,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || !r.paused_by || r.paused_at === null || r.hold_reminded >= HOLD_REMIND_MS.length) return;
        // Only the reminder that is due now: a hold found after both are due gets the later one.
        let step = r.hold_reminded;
        while (step + 1 < HOLD_REMIND_MS.length && now >= r.paused_at + HOLD_REMIND_MS[step + 1]) step++;
        if (now < r.paused_at + HOLD_REMIND_MS[step]) return;
        this.sql.exec("UPDATE runs SET hold_reminded = ? WHERE id = ?", step + 1, r.id);
        r.hold_reminded = step + 1;
        const holder = r.paused_by;
        const names = await this.names([holder, this.botId as string]).catch(() => new Map<string, string>());
        const member = names.get(this.botId as string) ?? "";
        const text =
          step === 0
            ? HOLD_TEXT.remind(member)
            : HOLD_TEXT.remindAll(names.get(holder) ?? "", member, Math.floor((now - r.paused_at) / 3_600_000), holdKindOf(r));
        const requester = requesterOf(r);
        const mentions = step === 0 ? [holder] : [...new Set([holder, ...(requester ? [requester] : []), ...(await this.groupAdmins(r.group_id))])];
        try {
          await this.post(r, `run:${r.id}:hold:${r.paused_at}:${step}`, text, { mentions });
        } catch (err) {
          this.log.warn("hold reminder failed", { runId: r.id, err });
        }
      });
    }
  }

  /** The active human admins in the group (mentions must be group members). */
  protected async groupAdmins(groupId: string): Promise<string[]> {
    const rows = await this.env.DB.prepare(
      `SELECT m.id FROM members m JOIN group_members g ON g.member_id = m.id AND g.group_id = ?
       WHERE m.level = 'owner' AND m.kind = 'human' AND m.deactivated = 0 ORDER BY m.id LIMIT 20`,
    )
      .bind(groupId)
      .all<{ id: string }>()
      .catch(() => ({ results: [] as { id: string }[] }));
    return rows.results.map((x) => x.id);
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
      if (!this.mayRetry(r, actor)) throw new ApiError("forbidden", "only the requester or an owner retries a run");
      if (r.ended_at === null) throw new ApiError("conflict", "the run is still active");
      if (r.external) throw new ApiError("conflict", "a run started over HTTP cannot be retried");
      if (!(await this.botActive())) throw new ApiError("conflict", "the member takes no runs");
      if (this.startBlocked(r.topic_id)) throw new ApiError("conflict", "the topic has an active run");
      const request = this.requestOf(r) ?? (await this.requestFromRoom(r));
      if (!request) throw new ApiError("conflict", "the request of this run is gone");
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
