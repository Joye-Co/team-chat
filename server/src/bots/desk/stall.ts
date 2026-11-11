// BotDesk stall detection: the alarm that marks silent runs blocked and fails runs whose start
// never reached a machine. Paused runs (接管) have no stall deadline until they resume.
import { RUN_KIND_ASIDE } from "@chat/protocol";
import { HEARTBEAT_STALE_MS, type MachineView, STALL_MS, TEXT, cut, isOnline } from "../dispatch";
import { CONTINUE_TEXT, LOOP_LIMIT, LOOP_TEXT, MACHINE_GONE_MS, MACHINE_MOVE_MS, MAX_AUTO_CONTINUE, MAX_INTERVENTIONS, NUDGE_MS, NUDGE_TEXT, TOOL_STALL_MS, isPromiseOnly, loopKey } from "../../agent/supervise";
import { ulid } from "../../core/ids";
import { linkStub } from "../env";
import { loadMachines, loadRunner } from "../store";
import { ACTIVE, QUEUE_MAX_MS, QUEUE_RETRY_MS, QUEUED, type RunRow, START_STUCK_MS } from "./core";
import { DeskEvents } from "./events";
import type { Restart } from "./runs";

/** A run that waits for its machine (盯 1.1) is not silent: no nudge, no stall. */
const PRESENT = "away_since IS NULL";

export abstract class DeskStall extends DeskEvents {
  // -------------------------------------------------------------------------
  // Stall detection
  // -------------------------------------------------------------------------

