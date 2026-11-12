// BotDesk events: the run log from MachineLink, its effects (progress message, plan, approval
// cards), and the end of a run (result, status, follow-up runs).
import { type Card, RUN_KIND_ASIDE, type RunStatus, type TopicStatus } from "@chat/protocol";
import { ApiError, unwrap } from "../../core/errors";
import { MAX_RESULT_CHARS, Progress, TEXT, cut, firstLine, readableError, stripAcceptance } from "../dispatch";
import { linkStub } from "../env";
import type { SeqFrame } from "../link";
import { type RunProof, type TestCounts, doneCard, doneCardV1, hasProof, mergePrs, prRefs, testsFromLog } from "../proof";
import { needsReply, parseReport, reportOf } from "./report";
import { TOPIC_LINK_RPC } from "../../integrations/links";
import { stamp } from "../../context/render";
import { MAX_FOLLOW_UPS, type RunRow, codeOf, isStallStatus, sanitizeAcceptance, sanitizePlan, sanitizeUsage, sessionIdOf } from "./core";
import { DeskRuns, type RunRequest } from "./runs";
import { briefText } from "../../agent/frontdesk";
import { retroAfterRun } from "../../memory/retro";

/** Room RPC of the cards feature: a run's CLI took the answer with this seq (`{ seq, runId, inputId }`). */
export const CARD_ANSWER_TAKEN_RPC = "cards.answerTaken";

export abstract class DeskEvents extends DeskRuns {
  /** 分出去 (delegate.ts): the result of a part goes back to the member that gave it out. */
  protected abstract reportPart(r: RunRow, ok: boolean, result: string): Promise<void>;
  /** 盯活 (stall.ts): a tool call of the run (loops). */
  protected abstract watchTool(r: RunRow, title: string): Promise<void>;
  /** 盯活 (stall.ts): continues a run that only promised work; true when it did. */
  protected abstract continuePromise(r: RunRow, result: string): Promise<boolean>;
  /** 盯 1.1 (stall.ts): the machine of a run that waited for it is back. */
  protected abstract machineBack(r: RunRow, say?: boolean): Promise<void>;

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
      // A late run.session still records its upload; a late run.done (the run was stopped
      // here first) still gives the run its usage and time.
      if (r.ended_at !== null) {
        const late = frames.find((f): f is Extract<SeqFrame, { t: "run.done" }> => f.t === "run.done");
        if (late) this.storeSession(r, late.sessionId);
        const usage = !!late && r.usage === null && r.duration_ms === null && this.storeUsage(r, late);
        // design-relay R5: and where it left its code.
        const code = !!late && r.code === null && this.storeCode(r, late.code);
        if (usage || code) await this.setStatus(r, r.status);
        return { lastSeq: top };
      }
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

