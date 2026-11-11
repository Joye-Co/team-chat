// BotDesk stall detection: the alarm that marks silent runs blocked and fails runs whose start
// never reached a machine. Paused runs (接管) have no stall deadline until they resume.
import { RUN_KIND_ASIDE } from "@chat/protocol";
import { HEARTBEAT_STALE_MS, STALL_MS, TEXT } from "../dispatch";
import { CONTINUE_TEXT, LOOP_LIMIT, LOOP_TEXT, MACHINE_GONE_MS, MAX_AUTO_CONTINUE, MAX_INTERVENTIONS, NUDGE_MS, NUDGE_TEXT, TOOL_STALL_MS, isPromiseOnly, loopKey } from "../../agent/supervise";
import { ulid } from "../../core/ids";
import { linkStub } from "../env";
import { loadMachines } from "../store";
import { ACTIVE, QUEUE_MAX_MS, QUEUE_RETRY_MS, QUEUED, type RunRow, START_STUCK_MS } from "./core";
import { DeskEvents } from "./events";

export abstract class DeskStall extends DeskEvents {
  // -------------------------------------------------------------------------
  // Stall detection
  // -------------------------------------------------------------------------

  protected async scheduleStall(): Promise<void> {
    const stall = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL`,
      )
      .one().at;
    const stuck = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(created_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL AND queued_at IS NULL`,
      )
      .one().at;
    // Queued runs (排队): try again soon, and fail the ones that waited too long.
    const queued = this.sql.exec<{ at: number | null }>(`SELECT min(queued_at) AS at FROM runs WHERE ${QUEUED}`).one().at;
    const retry = queued === null ? null : Math.min(Date.now() + QUEUE_RETRY_MS, queued + QUEUE_MAX_MS);
    // 盯活: the nudge comes before the stall, and running runs are checked every minute for a
    // machine that went away.
    const running = this.sql.exec<{ n: number }>(`SELECT count(*) AS n FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running'`).one().n;
    const watch = running > 0 ? Date.now() + 60_000 : null;
    const nudge = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND nudged = 0 AND open_tools = 0 AND interventions < ${MAX_INTERVENTIONS} AND approval_id IS NULL AND paused_by IS NULL`,
      )
      .one().at;
    const times = [stall === null ? null : stall + STALL_MS, stuck === null ? null : stuck + START_STUCK_MS, retry, nudge === null ? null : nudge + NUDGE_MS, watch].filter(
      (x): x is number => x !== null,
    );
    if (times.length === 0) return;
    const at = Math.min(...times);
    const cur = await this.ctx.storage.getAlarm();
    if (cur === null || cur > at || cur < Date.now()) await this.ctx.storage.setAlarm(at);
  }

  async alarm(): Promise<void> {
    const now = Date.now();
    const due = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL AND last_event_at <= CASE WHEN open_tools > 0 THEN ? ELSE ? END`,
        now - TOOL_STALL_MS,
        now - STALL_MS,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.external || r.ended_at !== null || r.stalled || r.paused_by || r.status !== "running" || r.last_event_at > now - (r.open_tools > 0 ? TOOL_STALL_MS : STALL_MS)) return;
        await this.markStalled(r);
      });
    }
    await this.superviseRunning(now);
    // Runs whose start broke before they reached a machine (for example a reset mid-start).
    const stuck = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL AND queued_at IS NULL AND created_at <= ?`,
        now - START_STUCK_MS,
      )
      .toArray();
    for (const { id } of stuck) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id || r.status !== "created" || r.queued_at) return;
        await this.end(r, "failed", TEXT.failed(""), "blocked", TEXT.failed(""));
      });
    }
    const expired = this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${QUEUED} AND queued_at <= ?`, now - QUEUE_MAX_MS).toArray();
    for (const { id } of expired) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id || !r.queued_at) return;
        const why = r.kind === RUN_KIND_ASIDE ? TEXT.noAsideMachine : TEXT.noMachine;
        await this.end(r, "failed", why, "blocked", why);
      });
    }
    await this.dispatchQueued();
    await this.scheduleStall();
  }

  // -------------------------------------------------------------------------
  // 盯活 (agent/supervise.ts): the desk steps in by itself before it tells people
  // -------------------------------------------------------------------------

  /** Silent runs get one nudge; runs whose machine went away start again elsewhere. */
  protected async superviseRunning(now: number): Promise<void> {
    const silent = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND nudged = 0 AND open_tools = 0 AND interventions < ${MAX_INTERVENTIONS} AND approval_id IS NULL AND paused_by IS NULL AND last_event_at <= ?`,
        now - NUDGE_MS,
      )
      .toArray();
    for (const { id } of silent) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.nudged || r.stalled || r.open_tools > 0 || r.interventions >= MAX_INTERVENTIONS || r.approval_id || r.paused_by || !r.machine_id) return;
        this.sql.exec("UPDATE runs SET nudged = 1, interventions = interventions + 1 WHERE id = ?", r.id);
        r.nudged = 1;
        r.interventions += 1;
        await this.tell(r, NUDGE_TEXT, "nudge");
      });
    }
    const onMachines = this.sql
      .exec<{ id: string; machine_id: string }>(`SELECT id, machine_id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND machine_id IS NOT NULL AND paused_by IS NULL`)
      .toArray();
    if (onMachines.length === 0) return;
    const machines = new Map((await loadMachines(this.env)).map((m) => [m.id, m]));
    for (const { id, machine_id } of onMachines) {
      const m = machines.get(machine_id);
      const gone = !m || m.revoked || m.lastHeartbeatAt === undefined || now - m.lastHeartbeatAt > Math.max(MACHINE_GONE_MS, HEARTBEAT_STALE_MS);
      if (!gone) continue;
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id !== machine_id) return;
        await this.restartElsewhere(r, "machine gone");
      });
    }
  }

  /** A `run.input` from the desk to the CLI (not a member's message: it is not kept as input). */
  protected async tell(r: RunRow, text: string, why: string): Promise<void> {
    if (!r.machine_id) return;
    this.log.info("supervise", { runId: r.id, why, interventions: r.interventions });
    try {
      await linkStub(this.env, r.machine_id).toRun({ t: "run.input", runId: r.id, inputId: `desk-${why}-${ulid()}`, text, authorId: this.botId } as never);
    } catch (err) {
      this.log.warn("supervise input failed", { runId: r.id, err });
    }
  }

  /** The same step again and again: one reminder per streak. */
  protected async watchTool(r: RunRow, title: string): Promise<void> {
    const key = loopKey(title);
    const count = key !== "" && key === r.loop_key ? r.loop_count + 1 : 1;
    this.sql.exec("UPDATE runs SET tool_calls = tool_calls + 1, loop_key = ?, loop_count = ? WHERE id = ?", key, count, r.id);
    r.tool_calls += 1;
    r.loop_key = key;
    r.loop_count = count;
    if (count === LOOP_LIMIT && r.interventions < MAX_INTERVENTIONS) {
      this.sql.exec("UPDATE runs SET interventions = interventions + 1 WHERE id = ?", r.id);
      r.interventions += 1;
      await this.tell(r, LOOP_TEXT(title), "loop");
    }
  }

  /** The CLI ended with a promise and no tool call: a follow-up run continues the session. */
  protected async continuePromise(r: RunRow, result: string): Promise<boolean> {
    if (r.kind === RUN_KIND_ASIDE || !isPromiseOnly(result, r.tool_calls) || r.auto_continues >= MAX_AUTO_CONTINUE) return false;
    if (this.activeRun(r.topic_id)) return false;
    const key = `continue:${ulid()}`;
    try {
      await this.start(
        { groupId: r.group_id, topicId: r.topic_id, messageId: key, authorId: r.requested_by, body: CONTINUE_TEXT, mentioned: true },
        r,
      );
    } catch (err) {
      this.log.warn("auto continue failed", { runId: r.id, err });
      return false;
    }
    const next = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (!next) return false;
    this.sql.exec("UPDATE runs SET auto_continues = ?, interventions = ? WHERE id = ?", r.auto_continues + 1, r.interventions + 1, next.id);
    this.log.info("supervise", { runId: r.id, why: "continue", next: next.id });
    return true;
  }

  /**
   * Ends the run without a message and starts its request again (on a healthy machine: the old
   * one is no longer one). The new run carries the count; out of tries it returns false.
   */
  protected async restartElsewhere(r: RunRow, why: string): Promise<boolean> {
    if (r.external || r.kind === RUN_KIND_ASIDE || r.interventions >= MAX_INTERVENTIONS) return false;
    const request = this.requestOf(r);
    if (!request) return false;
    if (r.machine_id) {
      try {
        await linkStub(this.env, r.machine_id).toRun({ t: "run.cancel", runId: r.id } as never);
      } catch {
        // The machine is gone; the cancel waits in its queue or is lost.
      }
    }
    await this.closeProgress(r);
    this.sql.exec("UPDATE runs SET approval_id = NULL, paused_by = NULL, paused_at = NULL WHERE id = ?", r.id);
    await this.setStatus(r, "cancelled", { ended_at: Date.now(), summary: `restarted: ${why}` });
    const key = `restart:${ulid()}`;
    const ack = request.m.ackMessageId ?? (r.trigger_message_id.includes(":") ? undefined : r.trigger_message_id);
    // The old machine's session is not reachable: the new run starts fresh with the full topic.
    this.sql.exec("UPDATE runs SET session_id = NULL WHERE id = ?", r.id);
    await this.start({ ...request.m, messageId: key, ackMessageId: ack, mentioned: true }, undefined, request.followUps, r.id, request.brief);
    const next = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (next) this.sql.exec("UPDATE runs SET interventions = ? WHERE id = ?", r.interventions + 1, next.id);
    this.log.info("supervise", { runId: r.id, why: `restart: ${why}`, next: next?.id });
    return true;
  }

  /** No progress: one note, the run and the topic show blocked until events come again. */
  protected async markStalled(r: RunRow): Promise<void> {
    // A paused run (接管) is quiet on purpose.
    if (r.stalled || r.ended_at !== null || r.approval_id || r.paused_by) return;
    // 盯活: start it again elsewhere while the desk has tries left; people hear about it only after.
    if (await this.restartElsewhere(r, "stalled")) return;
    this.sql.exec("UPDATE runs SET stalled = 1 WHERE id = ?", r.id);
    r.stalled = 1;
    try {
      await this.note(r, r.interventions >= MAX_INTERVENTIONS ? TEXT.gaveUp : TEXT.stalled);
    } catch (err) {
      this.log.warn("stall note failed", { runId: r.id, err });
    }
    await this.setStatus(r, "blocked");
    await this.topic(r, "blocked");
  }
}
