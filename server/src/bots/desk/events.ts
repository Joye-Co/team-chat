// BotDesk events: the run log from MachineLink, its effects (progress message, plan, approval
// cards), and the end of a run (result, status, follow-up runs).
import type { Card, RunStatus, TopicStatus } from "@chat/protocol";
import { ApiError, unwrap } from "../../core/errors";
import { MAX_RESULT_CHARS, Progress, TEXT, cut, firstLine } from "../dispatch";
import { linkStub } from "../env";
import type { SeqFrame } from "../link";
import { MAX_FOLLOW_UPS, type RunRow, isStallStatus, sanitizePlan } from "./core";
import { DeskRuns } from "./runs";

export abstract class DeskEvents extends DeskRuns {
  /** Stores run frames idempotently by (runId, seq) and applies their effects. */
  async ingest(runId: string, frames: SeqFrame[]): Promise<{ lastSeq: number }> {
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "unknown run");
      for (const f of [...frames].sort((a, b) => a.seq - b.seq)) {
        this.sql.exec(
          "INSERT OR IGNORE INTO run_log (run_id, seq, at, frame) VALUES (?, ?, ?, ?)",
          runId,
          f.seq,
          f.at,
          JSON.stringify(f),
        );
      }
      const top = Math.max(r.last_seq, ...frames.map((f) => f.seq));
      this.sql.exec("UPDATE runs SET last_seq = ? WHERE id = ?", top, runId);
      // A late run.session still records its upload.
      if (r.ended_at !== null) return { lastSeq: top };
      // Stored frames whose effects did not apply yet (also after a failed or interrupted apply).
      const pending = this.sql
        .exec<{ seq: number; frame: string }>("SELECT seq, frame FROM run_log WHERE run_id = ? AND seq > ? ORDER BY seq", runId, r.applied_seq)
        .toArray()
        .map((x) => JSON.parse(x.frame) as SeqFrame);
      if (pending.length === 0) return { lastSeq: top };
      await this.apply(r, pending);
      this.sql.exec("UPDATE runs SET applied_seq = max(applied_seq, ?) WHERE id = ?", pending[pending.length - 1].seq, runId);
      return { lastSeq: top };
    });
  }

  protected async apply(r: RunRow, frames: SeqFrame[]): Promise<void> {
    const progress = new Progress(r.progress_text, JSON.parse(r.items) as Record<string, string>, r.progress_full === 1);
    const now = Date.now();
    let done: Extract<SeqFrame, { t: "run.done" }> | null = null;
    let planChanged = false;
    // The daemon's own stall reports are not progress: they must not restart the stall clock.
    const progressed = frames.some((f) => f.t !== "run.event" || !isStallStatus(f.event));
    const stallReported = frames.some((f) => f.t === "run.event" && isStallStatus(f.event) && (f.event as { state?: unknown }).state === "stalled");
    if (!progressed) {
      if (stallReported) await this.markStalled(r);
      return;
    }
    this.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", now, r.id);
    r.last_event_at = now;
    if (r.stalled) {
      this.sql.exec("UPDATE runs SET stalled = 0 WHERE id = ?", r.id);
      r.stalled = 0;
      await this.setStatus(r, r.approval_id ? "waiting" : "running");
      await this.topic(r, r.approval_id ? "waiting_answer" : "in_progress", r.approval_id ? [r.requested_by] : []);
    } else if (r.status === "created") {
      await this.setStatus(r, "running", { started_at: now });
    }
    for (const f of frames) {
      if (f.t === "run.done") {
        done = f;
        continue;
      }
      if (f.t !== "run.event") continue;
      const ev = f.event as Record<string, unknown> & { type: string };
      switch (ev.type) {
        case "message.delta":
          progress.delta(String(ev.itemId), String(ev.text ?? ""));
          break;
        case "message":
          if (ev.role === "assistant") progress.message(String(ev.itemId), String(ev.text ?? ""));
          break;
        case "tool.call":
          progress.tool(String(ev.title || ev.tool || ""));
          break;
        case "approval.request": {
          await this.flush(r, progress);
          await this.askApproval(r, String(ev.approvalId), String(ev.title ?? ""), ev.detail ? String(ev.detail) : undefined);
          break;
        }
        case "status":
          // A stall report among real progress: the progress wins.
          break;
        case "approval.resolved":
          if (r.approval_id === ev.approvalId) {
            this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
            r.approval_id = null;
            await this.setStatus(r, "running");
            await this.topic(r, "in_progress");
          }
          break;
        case "input.accepted":
          this.sql.exec("UPDATE inputs SET accepted = 1 WHERE id = ? AND run_id = ?", String(ev.inputId), r.id);
          break;
        case "plan": {
          // The link reads events loosely (newer daemons): keep what fits the plan rules.
          const plan = sanitizePlan(ev.items);
          if (plan && this.storePlan(r, plan)) planChanged = true;
          break;
        }
        default:
          break;
      }
    }
    await this.flush(r, progress);
    // The final run.updated of `finish` carries the plan too.
    if (planChanged && !done) await this.setStatus(r, r.status);
    if (done) await this.finish(r, done);
    else await this.scheduleStall();
  }

  /** Streams the new progress text into the run's one streaming message. */
  protected async flush(r: RunRow, progress: Progress): Promise<void> {
    const append = progress.take();
    this.sql.exec(
      "UPDATE runs SET progress_text = ?, progress_full = ?, items = ? WHERE id = ?",
      progress.text,
      progress.full ? 1 : 0,
      JSON.stringify(progress.items),
      r.id,
    );
    r.progress_text = progress.text;
    if (append === "") return;
    try {
      if (!r.progress_message_id) {
        const id = await this.post(r, `run:${r.id}:progress`, progress.text, { streaming: true });
        this.sql.exec("UPDATE runs SET progress_message_id = ? WHERE id = ?", id, r.id);
        r.progress_message_id = id;
      } else {
        unwrap(await this.room(r).delta(r.group_id, this.actor(), r.progress_message_id, append));
      }
    } catch (err) {
      this.log.warn("progress stream failed", { runId: r.id, err });
    }
  }

  /** Ends the streaming progress message with its full text. */
  protected async closeProgress(r: RunRow): Promise<void> {
    if (!r.progress_message_id) return;
    if (r.progress_text.trim() === "" && r.progress_full === 0) {
      // Nothing but the answer was streamed: the result message alone says it.
      try {
        unwrap(await this.room(r).remove(r.group_id, this.actor(), r.progress_message_id));
      } catch (err) {
        this.log.warn("progress remove failed", { runId: r.id, err });
      }
      return;
    }
    try {
      unwrap(
        await this.room(r).edit(r.group_id, this.actor(), {
          t: "edit",
          rid: "bot",
          groupId: r.group_id,
          messageId: r.progress_message_id,
          body: r.progress_text.trimEnd() || TEXT.progress,
          streaming: false,
        } as never),
      );
    } catch (err) {
      this.log.warn("progress close failed", { runId: r.id, err });
    }
  }

  protected async askApproval(r: RunRow, approvalId: string, title: string, detail?: string): Promise<void> {
    this.sql.exec("UPDATE runs SET approval_id = ? WHERE id = ?", approvalId, r.id);
    r.approval_id = approvalId;
    const body = TEXT.approval(cut(title, 500), detail ? cut(detail, 4000) : undefined);
    const card: Card = {
      v: 1,
      title: cut(`需要你确认：${title}`, 200),
      tone: "warning",
      blocks: detail ? [{ type: "code", text: cut(detail, 8000) }] : [],
      actions: [
        { id: cut(`approve:${approvalId}`, 64), label: TEXT.approve, style: "primary" },
        { id: cut(`deny:${approvalId}`, 64), label: TEXT.deny, style: "danger" },
      ],
    };
    const cmid = `run:${r.id}:approval:${cut(approvalId, 24)}`;
    try {
      await this.post(r, cmid, body, { card, mentions: [r.requested_by] });
    } catch (err) {
      // A card the room rejects (for example an action id that is too long) falls back to text.
      this.log.warn("approval card failed", { runId: r.id, err });
      try {
        await this.post(r, `${cmid}:text`, body, { mentions: [r.requested_by] });
      } catch (err2) {
        // Nobody can be asked (for example the bot left the group): the answer is no.
        this.log.warn("approval post failed, denying", { runId: r.id, err: err2 });
        this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
        r.approval_id = null;
        if (r.machine_id) {
          await linkStub(this.env, r.machine_id)
            .toRun({ t: "run.approval", runId: r.id, approvalId, decision: "deny" } as never)
            .catch((e) => this.log.warn("approval deny failed", { runId: r.id, err: e }));
        }
        return;
      }
    }
    await this.setStatus(r, "waiting");
    await this.topic(r, "waiting_answer", [r.requested_by]);
  }

  protected async finish(r: RunRow, f: Extract<SeqFrame, { t: "run.done" }>): Promise<void> {
    if (f.status === "succeeded") {
      const result = (f.resultText ?? "").trim() || (f.summary ?? "").trim() || TEXT.done;
      this.dropResultFromProgress(r, result);
      await this.end(r, "done", cut(result, MAX_RESULT_CHARS), "done", firstLine(result));
    } else if (f.status === "cancelled") {
      await this.end(r, "cancelled", TEXT.cancelled, "blocked", TEXT.cancelled);
    } else {
      const why = firstLine(f.error ?? f.summary ?? "", 300);
      await this.end(r, "failed", TEXT.failed(why), "blocked", cut(TEXT.failed(why), 280));
    }
    // Follow-ups the CLI never took start a new run with the topic context.
    const left = this.sql
      .exec<{ message_id: string; author_id: string; body: string }>(
        "SELECT message_id, author_id, body FROM inputs WHERE run_id = ? AND accepted = 0 ORDER BY at",
        r.id,
      )
      .toArray();
    if (left.length === 0) return;
    if (this.followUpDepth(r) >= MAX_FOLLOW_UPS) {
      try {
        await this.note(r, TEXT.followUpLimit);
      } catch (err) {
        this.log.warn("follow-up note failed", { runId: r.id, err });
      }
      return;
    }
    const last = left[left.length - 1];
    try {
      await this.start(
        { groupId: r.group_id, topicId: r.topic_id, messageId: last.message_id, authorId: last.author_id, body: last.body, mentioned: true },
        r,
        left.slice(0, -1).map((x) => ({ authorId: x.author_id, body: x.body })),
      );
    } catch (err) {
      this.log.error("follow-up start failed", { runId: r.id, err });
    }
  }

  /** How many runs in a row before `r` were follow-ups. */
  protected followUpDepth(r: RunRow): number {
    let depth = 0;
    let parent = r.follow_up_of;
    while (parent && depth < MAX_FOLLOW_UPS) {
      depth++;
      parent = this.run(parent)?.follow_up_of ?? null;
    }
    return depth;
  }

  /** The CLI's last assistant message is also the result: keep it out of the progress text. */
  protected dropResultFromProgress(r: RunRow, result: string): void {
    const text = r.progress_text.trimEnd();
    if (!result || !text.endsWith(result)) return;
    const rest = text.slice(0, text.length - result.length).trimEnd();
    r.progress_text = rest;
    this.sql.exec("UPDATE runs SET progress_text = ? WHERE id = ?", rest, r.id);
  }

  /** Final state: closes the progress message, posts `body`, sets the run and topic status. */
  protected async end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string): Promise<void> {
    const now = Date.now();
    await this.closeProgress(r);
    this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
    r.approval_id = null;
    try {
      await this.post(r, `run:${r.id}:result`, body);
    } catch (err) {
      this.log.warn("result post failed", { runId: r.id, err });
    }
    await this.setStatus(r, status, { ended_at: now, summary: cut(summary, 280) || null });
    await this.topic(r, topicStatus);
  }
}
