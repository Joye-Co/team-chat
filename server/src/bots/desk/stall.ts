// BotDesk stall detection: the alarm that marks silent runs blocked and fails runs whose start
// never reached a machine. Paused runs (接管) have no stall deadline until they resume.
import { MAX_SUPERVISE_ACTS, RUN_KIND_ASIDE, type SuperviseAct } from "@chat/protocol";
import { HEARTBEAT_STALE_MS, MAX_RUN_MS, type MachineView, STALL_MS, TEXT, cut, isOnline, workTime } from "../dispatch";
import {
  CONTINUE_TEXT,
  LIMIT_GRACE_MS,
  LIMIT_STOP_ROUNDS,
  LIMIT_TEXT,
  LOOP_LIMIT,
  LOOP_TEXT,
  MACHINE_GONE_MS,
  MACHINE_MOVE_MS,
  MAX_AUTO_CONTINUE,
  MAX_INTERVENTIONS,
  NUDGE_MS,
  NUDGE_TEXT,
  TOOL_STALL_MS,
  isPromiseOnly,
  stepKey,
} from "../../agent/supervise";
import { actedOutward } from "../../agent/recovery";
import { ulid } from "../../core/ids";
import { linkStub } from "../env";
import type { SeqFrame } from "../link";
import { loadMachines, loadRunner } from "../store";
import { ACTIVE, QUEUE_MAX_MS, QUEUE_RETRY_MS, QUEUED, type RunRow, START_STUCK_MS, actsOf } from "./core";
import { DeskEvents } from "./events";
import { reportOf } from "./report";
import type { Restart } from "./runs";

/** A run that waits for its machine (盯 1.1) is not silent: no nudge, no stall. */
const PRESENT = "away_since IS NULL";
/**
 * 盯 1.4: since when a run is quiet. A step whose processes the daemon reports working (a long
 * build, a full test run) is not quiet, however long it takes.
 */
const QUIET = "max(last_event_at, CASE WHEN open_tools > 0 THEN ifnull(alive_at, 0) ELSE 0 END)";
const quietSince = (r: RunRow): number => Math.max(r.last_event_at, r.open_tools > 0 ? (r.alive_at ?? 0) : 0);
/** Open tool calls kept per run for loop detection (the oldest go first). */
const MAX_LOOP_CALLS = 32;

export abstract class DeskStall extends DeskEvents {
  // -------------------------------------------------------------------------
  // Stall detection
  // -------------------------------------------------------------------------

