// BotDesk stall detection: the alarm that marks silent runs blocked and fails runs whose start
// never reached a machine.
import { STALL_MS, TEXT } from "../dispatch";
import { ACTIVE, type RunRow, START_STUCK_MS } from "./core";
import { DeskEvents } from "./events";

export abstract class DeskStall extends DeskEvents {
  // -------------------------------------------------------------------------
  // Stall detection
  // -------------------------------------------------------------------------

  protected async scheduleStall(): Promise<void> {
    const stall = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL`,
      )
      .one().at;
    const stuck = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(created_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL`,
      )
      .one().at;
    const times = [stall === null ? null : stall + STALL_MS, stuck === null ? null : stuck + START_STUCK_MS].filter(
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
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND last_event_at <= ?`,
        now - STALL_MS,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.external || r.ended_at !== null || r.stalled || r.status !== "running" || r.last_event_at > now - STALL_MS) return;
        await this.markStalled(r);
      });
    }
    // Runs whose start broke before they reached a machine (for example a reset mid-start).
    const stuck = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL AND created_at <= ?`,
        now - START_STUCK_MS,
      )
      .toArray();
    for (const { id } of stuck) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id || r.status !== "created") return;
        await this.end(r, "failed", TEXT.failed(""), "blocked", TEXT.failed(""));
      });
    }
    await this.scheduleStall();
  }

  /** No progress: one note, the run and the topic show blocked until events come again. */
  protected async markStalled(r: RunRow): Promise<void> {
    if (r.stalled || r.ended_at !== null || r.approval_id) return;
    this.sql.exec("UPDATE runs SET stalled = 1 WHERE id = ?", r.id);
    r.stalled = 1;
    try {
      await this.note(r, TEXT.stalled);
    } catch (err) {
      this.log.warn("stall note failed", { runId: r.id, err });
    }
    await this.setStatus(r, "blocked");
    await this.topic(r, "blocked");
  }
}