  protected async scheduleStall(): Promise<void> {
    const stall = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT}`,
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
    // 盯活: the nudge comes before the stall, and running runs (and runs waiting for their
    // machine) are checked every minute for a machine that went away or came back.
    const running = this.sql.exec<{ n: number }>(`SELECT count(*) AS n FROM runs WHERE ${ACTIVE} AND external = 0 AND (status = 'running' OR away_since IS NOT NULL)`).one().n;
    const watch = running > 0 ? Date.now() + 60_000 : null;
    const nudge = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND nudged = 0 AND open_tools = 0 AND interventions < ${MAX_INTERVENTIONS} AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT} AND stall_step = 0`,
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
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT} AND last_event_at <= CASE WHEN open_tools > 0 THEN ? ELSE ? END`,
        now - TOOL_STALL_MS,
        now - STALL_MS,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.external || r.ended_at !== null || r.stalled || r.paused_by || r.away_since !== null || r.status !== "running" || r.last_event_at > now - (r.open_tools > 0 ? TOOL_STALL_MS : STALL_MS)) return;
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
    if (expired.length > 0) {
      // 机 2.10: say what it waited for (a computer that stayed offline, or a free one).
      const runner = await loadRunner(this.env.DB, this.botId as string);
      const machines = await loadMachines(this.env);
      const hours = Math.round(QUEUE_MAX_MS / 3_600_000);
      for (const { id } of expired) {
        await this.serial(id, async () => {
          const r = this.run(id);
          if (!r || r.ended_at !== null || r.machine_id || !r.queued_at) return;
          const none = runner ? this.whyNone(r, runner, machines) : null;
          if (runner && r.kind !== RUN_KIND_ASIDE && none?.why === "signed_out") {
            // 模 3.1: what it waited for is a sign-in: say which computer needs it.
            const note = await this.signedOutNote(r, runner.runtime, none.machines);
            await this.end(r, "failed", note.text, "blocked", cut(note.text, 280), false, note.mentions);
            return;
          }
          const why = r.kind === RUN_KIND_ASIDE ? TEXT.noAsideMachine : TEXT.queueExpired(hours, none?.why === "offline" ? none.names : undefined);
          await this.end(r, "failed", why, "blocked", why);
        });
      }
    }
    await this.dispatchQueued();
    await this.scheduleStall();
  }

  // -------------------------------------------------------------------------
  // 盯活 (agent/supervise.ts): the desk steps in by itself before it tells people
  // -------------------------------------------------------------------------

  /**
   * Silent runs get one nudge (once per piece of work: a restarted one goes on to the next
   * step). A run whose machine went away waits for it (盯 1.1), and goes on when it is back.
   */
  protected async superviseRunning(now: number): Promise<void> {
    const silent = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND nudged = 0 AND open_tools = 0 AND interventions < ${MAX_INTERVENTIONS} AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT} AND stall_step = 0 AND last_event_at <= ?`,
        now - NUDGE_MS,
      )
      .toArray();
    for (const { id } of silent) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.nudged || r.stalled || r.open_tools > 0 || r.interventions >= MAX_INTERVENTIONS || r.approval_id || r.paused_by || r.away_since !== null || r.stall_step > 0 || !r.machine_id) return;
        this.sql.exec("UPDATE runs SET nudged = 1, nudge_sent = 1, interventions = interventions + 1 WHERE id = ?", r.id);
        r.nudged = 1;
        r.nudge_sent = 1;
        r.interventions += 1;
        await this.tell(r, NUDGE_TEXT, "nudge");
      });
    }
    const onMachines = this.sql
      .exec<{ id: string; machine_id: string }>(
        `SELECT id, machine_id FROM runs WHERE ${ACTIVE} AND external = 0 AND (status = 'running' OR away_since IS NOT NULL) AND machine_id IS NOT NULL AND paused_by IS NULL`,
      )
      .toArray();
    if (onMachines.length === 0) return;
    const machines = new Map((await loadMachines(this.env)).map((m) => [m.id, m]));
    for (const { id, machine_id } of onMachines) {
      const m = machines.get(machine_id);
      const gone = !m || m.revoked || m.lastHeartbeatAt === undefined || now - m.lastHeartbeatAt > Math.max(MACHINE_GONE_MS, HEARTBEAT_STALE_MS);
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id !== machine_id) return;
        if (gone) await this.machineAway(r, m, now);
        // Online again, but only replayed frames show it still has the run (events.ts): no note yet.
        else if (r.away_since !== null && m && isOnline(m, now)) await this.machineBack(r, false);
      });
    }
  }

  // -------------------------------------------------------------------------
  // 盯 1.1: a machine that goes away is waited for, not punished
  // -------------------------------------------------------------------------

  /**
   * The run's machine is away. The run waits for it (the daemon keeps working offline and
   * replays on reconnect); after MACHINE_MOVE_MS it moves when another healthy machine can take
   * it, else it waits on. A removed machine never comes back. True while the run waits.
   */
  protected async machineAway(r: RunRow, m: MachineView | undefined, now: number): Promise<boolean> {
    if (r.ended_at !== null || !r.machine_id) return false;
    if (r.away_since === null) {
      const since = Math.min(now, m?.lastHeartbeatAt ?? now);
      this.sql.exec("UPDATE runs SET away_since = ? WHERE id = ?", since, r.id);
      r.away_since = since;
      this.log.info("supervise", { runId: r.id, why: "machine away", machineId: r.machine_id });
      if (m) await this.note(r, TEXT.machineAway(m.name)).catch((err) => this.log.warn("away note failed", { runId: r.id, err }));
    }
    const removed = !m || m.revoked;
    if (!removed && now - r.away_since < MACHINE_MOVE_MS) return true;
    if ((await this.canMove(r)) && (await this.restartElsewhere(r, removed ? "machine removed" : "machine away", { exclude: [r.machine_id] }, { count: false }))) {
      if (m) await this.note(r, TEXT.machineMoved(m.name, Math.round((now - r.away_since) / 60_000))).catch((err) => this.log.warn("move note failed", { runId: r.id, err }));
      return false;
    }
    if (!removed) return true;
    await this.end(r, "blocked", TEXT.lost, "blocked", TEXT.lost);
    return false;
  }

  /** The run's machine is back: the run goes on, and its silence counts from now. `say`: it showed it still has the run. */
  protected async machineBack(r: RunRow, say = true): Promise<void> {
    if (r.away_since === null || r.ended_at !== null) return;
    const now = Date.now();
    this.sql.exec("UPDATE runs SET away_since = NULL, last_event_at = max(last_event_at, ?), nudged = 0 WHERE id = ?", now, r.id);
    r.away_since = null;
    r.last_event_at = Math.max(r.last_event_at, now);
    r.nudged = 0;
    this.log.info("supervise", { runId: r.id, why: "machine back", machineId: r.machine_id });
    const row = say && r.machine_id
      ? await this.env.DB.prepare("SELECT name FROM machines WHERE id = ?")
          .bind(r.machine_id)
          .first<{ name: string }>()
          .catch(() => null)
      : null;
    if (row) await this.note(r, TEXT.machineBack(row.name)).catch((err) => this.log.warn("back note failed", { runId: r.id, err }));
  }

  /** The lease ran out (MachineLink): wait for the machine, unless it is there without the run. */
  protected async machineLost(r: RunRow): Promise<boolean> {
    const now = Date.now();
    const m = (await loadMachines(this.env)).find((x) => x.id === r.machine_id);
    if (m && isOnline(m, now)) {
      // The daemon is connected and no longer reports the run: it does not have it any more.
      if (await this.restartElsewhere(r, "machine lost")) return false;
      await this.end(r, "blocked", TEXT.lost, "blocked", TEXT.lost);
      return false;
    }
    return this.machineAway(r, m, now);
  }

  /** Another healthy machine (not this run's, not one it left out) can take the run now. */
  protected async canMove(r: RunRow): Promise<boolean> {
    if (r.external || r.kind === RUN_KIND_ASIDE || !r.machine_id) return false;
    const runner = await loadRunner(this.env.DB, this.botId as string);
    if (!runner?.enabled) return false;
    const exclude = [r.machine_id, ...(JSON.parse(r.excluded) as string[])];
    return this.pick(r, runner, await loadMachines(this.env), exclude) !== null;
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
   * Ends the run without a message and starts its request again: in the same CLI session
   * (`sameSession`), or fresh, leaving out the machines in `exclude`. The new run carries the
   * count and the stall step; out of tries (when it counts) it returns false.
   */
  protected async restartElsewhere(r: RunRow, why: string, how: Restart = {}, opts: { count?: boolean; step?: number } = {}): Promise<boolean> {
    const count = opts.count ?? true;
    if (r.external || r.kind === RUN_KIND_ASIDE || (count && r.interventions >= MAX_INTERVENTIONS)) return false;
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
    await this.closeApprovalCard(r, "expired");
    this.sql.exec("UPDATE runs SET approval_id = NULL, paused_by = NULL, paused_at = NULL WHERE id = ?", r.id);
    await this.setStatus(r, "cancelled", { ended_at: Date.now(), summary: `restarted: ${why}` });
    const key = `restart:${ulid()}`;
    const ack = request.m.ackMessageId ?? (r.trigger_message_id.includes(":") ? undefined : r.trigger_message_id);
    // A new session starts fresh with the full topic; the same session gets what is new.
    if (!how.sameSession) this.sql.exec("UPDATE runs SET session_id = NULL WHERE id = ?", r.id);
    // The same session lives on its machine: the retry stays there (it waits when that is full).
    const keep: Restart = how.sameSession && r.machine_id ? { ...how, machine: r.machine_id } : how;
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `restart:${r.id}`, JSON.stringify(keep));
    await this.start({ ...request.m, messageId: key, ackMessageId: ack, mentioned: true }, undefined, request.followUps, r.id, request.brief);
    const next = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (next) this.sql.exec("UPDATE runs SET interventions = ?, stall_step = ?, nudge_sent = ? WHERE id = ?", r.interventions + (count ? 1 : 0), opts.step ?? r.stall_step, r.nudge_sent, next.id);
    this.log.info("supervise", { runId: r.id, why: `restart: ${why}`, next: next?.id });
    return true;
  }

  /**
   * No progress (盯 1.3): after the nudge, interrupt the run and retry it in the same session;
   * then move it to another machine when one can take it; only then one note, and the run and
   * the topic show blocked until events come again.
   */
  protected async markStalled(r: RunRow): Promise<void> {
    // A paused run (接管) is quiet on purpose; a run waiting for its machine is not silent.
    if (r.stalled || r.ended_at !== null || r.approval_id || r.paused_by || r.away_since !== null) return;
    if (r.stall_step < 1 && (await this.restartElsewhere(r, "stalled", { sameSession: true }, { step: 1 }))) return;
    if (r.stall_step < 2 && r.machine_id && r.interventions < MAX_INTERVENTIONS && (await this.canMove(r))) {
      if (await this.restartElsewhere(r, "stalled", { exclude: [r.machine_id] }, { step: 2 })) return;
    }
    this.sql.exec("UPDATE runs SET stalled = 1 WHERE id = ?", r.id);
    r.stalled = 1;
    // 盯 1.12: the true silence, and the tries that really happened.
    const minutes = Math.max(1, Math.round((Date.now() - r.last_event_at) / 60_000));
    const tries = [...(r.nudge_sent ? ["提醒"] : []), ...(r.stall_step >= 1 ? ["打断后重来"] : []), ...(r.stall_step >= 2 ? ["换电脑重来"] : [])];
    try {
      await this.note(r, tries.length > 0 ? TEXT.gaveUp(minutes, tries) : TEXT.stalled(minutes));
    } catch (err) {
      this.log.warn("stall note failed", { runId: r.id, err });
    }
    await this.setStatus(r, "blocked");
    await this.topic(r, "blocked");
  }
}