  private openTools(r: RunRow, n: number): void {
    const next = Math.max(0, n);
    if (next === r.open_tools) return;
    this.sql.exec("UPDATE runs SET open_tools = ? WHERE id = ?", next, r.id);
    r.open_tools = next;
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
      // 盯 1.3: the daemon's stall report is a signal only. The desk's own order (nudge, wait,
      // restart in the same session, another machine) decides; the report never restarts a run.
      if (stallReported) this.log.info("supervise", { runId: r.id, why: "daemon reports stalled", silentMs: now - r.last_event_at });
      return;
    }
    // Frames from a machine the run waited for: it is back (盯 1.1).
    if (r.away_since !== null) await this.machineBack(r);
    this.sql.exec("UPDATE runs SET last_event_at = ?, nudged = 0 WHERE id = ?", now, r.id);
    r.last_event_at = now;
    r.nudged = 0;
    if (r.stalled) {
      this.sql.exec("UPDATE runs SET stalled = 0 WHERE id = ?", r.id);
      r.stalled = 0;
      await this.setStatus(r, r.approval_id ? "waiting" : "running");
      await this.topic(r, r.approval_id ? "waiting_answer" : "in_progress", r.approval_id ? await this.approvers(r, this.approvalKind(r)) : []);
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
          await this.watchTool(r, String(ev.title || ev.tool || ""));
          this.openTools(r, r.open_tools + 1);
          break;
        case "tool.result":
          this.openTools(r, r.open_tools - 1);
          break;
        case "approval.request": {
          await this.flush(r, progress);
          // Feature "gates": the daemon stopped a command of a gated kind.
          const gate = typeof ev.kind === "string" && ev.kind !== "" ? { kind: cut(ev.kind, 32), command: typeof ev.command === "string" ? ev.command : undefined } : undefined;
          await this.askApproval(r, String(ev.approvalId), String(ev.title ?? ""), ev.detail ? String(ev.detail) : undefined, gate);
          break;
        }
        case "status":
          // A stall report among real progress: the progress wins.
          break;
        case "session":
          this.storeSession(r, ev.sessionId);
          break;
        case "approval.resolved":
          if (r.approval_id === ev.approvalId) {
            this.sql.exec("DELETE FROM meta WHERE key = ?", `approval:${r.id}`);
            this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
            r.approval_id = null;
            await this.setStatus(r, "running");
            await this.topic(r, "in_progress");
          }
          break;
        case "turn.started":
        case "turn.completed": {
          const inTurn = ev.type === "turn.started" ? 1 : 0;
          if (r.in_turn !== inTurn) {
            this.sql.exec("UPDATE runs SET in_turn = ? WHERE id = ?", inTurn, r.id);
            r.in_turn = inTurn;
          }
          if (inTurn === 0) this.openTools(r, 0);
          break;
        }
        case "output":
          // chat-run (1.1): post or patch a card, resolve an event, push or answer a 产出.
          await this.output(r, ev as { outputId?: unknown; op?: unknown; body?: unknown }, f);
          break;
        case "input.accepted": {
          const taken = this.sql.exec("UPDATE inputs SET accepted = 1 WHERE id = ? AND run_id = ?", String(ev.inputId), r.id).rowsWritten;
          // 接 4.2: a person's message reached the run after its report: that report no longer
          // says how the work ends (the run reports again, or ends normally as done).
          if (taken > 0 && r.report) {
            r.report = null;
            this.sql.exec("UPDATE runs SET report = NULL WHERE id = ?", r.id);
          }
          await this.answerTaken(r, String(ev.inputId));
          break;
        }
        case "plan": {
          // The link reads events loosely (newer daemons): keep what fits the plan rules.
          const plan = sanitizePlan(ev.items);
          if (plan && this.storePlan(r, plan)) planChanged = true;
          break;
        }
        case "acceptance": {
          const items = sanitizeAcceptance(ev.items);
          if (items && this.storeAcceptance(r, items)) planChanged = true;
          break;
        }
        case "paused":
          // The daemon took a pause: count it (at most the pauses sent).
          if (ev.paused === true && r.pause_seen < r.pause_gen) {
            this.sql.exec("UPDATE runs SET pause_seen = pause_seen + 1 WHERE id = ?", r.id);
            r.pause_seen += 1;
          }
          // The CLI goes on without a resume from here (for example the daemon restarted it):
          // the run is no longer taken over. A `paused: false` from before the daemon took the
          // latest pause (the answer to an earlier resume) is stale and keeps pausedBy.
          if (ev.paused === false && r.paused_by && r.pause_seen >= r.pause_gen) {
            this.sql.exec("UPDATE runs SET paused_by = NULL, paused_at = NULL WHERE id = ?", r.id);
            r.paused_by = null;
            r.paused_at = null;
            planChanged = true;
          }
          break;
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

  /**
   * Keeps the progress text. Since 1.1 progress lives in the run (plan, run log) and nothing is
   * streamed into the topic; only a run that already had a streaming message (started before
   * the upgrade) keeps it up to date until it ends.
   */
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
    if (append === "" || !r.progress_message_id) return;
    try {
      unwrap(await this.room(r).delta(r.group_id, this.actor(), r.progress_message_id, append));
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

  /**
   * The approval card changes in place once it is answered or no longer needed (DESIGN.md §7.10):
   * the buttons go, the title says the outcome and one line says who and when. Never throws.
   */
  protected async closeApprovalCard(r: RunRow, outcome: "approve" | "approve_run" | "deny" | "expired", actorId?: string): Promise<void> {
    const raw = this.meta(`approvalCard:${r.id}`);
    if (!raw) return;
    this.sql.exec("DELETE FROM meta WHERE key = ?", `approvalCard:${r.id}`);
    try {
      const saved = JSON.parse(raw) as { messageId?: string; shown?: string; card?: Card };
      // Only a posted card changes (not the text form, nor a card that was never posted).
      if (!saved.messageId || !saved.card || saved.shown === undefined) return;
      const who = actorId ? ((await this.names([actorId])).get(actorId) ?? "") : "";
      const text = TEXT.approvalClosed[outcome];
      const line = text.line(who, stamp(Date.now()).slice(-5));
      const card: Card = {
        ...saved.card,
        title: cut(text.title(saved.shown), 200),
        tone: outcome === "deny" ? "danger" : outcome === "expired" ? "neutral" : "success",
        blocks: [...(saved.card.blocks ?? []), { type: "text", text: line }],
        actions: [],
      };
      unwrap(
        await this.room(r).edit(r.group_id, this.actor(), {
          t: "edit",
          rid: "bot",
          groupId: r.group_id,
          messageId: saved.messageId,
          body: `${card.title}\n\n${line}`,
          card,
        } as never),
      );
    } catch (err) {
      this.log.warn("approval card close failed", { runId: r.id, err });
    }
  }

  /**
   * Posts the approval card (and its text form) to the approver. `gate` (feature "gates"): the
   * daemon stopped a command of that kind; the card names the kind, shows the command, goes to
   * the kind's approver and offers 这件事里都同意.
   */
  protected async askApproval(r: RunRow, approvalId: string, title: string, detail?: string, gate?: { kind: string; command?: string }): Promise<void> {
    this.sql.exec("UPDATE runs SET approval_id = ? WHERE id = ?", approvalId, r.id);
    r.approval_id = approvalId;
    if (gate) this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `approval:${r.id}`, JSON.stringify({ approvalId, kind: gate.kind }));
    else this.sql.exec("DELETE FROM meta WHERE key = ?", `approval:${r.id}`);
    const shown = gate ? (TEXT.gateKinds[gate.kind] ?? title) : title;
    const what = gate ? (gate.command ?? detail) : detail;
    // The approvers; with none among the owners, the requester (and the card says why).
    const { ids: askWho, noOwner } = await this.approversOrRequester(r, gate?.kind);
    const cardMeta = (messageId?: string, posted?: { shown: string; card: Card }) =>
      this.sql.exec(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)",
        `approvalCard:${r.id}`,
        JSON.stringify({ approvalId, noOwner, ...(messageId ? { messageId } : {}), ...(posted ?? {}) }),
      );
    cardMeta();
    const asked = gate ? TEXT.approvalGate(cut(shown, 500), what ? cut(what, 4000) : undefined) : TEXT.approval(cut(title, 500), detail ? cut(detail, 4000) : undefined);
    const body = noOwner ? `${TEXT.approvalNoOwner}\n\n${asked}` : asked;
    const card: Card = {
      v: 1,
      title: cut(`需要你确认：${shown}`, 200),
      tone: "warning",
      blocks: [...(noOwner ? [{ type: "text" as const, text: TEXT.approvalNoOwner }] : []), ...(what ? [{ type: "code" as const, text: cut(what, 8000) }] : [])],
      actions: [
        { id: cut(`approve:${approvalId}`, 64), label: TEXT.approve, style: "primary" },
        ...(gate ? [{ id: cut(`approveall:${approvalId}`, 64), label: TEXT.approveRun, confirm: TEXT.approveRunConfirm }] : []),
        { id: cut(`deny:${approvalId}`, 64), label: TEXT.deny, style: "danger" },
      ],
    };
    const cmid = `run:${r.id}:approval:${cut(approvalId, 24)}`;
    try {
      // A reply that quotes this message answers it (approvalDecision); the card changes in
      // place once answered (closeApprovalCard).
      cardMeta(await this.post(r, cmid, body, { card, mentions: askWho }), { shown: cut(shown, 180), card });
    } catch (err) {
      // A card the room rejects (for example an action id that is too long) falls back to text.
      this.log.warn("approval card failed", { runId: r.id, err });
      try {
        cardMeta(await this.post(r, `${cmid}:text`, body, { mentions: askWho }));
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
    await this.topic(r, "waiting_answer", askWho);
  }

  /**
   * The CLI took a card answer: the member started acting on it, so the answerer can no longer
   * retract it (INTERACT §3.6). Tells the room through `cards.answerTaken` (the cards feature's
   * RPC; a room without it answers not_found, which changes nothing).
   */
  protected async answerTaken(r: RunRow, inputId: string): Promise<void> {
    const row = this.sql.exec<{ message_id: string }>("SELECT message_id FROM inputs WHERE id = ? AND run_id = ?", inputId, r.id).toArray()[0];
    const m = row ? /^answer:[^:]+:(\d+)$/.exec(row.message_id) : null;
    if (!m) return;
    try {
      await this.room(r).rpc(r.group_id, CARD_ANSWER_TAKEN_RPC, this.actor(), { seq: Number(m[1]), runId: r.id, inputId });
    } catch (err) {
      this.log.warn("answer taken failed", { runId: r.id, err });
    }
  }

  // cards2 (F5): the retract window of a card answer, through the room's `cards.answerTaken`.
  /**
   * False when the answerer took answer `seq` back. With `close`, the answer starts a run now, so
   * the room closes its retract window (INTERACT §3.6, "the member starts acting"). A room
   * without the RPC lets the answer stand.
   */
  protected async answerStands(groupId: string, seq: number, close: boolean, liveAsk = false): Promise<boolean> {
    try {
      const res = unwrap(await this.room({ group_id: groupId }).rpc(groupId, CARD_ANSWER_TAKEN_RPC, this.actor(), { seq, close, liveAsk })) as {
        retracted?: boolean;
        askOpen?: boolean;
      };
      // With `liveAsk`, an answer whose ask was closed, expired or superseded starts no run.
      return res?.retracted !== true && !(liveAsk && res?.askOpen === false);
    } catch (err) {
      this.log.warn("answer check failed", { groupId, seq, err });
      return true;
    }
  }

  /** An input a follow-up run starts with: an answer (`answer:{groupId}:{seq}`) closes its window. */
  protected async answerStartsRun(r: RunRow, messageId: string): Promise<boolean> {
    const m = /^answer:[^:]+:(\d+)$/.exec(messageId);
    return m ? this.answerStands(r.group_id, Number(m[1]), true) : true;
  }

  /** Keeps `run.done` usage and time with the run. Returns false when nothing changed. */
  protected storeUsage(r: RunRow, f: Extract<SeqFrame, { t: "run.done" }>): boolean {
    const usage = sanitizeUsage(f.usage);
    const duration = Number.isInteger(f.durationMs) && f.durationMs >= 0 ? f.durationMs : null;
    if (!usage && duration === null) return false;
    r.usage = usage ? JSON.stringify(usage) : null;
    r.duration_ms = duration;
    this.sql.exec("UPDATE runs SET usage = ?, duration_ms = ? WHERE id = ?", r.usage, r.duration_ms, r.id);
    return true;
  }

  /** Keeps where the run left its code (`run.done.code`, design-relay R5). False when not known. */
  protected storeCode(r: RunRow, raw: unknown): boolean {
    const code = codeOf(raw);
    if (!code) return false;
    r.code = JSON.stringify(code);
    this.sql.exec("UPDATE runs SET code = ? WHERE id = ?", r.code, r.id);
    return true;
  }

  /** Keeps the CLI session of the run for the topic's next run (resume). */
  protected storeSession(r: RunRow, sessionId: unknown): void {
    const sid = sessionIdOf(sessionId);
    if (!sid || sid === r.session_id) return;
    r.session_id = sid;
    this.sql.exec("UPDATE runs SET session_id = ? WHERE id = ?", sid, r.id);
  }

  protected async finish(r: RunRow, f: Extract<SeqFrame, { t: "run.done" }>): Promise<void> {
    this.storeUsage(r, f);
    this.storeSession(r, f.sessionId);
    this.storeCode(r, f.code);
    if (f.status === "succeeded") {
      const raw = (f.resultText ?? "").trim();
      this.dropResultFromProgress(r, raw);
      // The acceptance block is shown by the done card: the posted text leaves it out.
      // 接 4.2: the CLI's own report says whether the work is done or waits for a person.
      const report = reportOf(r);
      const result = stripAcceptance(raw) || stripAcceptance((f.summary ?? "").trim()) || report?.text || TEXT.done;
      if (needsReply(report)) {
        const topicStatus = report.state === "waiting" ? "waiting_answer" : "blocked";
        await this.end(r, "done", cut(result, MAX_RESULT_CHARS), topicStatus, report.text, false, this.requester(r));
      } else {
        await this.end(r, "done", cut(result, MAX_RESULT_CHARS), "done", firstLine(result), true);
        // 盯活: the CLI only promised the work and stopped. The desk continues it (at most 3 times).
        if (await this.continuePromise(r, raw)) return;
      }
    } else if (f.status === "cancelled") {
      // 接 5.4: a plain stop on the machine reads like stopped by a person (input.ts `stop`). A
      // cancel the daemon gives a reason for (it ran past maxDurationMs) is work that did not
      // finish: the topic stays blocked, as before.
      const aborted = typeof f.error === "string" && f.error.trim() !== "";
      await this.end(r, "cancelled", TEXT.cancelled, aborted ? "blocked" : "open", TEXT.cancelled);
    } else {
      const why = firstLine(readableError(f.error ?? f.summary ?? ""), 300);
      await this.end(r, "failed", TEXT.failed(why), "blocked", cut(TEXT.failed(why), 280));
    }
    // Follow-ups the CLI never took start a new run with the topic context.
    const unread = this.sql
      .exec<{ message_id: string; author_id: string; body: string; ack_message_id: string | null }>(
        "SELECT message_id, author_id, body, ack_message_id FROM inputs WHERE run_id = ? AND accepted = 0 ORDER BY at",
        r.id,
      )
      .toArray();
    // cards2 (F5): the follow-up acts on the card answers it carries, so their retract windows
    // close now; an answer taken back in the meantime is left out.
    const left: typeof unread = [];
    for (const x of unread) if (await this.answerStartsRun(r, x.message_id)) left.push(x);
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
        {
          groupId: r.group_id,
          topicId: r.topic_id,
          messageId: last.message_id,
          ackMessageId: last.ack_message_id ?? undefined,
          authorId: last.author_id,
          body: last.body,
          mentioned: true,
        },
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

  /**
   * Final state: closes a legacy progress message, posts the result (`body`; on success a Card v2
   * with intent "done" and its proof), keeps it as `resultMessageId`, sets the run and topic
   * status.
   */
  protected async end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string, success = false, waitingOn: string[] = []): Promise<void> {
    const now = Date.now();
    await this.closeProgress(r);
    await this.closeApprovalCard(r, "expired");
    this.sql.exec("UPDATE runs SET approval_id = NULL, paused_by = NULL, paused_at = NULL WHERE id = ?", r.id);
    r.approval_id = null;
    r.paused_by = null;
    r.paused_at = null;
    const id = await this.postResult(r, body, success, waitingOn);
    if (id) {
      this.sql.exec("UPDATE runs SET result_message_id = ? WHERE id = ?", id, r.id);
      r.result_message_id = id;
    }
    await this.setStatus(r, status, { ended_at: now, summary: cut(summary, 280) || null });
    // A run that stopped for someone's GitHub is not done: the topic waits on that person.
    const githubFor = status !== "cancelled" ? this.githubWaitingOn(r.id) : null;
    await this.topic(r, githubFor ? "blocked" : topicStatus, githubFor ? [githubFor] : waitingOn);
    if (status !== "cancelled") await this.reportPart(r, success, body);
    this.retro(r, status, body);
    // A slot may be free now: the next queued run (排队) gets it. Not awaited: it takes other
    // runs' turns and must not wait inside this run's turn.
    void this.dispatchQueued().catch((err) => this.log.warn("queue dispatch failed", { err }));
    if (status !== "cancelled" && this.meta(`github_resume:${r.id}`) !== null) await this.resumeAfterGithub(r);
  }

  /** 复盘 (memory/retro.ts): lessons from a run that ran on a machine, after it ended. Never fails the run. */
  protected retro(r: RunRow, status: RunStatus, result: string): void {
    if (!r.machine_id || r.external || r.kind === RUN_KIND_ASIDE) return;
    try {
      const raw = this.meta(`request:${r.id}`);
      if (raw === null) return;
      const req = JSON.parse(raw) as RunRequest;
      const input = {
        botId: this.botId as string,
        runId: r.id,
        groupId: r.group_id,
        status,
        requesterId: req.m.authorId,
        request: [req.m.body, ...req.followUps.map((f) => f.body)].join("\n\n"),
        ...(req.brief ? { brief: briefText(req.brief) } : {}),
        progress: r.progress_text,
        result,
      };
      this.ctx.waitUntil(retroAfterRun(this.env, input).catch((err) => this.log.warn("run review failed", { runId: r.id, err })));
    } catch (err) {
      this.log.warn("run review failed", { runId: r.id, err });
    }
  }

  /**
   * The pull requests a result names that the topic has no link for yet become `topic.linked`
   * (state "open" until the GitHub adapter sends the real state, checks and size).
   */
  protected async linkPrs(r: RunRow, proof: RunProof, body: string): Promise<void> {
    for (const pr of prRefs(body)) {
      if (proof.prs.some((p) => p.url === pr.url)) continue;
      const link = { kind: "pr", repo: pr.repo, number: pr.number, url: pr.url, title: "", state: "open", checks: [] };
      try {
        unwrap(await this.room(r).rpc(r.group_id, TOPIC_LINK_RPC, this.actor(), { topicId: r.topic_id, link }));
      } catch (err) {
        this.log.warn("topic link from the result failed", { runId: r.id, err });
      }
    }
  }

  /** Posts the result once (idempotent by client message id); returns its message id. */
  protected async postResult(r: RunRow, body: string, success: boolean, mentions: string[] = []): Promise<string | null> {
    const cmid = `run:${r.id}:result`;
    // Problem 5: the CLI posted its own done card (`chat-run card post`): that card is the
    // result, and its last message is not posted a second time.
    const ownCard = success ? this.meta(`doneCard:${r.id}`) : null;
    if (ownCard) return ownCard;
    // A side chat answers as a plain message in the asker's DM.
    if (success && r.kind !== RUN_KIND_ASIDE) {
      try {
        const proof = unwrap(
          await this.room(r).rpc(r.group_id, "bots.runProof", this.actor(), { topicId: r.topic_id, runId: r.id, since: r.created_at }),
        ) as RunProof;
        await this.linkPrs(r, proof, body);
        const card = doneCard({
          runId: r.id,
          result: body,
          prs: mergePrs(proof.prs, prRefs(body)),
          artifacts: proof.artifacts,
          acceptance: this.acceptanceOf(r) ?? [],
          tests: this.loggedTests(r),
        });
        // A plain answer (no tests, acceptance, pull requests or 产出) is a text message.
        if (card && hasProof(card)) return await this.post(r, cmid, body, { cardV2: card, card: doneCardV1(card) });
      } catch (err) {
        this.log.warn("done card failed, posting text", { runId: r.id, err });
      }
    }
    try {
      return await this.post(r, cmid, body, mentions.length > 0 ? { mentions } : {});
    } catch (err) {
      this.log.warn("result post failed", { runId: r.id, err });
      return null;
    }
  }

  /** 接 5.1: test counts from the run's recorded tool calls and results (its last test command). */
  protected loggedTests(r: Pick<RunRow, "id">): TestCounts | null {
    const rows = this.sql
      .exec<{ frame: string }>(
        `SELECT frame FROM run_log WHERE run_id = ? AND (frame LIKE '%"type":"tool.call"%' OR frame LIKE '%"type":"tool.result"%')
         ORDER BY seq DESC LIMIT 400`,
        r.id,
      )
      .toArray();
    const events: unknown[] = [];
    for (const x of rows.reverse()) {
      try {
        events.push((JSON.parse(x.frame) as { event?: unknown }).event);
      } catch {
        // A frame that does not parse shows nothing.
      }
    }
    return testsFromLog(events);
  }

  /**
   * 接 4.2: `chat-run report` (output op `run.report`): keeps the run's latest report; it counts
   * when the run ends (finish). Result `{ state }`.
   */
  protected storeReport(r: RunRow, body: unknown): { state: string } {
    const report = parseReport(body);
    r.report = JSON.stringify({ ...report, at: Date.now() });
    this.sql.exec("UPDATE runs SET report = ? WHERE id = ?", r.report, r.id);
    return { state: report.state };
  }
}