  protected async scheduleStall(): Promise<void> {
    const stall = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(${QUIET} + CASE WHEN open_tools > 0 THEN ${TOOL_STALL_MS - STALL_MS} ELSE 0 END) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT}`,
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
    // A run waiting for an approval is watched too: its time limit stands still (盯 1.8).
    const running = this.sql
      .exec<{ n: number }>(`SELECT count(*) AS n FROM runs WHERE ${ACTIVE} AND external = 0 AND (status IN ('running', 'waiting') OR away_since IS NOT NULL)`)
      .one().n;
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
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND paused_by IS NULL AND ${PRESENT} AND ${QUIET} <= CASE WHEN open_tools > 0 THEN ? ELSE ? END`,
        now - TOOL_STALL_MS,
        now - STALL_MS,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.external || r.ended_at !== null || r.stalled || r.paused_by || r.away_since !== null || r.status !== "running" || quietSince(r) > now - (r.open_tools > 0 ? TOOL_STALL_MS : STALL_MS)) return;
        await this.markStalled(r);
      });
    }
    await this.superviseRunning(now);
    await this.superviseLimits(now);
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
        await this.act(r, "nudge", "silent", (now - r.last_event_at) / 60_000);
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
      await this.act(r, "wait_machine", "machine_away");
      if (m) await this.note(r, TEXT.machineAway(m.name)).catch((err) => this.log.warn("away note failed", { runId: r.id, err }));
    }
    const removed = !m || m.revoked;
    if (!removed && now - r.away_since < MACHINE_MOVE_MS) return true;
    const away = r.away_since;
    if ((await this.canMove(r)) && (await this.restartElsewhere(r, removed ? "machine removed" : "machine away", { exclude: [r.machine_id] }, { count: false }))) {
      await this.act(r, "move", removed ? "machine_removed" : "machine_away", (now - away) / 60_000);
      if (m) await this.note(r, TEXT.machineMoved(m.name, Math.round((now - away) / 60_000))).catch((err) => this.log.warn("move note failed", { runId: r.id, err }));
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
    const awayMinutes = (now - r.away_since) / 60_000;
    this.sql.exec("UPDATE runs SET away_since = NULL, last_event_at = max(last_event_at, ?), nudged = 0 WHERE id = ?", now, r.id);
    r.away_since = null;
    await this.act(r, "machine_back", "machine_away", awayMinutes);
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
      if (await this.restartElsewhere(r, "machine lost")) {
        await this.act(r, "retry", "machine_lost");
        return false;
      }
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

  /** A tool call: counted, and kept open until its result (loops look at the output, 盯 1.11). */
  protected async watchTool(r: RunRow, itemId: string, title: string): Promise<void> {
    const calls = this.loopCalls(r);
    if (itemId !== "") {
      delete calls[itemId];
      calls[itemId] = title;
      const keys = Object.keys(calls);
      for (const k of keys.slice(0, Math.max(0, keys.length - MAX_LOOP_CALLS))) delete calls[k];
    }
    r.loop_calls = JSON.stringify(calls);
    this.sql.exec("UPDATE runs SET tool_calls = tool_calls + 1, loop_calls = ? WHERE id = ?", r.loop_calls, r.id);
    r.tool_calls += 1;
  }

  /**
   * The result of a tool call. The same step with the same output again and again is a loop:
   * one reminder per streak (盯 1.11: a step whose output changes, such as polling CI, is not).
   * A new successful step is progress (盯 1.6).
   */
  protected async watchResult(r: RunRow, itemId: string, ok: boolean, output: string): Promise<void> {
    const calls = this.loopCalls(r);
    const title = calls[itemId];
    if (title === undefined) {
      if (ok) this.addProgress(r);
      return;
    }
    delete calls[itemId];
    r.loop_calls = JSON.stringify(calls);
    const key = stepKey(title, output);
    const count = key !== "" && key === r.loop_key ? r.loop_count + 1 : 1;
    this.sql.exec("UPDATE runs SET loop_calls = ?, loop_key = ?, loop_count = ? WHERE id = ?", r.loop_calls, key, count, r.id);
    r.loop_key = key;
    r.loop_count = count;
    if (ok && count === 1) this.addProgress(r);
    if (count === LOOP_LIMIT && r.interventions < MAX_INTERVENTIONS) {
      this.sql.exec("UPDATE runs SET interventions = interventions + 1 WHERE id = ?", r.id);
      r.interventions += 1;
      await this.tell(r, LOOP_TEXT(title), "loop");
      await this.act(r, "loop", "same_step");
    }
  }

  private loopCalls(r: RunRow): Record<string, string> {
    try {
      const v = r.loop_calls ? (JSON.parse(r.loop_calls) as unknown) : null;
      return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, string>) : {};
    } catch {
      return {};
    }
  }

  private addProgress(r: RunRow): void {
    this.sql.exec("UPDATE runs SET progress = progress + 1 WHERE id = ?", r.id);
    r.progress += 1;
  }

  /** 盯 1.10: the piece of work has a work order (交代单): the run, or the run it follows or retries. */
  protected workOrdered(r: RunRow): boolean {
    let cur: RunRow | undefined = r;
    for (let i = 0; cur && i < 12; i++) {
      if (this.requestOf(cur)?.brief) return true;
      const up: string | null = cur.follow_up_of ?? cur.retry_of;
      cur = up ? this.run(up) : undefined;
    }
    return false;
  }

  /**
   * 盯 5.5: keeps one act of the desk with the run, with its reason (`BotRun.supervision`), and
   * tells clients unless `publish` is false (a status change follows).
   */
  protected async act(r: RunRow, action: string, reason: string, minutes?: number, publish = true): Promise<void> {
    const item: SuperviseAct = { at: Date.now(), action, reason, ...(minutes !== undefined && Number.isFinite(minutes) ? { minutes: Math.max(0, Math.round(minutes)) } : {}) };
    r.acts = JSON.stringify([...actsOf(r), item].slice(-MAX_SUPERVISE_ACTS));
    this.sql.exec("UPDATE runs SET acts = ? WHERE id = ?", r.acts, r.id);
    this.log.info("supervise act", { runId: r.id, action, reason, minutes: item.minutes });
    if (publish) await this.setStatus(r, r.status);
  }

  /** The CLI ended with a promise and no tool call: a follow-up run continues the session. */
  protected async continuePromise(r: RunRow, result: string): Promise<boolean> {
    if (r.kind === RUN_KIND_ASIDE || !isPromiseOnly(result, r.tool_calls, this.workOrdered(r)) || r.auto_continues >= MAX_AUTO_CONTINUE) return false;
    if (this.startBlocked(r.topic_id)) return false;
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
    await this.act(r, "continue", "promise");
    return true;
  }

  /**
   * Ends the run without a message and continues its work in a new run (盯 1.2): on the same
   * machine (`sameSession`), or leaving out the machines in `exclude`. The new run keeps the work
   * line, resumes the CLI session when it lands on the machine that has it, and starts with the
   * recovery note (agent/recovery.ts). It carries the count and the stall step; out of tries
   * (when it counts) it returns false. A run that leaves its machine is closed there: the old
   * copy is stopped as soon as that daemon is back, before anything else (盯 1.14).
   */
  protected async restartElsewhere(r: RunRow, why: string, how: Restart = {}, opts: { count?: boolean; step?: number; limitRounds?: number } = {}): Promise<boolean> {
    const count = opts.count ?? true;
    if (r.external || r.kind === RUN_KIND_ASIDE || (count && r.interventions >= MAX_INTERVENTIONS)) return false;
    const request = this.requestOf(r);
    if (!request) return false;
    if (r.machine_id) {
      const leaves = (how.exclude ?? []).includes(r.machine_id);
      try {
        // Leaving the machine: the run is closed there, so the next welcome leaves it out and
        // the daemon stops it first thing. Staying: a cancel, and the late frames still count.
        if (leaves) await linkStub(this.env, r.machine_id).closeRun(r.id);
        else await linkStub(this.env, r.machine_id).toRun({ t: "run.cancel", runId: r.id } as never);
      } catch (err) {
        this.log.warn("restart cancel failed", { runId: r.id, err });
      }
    }
    await this.closeProgress(r);
    await this.closeApprovalCard(r, "expired");
    this.sql.exec("UPDATE runs SET approval_id = NULL, paused_by = NULL, paused_at = NULL WHERE id = ?", r.id);
    await this.setStatus(r, "cancelled", { ended_at: Date.now(), summary: `restarted: ${why}` });
    const key = `restart:${ulid()}`;
    const ack = request.m.ackMessageId ?? (r.trigger_message_id.includes(":") ? undefined : r.trigger_message_id);
    // The run keeps its CLI session: a retry that lands on the machine that has it resumes it,
    // anywhere else it starts a new one with the full topic. Both get the recovery note.
    // The same session lives on its machine: the retry stays there (it waits when that is full).
    const keep: Restart = { ...how, why, ...(how.sameSession && r.machine_id ? { machine: r.machine_id } : {}) };
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `restart:${r.id}`, JSON.stringify(keep));
    // What people sent the old run that its CLI never took (a card answer given while the
    // computer was off): the new run carries it, as a follow-up run would have.
    const unread = this.sql
      .exec<{ message_id: string; author_id: string; body: string }>("SELECT message_id, author_id, body FROM inputs WHERE run_id = ? AND accepted = 0 ORDER BY at", r.id)
      .toArray();
    const carried: { authorId: string; body: string }[] = [];
    for (const x of unread) if (await this.answerStartsRun(r, x.message_id)) carried.push({ authorId: x.author_id, body: x.body });
    this.sql.exec("UPDATE inputs SET accepted = 1 WHERE run_id = ? AND accepted = 0", r.id);
    await this.start({ ...request.m, messageId: key, ackMessageId: ack, mentioned: true }, undefined, [...request.followUps, ...carried], r.id, request.brief);
    const next = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (next) {
      this.sql.exec(
        "UPDATE runs SET interventions = ?, stall_step = ?, nudge_sent = ?, limit_rounds = ? WHERE id = ?",
        r.interventions + (count ? 1 : 0),
        opts.step ?? r.stall_step,
        r.nudge_sent,
        opts.limitRounds ?? r.limit_rounds,
        next.id,
      );
    }
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
    const silent = (Date.now() - quietSince(r)) / 60_000;
    if (r.stall_step < 1 && (await this.restartElsewhere(r, "stalled", { sameSession: true }, { step: 1 }))) {
      await this.act(r, "retry", "silent", silent);
      return;
    }
    // 机 2.6: work that already reached outside (a push, a pull request, a card, 产出) is not
    // started over on another machine: the retry above resumed and verified it; now people decide.
    if (r.stall_step < 2 && r.machine_id && r.interventions < MAX_INTERVENTIONS && (await this.canMove(r)) && !actedOutward(await this.recoveryFacts(r.id, "stalled"))) {
      if (await this.restartElsewhere(r, "stalled", { exclude: [r.machine_id] }, { step: 2 })) {
        await this.act(r, "move", "silent", silent);
        return;
      }
    }
    this.sql.exec("UPDATE runs SET stalled = 1 WHERE id = ?", r.id);
    r.stalled = 1;
    await this.act(r, "gave_up", "silent", silent, false);
    // 盯 1.12: the true silence, and the tries that really happened.
    const minutes = Math.max(1, Math.round((Date.now() - quietSince(r)) / 60_000));
    const tries = [...(r.nudge_sent ? ["提醒"] : []), ...(r.stall_step >= 1 ? ["打断后重来"] : []), ...(r.stall_step >= 2 ? ["换电脑重来"] : [])];
    try {
      await this.note(r, tries.length > 0 ? TEXT.gaveUp(minutes, tries) : TEXT.stalled(minutes));
    } catch (err) {
      this.log.warn("stall note failed", { runId: r.id, err });
    }
    await this.setStatus(r, "blocked");
    await this.topic(r, "blocked");
  }

  // -------------------------------------------------------------------------
  // 盯 1.4: a long step whose processes work is not silent
  // -------------------------------------------------------------------------

  /** MachineLink: the daemon's heartbeat says a step of the run is working (`RunCursor.active`). */
  async runAlive(runId: string, machineId: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      // Only a step the CLI waits for: a CLI that is quiet between steps is still nudged.
      if (!r || r.ended_at !== null || r.machine_id !== machineId || r.open_tools === 0) return;
      const now = Date.now();
      this.sql.exec("UPDATE runs SET alive_at = ? WHERE id = ?", now, r.id);
    });
  }

  // -------------------------------------------------------------------------
  // 盯 1.6, 1.8: the time limit saves and goes on; waits do not count
  // -------------------------------------------------------------------------

  /**
   * Counts each run's work time (only while it runs: not while it waits for an approval, is
   * paused, waits for its machine or is marked stalled), tells a run at its limit to save its
   * state, and interrupts one that is still going `LIMIT_GRACE_MS` later.
   */
  protected async superviseLimits(now: number): Promise<void> {
    const rows = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND machine_id IS NOT NULL AND (kind IS NULL OR kind != '${RUN_KIND_ASIDE}') AND status IN ('running', 'waiting', 'blocked')`,
      )
      .toArray();
    for (const { id } of rows) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || !r.machine_id) return;
        const counting = r.status === "running" && !r.approval_id && !r.paused_by && r.away_since === null && !r.stalled;
        const since = r.worked_at ?? r.started_at ?? now;
        const worked = r.worked_ms + (counting ? Math.max(0, now - since) : 0);
        this.sql.exec("UPDATE runs SET worked_ms = ?, worked_at = ? WHERE id = ?", worked, now, r.id);
        r.worked_ms = worked;
        r.worked_at = now;
        if (r.limit_at === null) {
          if (counting && worked >= (r.limit_ms ?? MAX_RUN_MS)) await this.limitReached(r, now);
        } else if (now - r.limit_at >= LIMIT_GRACE_MS && !r.approval_id && !r.paused_by && r.away_since === null) {
          // Told to save and end its turn, and still going: interrupt it; it goes on in its session.
          await this.limitGoOn(r, false);
        }
      });
    }
  }

  /** The run worked its limit: it is told to save its state and end its turn. */
  protected async limitReached(r: RunRow, now: number): Promise<void> {
    this.sql.exec("UPDATE runs SET limit_at = ? WHERE id = ?", now, r.id);
    r.limit_at = now;
    const minutes = Math.round(r.worked_ms / 60_000);
    await this.tell(r, LIMIT_TEXT(minutes), "limit");
    await this.act(r, "limit", "time_limit", minutes);
  }

  /**
   * The run ended after the time limit: it saved its state (the CLI took the limit message and
   * ended its turn without a final report), or the daemon's backstop stopped it. True when the
   * desk went on with the work (or stopped it); false: the run ended normally.
   */
  protected async limitEnded(r: RunRow, f: Extract<SeqFrame, { t: "run.done" }>): Promise<boolean> {
    const backstop = f.status === "timeout" || (f.status === "cancelled" && /maxDurationMs/.test(f.error ?? ""));
    const report = reportOf(r);
    const saved =
      f.status === "succeeded" && r.limit_at !== null && this.limitTaken(r) && !(report && report.at >= r.limit_at) && !this.handedOverOf(r.id);
    if (!backstop && !saved) return false;
    return this.limitGoOn(r, true);
  }

  /** The CLI took the desk's limit message (input.accepted of a `desk-limit-…` input). */
  private limitTaken(r: RunRow): boolean {
    return (
      this.sql
        .exec("SELECT 1 FROM run_log WHERE run_id = ? AND frame LIKE '%\"input.accepted\"%' AND frame LIKE '%\"desk-limit-%' LIMIT 1", r.id)
        .toArray().length > 0
    );
  }

  /**
   * A new turn in the same session goes on with the work. After `LIMIT_STOP_ROUNDS` turns in a
   * row at the limit without progress, the work stops and people are told once. `ended`: the run
   * already ended on its machine.
   */
  protected async limitGoOn(r: RunRow, ended: boolean): Promise<boolean> {
    const rounds = r.progress > 0 ? 0 : r.limit_rounds + 1;
    const minutes = r.worked_ms / 60_000;
    const total = this.retryChain(r.id).reduce((n, x) => n + (x.id === r.id ? r.worked_ms : x.worked_ms), 0) / 60_000;
    if (rounds < LIMIT_STOP_ROUNDS && r.kind !== RUN_KIND_ASIDE) {
      if (await this.restartElsewhere(r, "time limit", { sameSession: true }, { count: false, limitRounds: rounds })) {
        await this.act(r, "limit_continue", rounds > 0 ? "no_progress" : "time_limit", minutes);
        return true;
      }
    }
    if (!ended && r.machine_id) {
      await linkStub(this.env, r.machine_id)
        .toRun({ t: "run.cancel", runId: r.id } as never)
        .catch((err) => this.log.warn("limit cancel failed", { runId: r.id, err }));
    }
    const stuck = rounds >= LIMIT_STOP_ROUNDS;
    await this.act(r, "limit_stop", stuck ? "no_progress" : "time_limit", minutes, false);
    const text = stuck ? TEXT.limitStopped(workTime(total), rounds) : TEXT.timeUp(workTime(total));
    await this.end(r, "blocked", text, "blocked", text);
    return true;
  }
}
