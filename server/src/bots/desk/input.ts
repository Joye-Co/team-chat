// BotDesk input: topic messages for the bot (new runs, follow-up input, text approvals), card
// presses on approval cards, and stopping runs (cancel by a member, a deactivated bot).
import { type Approver, type BotRun, isMainTopicId, RUN_KIND_ASIDE } from "@chat/protocol";
import { loadModelConfig } from "../../aiservice";
import { roomStub } from "../../core/env";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TEXT, approvalDecision, approvesWholeWork } from "../dispatch";
import { linkStub } from "../env";
import { answerText } from "../answers";
import { refsText } from "../refs";
import { requestAuthors, requestText } from "../../context/render";
import type { CardAnswerIn, CardPress, TopicMessage } from "../room";
import { ACTIVE, type RunRow, result } from "./core";
import { DeskDelegate } from "./delegate";
import { isProgressQuestion, isStopRequest, progressAnswer } from "../../agent/supervise";
import { isNewTask } from "../../agent/frontdesk";

/**
 * 接 1.4 (usable section 4): a message without an @ from the person who asked for the last run in
 * a topic counts as a follow-up for this long after the bot's last answer there.
 */
export const FOLLOW_UP_WINDOW_MS = 30 * 60_000;

export abstract class DeskInput extends DeskDelegate {
  // -------------------------------------------------------------------------
  // Messages from the topic
  // -------------------------------------------------------------------------

  /**
   * A message for this bot, from the room's outbox: one that mentions it, a DM, or (`mentioned`
   * false) one without any @ in a topic it most recently started a run in. The room retries
   * until this returns, so it throws on failure and is idempotent by message id: a message that
   * started a run or was passed to one is not handled again.
   */
  async onMessage(botId: string, m: TopicMessage): Promise<void> {
    this.bind(botId);
    if (this.handledMessage(m.messageId)) return;
    if (this.messagesInFlight.has(m.messageId)) throw new ApiError("conflict", "message is being handled");
    this.messagesInFlight.add(m.messageId);
    try {
      if (!(await this.botActive())) return;
      const active = this.runFor(m);
      // 接 1.5: a new request next to a running one in a chat area gets its own run.
      if (active === null) return await this.admit({ ...m, mentioned: true });
      if (active) {
        // 接 1.6: while it works, only what is meant for it reaches the CLI; the rest of the talk
        // stays in the topic and reaches the next turn as context.
        if (!this.meantForRun(active, m)) return;
        return await this.serial(active.id, () => this.toActive(active.id, m));
      }
      // Without an @: the answer to the front desk's open question, or a follow-up (接 1.4).
      if (m.mentioned || this.openAsk(m)) return await this.admit({ ...m, mentioned: true });
      if (this.isFollowUp(m)) await this.admit({ ...m, mentioned: true }, true);
    } catch (err) {
      this.log.error("onMessage failed", { err, topicId: m.topicId });
      throw err;
    } finally {
      this.messagesInFlight.delete(m.messageId);
    }
  }

  protected readonly messagesInFlight = new Set<string>();

  /**
   * The active run a message is for; null: a new request that gets its own run next to the
   * running ones (接 1.5). In a topic (话题) that is the topic's run. In a chat area (群聊, a DM's
   * chat area) a member may work on several requests at once: a reply to a run's message goes to
   * that run, an answer to the front desk's question and 新任务 start a new one, an approval
   * answer goes to the run that waits for it, and the person's own request gets what they say;
   * anyone else who @s the member starts a new run.
   */
  protected runFor(m: TopicMessage): RunRow | null | undefined {
    const runs = this.activeRuns(m.topicId);
    if (runs.length === 0) return undefined;
    if (!isMainTopicId(m.topicId)) return runs[0];
    const quoted = m.replyToMessageId;
    if (quoted) {
      const r = runs.find((x) => this.runMessage(x, quoted));
      if (r) return r;
    }
    if (this.openAsk(m) || (m.mentioned && isNewTask(m.body))) return null;
    const waits = runs.find((x) => x.approval_id && (approvalDecision(m.body) !== null || approvesWholeWork(m.body)));
    if (waits) return waits;
    const own = runs.find((x) => x.requested_by === m.authorId);
    if (own) return own;
    if (m.replyToAuthorId === this.botId || isProgressQuestion(m.body)) return runs[0];
    return m.mentioned ? null : runs[0];
  }

  /** Whether a message is the run's own: its request, its progress or approval card, or an input it took. */
  protected runMessage(r: RunRow, messageId: string): boolean {
    if (messageId === r.trigger_message_id || messageId === r.progress_message_id || messageId === r.result_message_id) return true;
    if (messageId === this.approvalCard(r)?.messageId || this.meta(`runCard:${messageId}`) === r.id) return true;
    return this.sql.exec("SELECT 1 FROM inputs WHERE run_id = ? AND message_id = ? LIMIT 1", r.id, messageId).toArray().length > 0;
  }

  /**
   * 接 1.6: a message is input for the running run when it mentions the bot (or is in its DM),
   * quotes the bot, comes from the run's requester, answers its approval or asks how it goes.
   */
  protected meantForRun(r: RunRow, m: TopicMessage): boolean {
    if (m.mentioned || m.replyToAuthorId === this.botId || m.authorId === r.requested_by) return true;
    if (r.approval_id && (approvalDecision(m.body) !== null || approvesWholeWork(m.body))) return true;
    return isProgressQuestion(m.body);
  }

  /**
   * 接 1.4: a message without an @ in a topic is a follow-up when it comes from the person the
   * bot last answered there (a run's requester, or the front desk's reply), soon after.
   */
  protected isFollowUp(m: TopicMessage): boolean {
    if (isMainTopicId(m.topicId) || m.dm) return false;
    const since = Date.now() - FOLLOW_UP_WINDOW_MS;
    const last = this.sql
      .exec<{ requested_by: string; ended_at: number | null; status: string }>(
        "SELECT requested_by, ended_at, status FROM runs WHERE topic_id = ? AND (kind IS NULL OR kind != ?) ORDER BY created_at DESC LIMIT 1",
        m.topicId,
        RUN_KIND_ASIDE,
      )
      .toArray()[0];
    // A run a person stopped (停, the stop button) invites no follow-up: "好的我自己来" starts nothing.
    if (last?.status === "cancelled") return false;
    if (last && last.ended_at !== null && last.ended_at >= since && last.requested_by === m.authorId) return true;
    return (
      this.sql
        .exec("SELECT 1 FROM decisions WHERE topic_id = ? AND author_id = ? AND reply_message_id IS NOT NULL AND at >= ? LIMIT 1", m.topicId, m.authorId, since)
        .toArray().length > 0
    );
  }

  protected handledMessage(messageId: string): boolean {
    return (
      this.sql.exec("SELECT 1 FROM runs WHERE trigger_message_id = ? LIMIT 1", messageId).toArray().length > 0 ||
      this.sql.exec("SELECT 1 FROM inputs WHERE message_id = ? LIMIT 1", messageId).toArray().length > 0 ||
      // A front desk decision that is fully acted on (a dispatch is a run, above).
      this.sql
        .exec("SELECT 1 FROM decisions WHERE message_id = ? AND (action = 'skip' OR reply_message_id IS NOT NULL) LIMIT 1", messageId)
        .toArray().length > 0
    );
  }


  /**
   * Only a human who is the run's requester, or an owner, may answer its approval. settings:
   * with approver "owners" (谁可以批准 = 管理员) only owners may.
   */
  protected mayApprove(
    r: RunRow,
    actorId: string,
    kind: string | null | undefined,
    level: string | null | undefined,
    approver: Approver = "requester",
  ): boolean {
    if (kind !== "human") return false;
    if (level === "owner") return true;
    return approver !== "owners" && actorId === r.requested_by;
  }

  protected async auditApproval(r: RunRow, actorId: string, decision: string, via: "card" | "text"): Promise<void> {
    try {
      await this.env.DB.prepare(
        "INSERT INTO audit (actor_id, credential_id, action, target, details, at) VALUES (?, NULL, 'run.approval', ?, ?, ?)",
      )
        .bind(actorId, r.id, JSON.stringify({ approvalId: r.approval_id, decision, via, botId: this.botId }), Date.now())
        .run();
    } catch (err) {
      this.log.warn("approval audit failed", { runId: r.id, err });
    }
  }

  protected async toActive(runId: string, m: TopicMessage): Promise<void> {
    const r = this.run(runId);
    if (!r) return;
    if (r.ended_at !== null) {
      // Ended meanwhile: treat as a new request when it mentions the bot or follows its answer.
      if (m.mentioned) await this.admit(m);
      else if (this.isFollowUp(m)) await this.admit({ ...m, mentioned: true }, true);
      return;
    }
    const link = r.machine_id ? linkStub(this.env, r.machine_id) : null;
    const kind = this.approvalKind(r);
    // 这件事里都同意 answers only a gated approval (one with a kind).
    const whole = kind !== null && approvesWholeWork(m.body);
    // A short "好" / "ok" answers only in a reply that quotes the approval card.
    const toCard = m.replyToMessageId !== undefined && m.replyToMessageId === this.approvalCard(r)?.messageId;
    const decision = r.approval_id ? (whole ? "approve" : approvalDecision(m.body, toCard)) : null;
    // The room hook sends only human messages; other members' 同意 stays plain input.
    if (r.approval_id && decision && link && this.mayApprove(r, m.authorId, "human", m.authorLevel, await this.approverOfOpen(r, kind))) {
      await this.auditApproval(r, m.authorId, whole ? "approve_run" : decision, "text");
      // Recorded as an input so a retried call does not answer twice.
      this.sql.exec(
        "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, accepted) VALUES (?, ?, ?, ?, ?, ?, 1)",
        ulid(),
        r.id,
        m.messageId,
        m.authorId,
        m.body,
        Date.now(),
      );
      if (whole && kind) this.approveGateForWork(r.id, kind);
      await link.toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision, ...(whole ? { scope: "run" } : {}) } as never);
      await this.closeApprovalCard(r, whole ? "approve_run" : decision, m.authorId);
      return;
    }
    // 接 1.12: a plain "停" from the requester stops the run (with a front desk, it decides).
    if (m.authorId === r.requested_by && isStopRequest(m.body) && !(await loadModelConfig(this.env))) {
      // Recorded as a taken input, so a retried delivery does not stop or start anything again.
      this.sql.exec(
        "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, accepted) VALUES (?, ?, ?, ?, ?, ?, 1)",
        ulid(),
        r.id,
        m.messageId,
        m.authorId,
        m.body,
        Date.now(),
      );
      await this.stop(r);
      this.log.info("run stopped by text", { runId: r.id, actorId: m.authorId });
      return;
    }
    // 盯活: "进展怎么样了" is answered from the run's own state; the CLI is not interrupted.
    if (!r.approval_id && isProgressQuestion(m.body)) return this.answerProgress(r, m);
    const inputId = ulid();
    const text = await this.inputText(r, m);
    this.sql.exec(
      "INSERT INTO inputs (id, run_id, message_id, author_id, body, at) VALUES (?, ?, ?, ?, ?, ?)",
      inputId,
      r.id,
      m.messageId,
      m.authorId,
      text,
      Date.now(),
    );
    if (link) await link.toRun({ t: "run.input", runId: r.id, inputId, text, authorId: m.authorId } as never);
  }

  protected async answerProgress(r: RunRow, m: TopicMessage): Promise<void> {
    // Kept as a taken input, so a retried delivery does not answer twice and no follow-up starts.
    this.sql.exec(
      "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, accepted) VALUES (?, ?, ?, ?, ?, ?, 1)",
      ulid(),
      r.id,
      m.messageId,
      m.authorId,
      m.body,
      Date.now(),
    );
    const plan = r.plan ? (JSON.parse(r.plan) as { text: string; state: string }[]) : [];
    const lastTool = r.progress_text
      .split("\n")
      .reverse()
      .find((l) => l.startsWith("· "))
      ?.slice(2);
    const text = progressAnswer({ startedAt: r.started_at, now: Date.now(), plan, status: r.status, ...(lastTool ? { lastTool } : {}) });
    await this.post(r, `progress:${m.messageId}`, text);
  }

  /**
   * The `run.input` text of a topic message: the body, and with refs (引用提问) the question
   * and the references section; context only for sources the run did not see yet. Files, 产出
   * and the quoted message follow the body as indented lines, so an image alone is not empty.
   */
  protected async inputText(r: RunRow, m: TopicMessage): Promise<string> {
    if ((!m.refs || m.refs.length === 0) && !m.ext) return m.body;
    const ids = (m.refs ?? []).map((x) => x.fromId).filter((x): x is string => Boolean(x));
    const names = await this.names([...new Set([m.authorId, ...ids, ...requestAuthors(m.ext)])]);
    const body = requestText(m.body, m.ext, (id) => names.get(id) ?? id);
    if (!m.refs || m.refs.length === 0) return body;
    const text = [body, refsText(names.get(m.authorId) ?? m.authorId, m.refs, names, this.seenRefs(r.id))].filter((x) => x !== "").join("\n\n");
    this.seeRefs(r.id, m.refs.map((x) => x.messageId));
    return text;
  }

  /** A card button of this bot's message was pressed (approval cards). */
  async cardAction(botId: string, a: CardPress): Promise<void> {
    this.bind(botId);
    const r = this.sql
      .exec<RunRow>(`SELECT * FROM runs WHERE ${ACTIVE} AND approval_id IS NOT NULL AND group_id = ?`, a.groupId)
      .toArray()
      .find((x) => (x as RunRow).approval_id && a.actionId.endsWith(`:${(x as RunRow).approval_id}`)) as RunRow | undefined;
    if (!r?.machine_id || !r.approval_id) return;
    const kind = this.approvalKind(r);
    // 这件事里都同意 (approveall:) counts only for a gated approval.
    const whole = kind !== null && a.actionId.startsWith("approveall:");
    const decision = a.actionId.startsWith("approve:") || whole ? "approve" : a.actionId.startsWith("deny:") ? "deny" : null;
    if (!decision) return;
    if (!this.mayApprove(r, a.actorId, a.actorKind, a.actorLevel, await this.approverOfOpen(r, kind))) {
      this.log.info("approval press ignored", { runId: r.id, actorId: a.actorId, kind: a.actorKind });
      return;
    }
    await this.auditApproval(r, a.actorId, whole ? "approve_run" : decision, "card");
    if (whole && kind) this.approveGateForWork(r.id, kind);
    await linkStub(this.env, r.machine_id).toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision, ...(whole ? { scope: "run" } : {}) } as never);
    await this.closeApprovalCard(r, whole ? "approve_run" : decision, a.actorId);
  }

  // -------------------------------------------------------------------------
  // Card v2 answers and 产出 page events (1.1)
  // -------------------------------------------------------------------------

  /**
   * A person answered a v2 card of this bot. The topic's active run gets it as `run.answer` (a
   * 1.1 daemon) or `run.input` with the text envelope (older daemons; typed `answer` for feature
   * "answer"), see MachineLink; with no active run, the answer starts a run like a mention
   * while its ask still stands.
   * Idempotent by the answer's seq.
   */
  async cardAnswer(botId: string, a: CardAnswerIn): Promise<void> {
    this.bind(botId);
    const key = `answer:${a.groupId}:${a.seq}`;
    // People only: an answer by a bot never reaches a bot (no loops).
    if (a.actorKind !== "human" || this.handledMessage(key)) return;
    if (this.messagesInFlight.has(key)) throw new ApiError("conflict", "answer is being handled");
    this.messagesInFlight.add(key);
    try {
      if (!(await this.botActive())) return;
      const actor = { id: a.actorId, name: a.actorName };
      const inputId = ulid();
      const text = answerText(a.card, a.tally ? { ...a.answer, tally: a.tally } : a.answer, actor, inputId);
      const m: TopicMessage = {
        groupId: a.groupId,
        topicId: a.topicId,
        messageId: key,
        ackMessageId: a.answer.messageId,
        authorId: a.actorId,
        authorLevel: a.actorLevel ?? undefined,
        body: text,
        mentioned: true,
      };
      // A private card's answerer is known here only: the run works for them (protocol "Who a run works for").
      if (a.voterId) m.requestedFor = a.voterId;
      // 接 1.8: an answer in the front desk's question card answers that question; in a chat area
      // it starts its own run next to any running one.
      const asked = this.askOfCard(a.answer.messageId);
      if (asked) this.sql.exec("UPDATE decisions SET answered = 1 WHERE message_id = ?", asked.message_id);
      const active = asked && isMainTopicId(a.topicId) ? undefined : this.runOfCard(a.topicId, a.answer.messageId);
      // cards2 (F5): an answer that starts a run closes its retract window first. It starts a
      // run only while its ask still stands: an answer to a closed ask of an ended run does not.
      if (!active) {
        if (!(await this.answerStands(a.groupId, a.seq, true, true))) return;
        return await this.start(m);
      }
      await this.serial(active.id, async () => {
        const r = this.run(active.id);
        if (!r) return;
        if (r.ended_at !== null) {
          if (!(await this.answerStands(a.groupId, a.seq, true, true))) return;
          return this.start(m);
        }
        if (!(await this.answerStands(a.groupId, a.seq, false))) return;
        this.sql.exec(
          "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, ack_message_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
          inputId,
          r.id,
          key,
          a.actorId,
          text,
          Date.now(),
          a.answer.messageId,
        );
        if (!r.machine_id || r.external) return;
        await linkStub(this.env, r.machine_id).toRun(this.answerFrame(r, inputId, text, a) as never);
      });
    } catch (err) {
      this.log.error("cardAnswer failed", { err, topicId: a.topicId });
      throw err;
    } finally {
      this.messagesInFlight.delete(key);
    }
  }

  /** `run.answer` (or `run.input`) for an answer; the interaction module builds it. */
  protected abstract answerFrame(r: RunRow, inputId: string, text: string, a: CardAnswerIn): Record<string, unknown>;

  // -------------------------------------------------------------------------
  // Stopping runs (members, and a deactivated bot)
  // -------------------------------------------------------------------------

  /** A member stops a run: the machine gets run.cancel, and the run ends here at once. */
  async cancel(botId: string, runId: string, actorId: string): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        await this.stop(r);
        this.log.info("run cancelled", { runId, actorId });
        return this.toRun(r);
      });
    });
  }

  protected async stop(r: RunRow): Promise<void> {
    if (r.machine_id && !r.external) {
      try {
        await linkStub(this.env, r.machine_id).toRun({ t: "run.cancel", runId: r.id } as never);
      } catch (err) {
        this.log.warn("run.cancel failed", { runId: r.id, err });
      }
    }
    this.planBuckets.delete(r.id);
    if (r.external) {
      await this.setStatus(r, "cancelled", { ended_at: Date.now(), summary: TEXT.cancelled });
      return;
    }
    await this.end(r, "cancelled", TEXT.cancelled, "open", TEXT.cancelled);
  }

  /** The bot was deactivated: stop its active runs and give up its topics. */
  async deactivated(botId: string): Promise<void> {
    this.bind(botId);
    const active = this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${ACTIVE}`).toArray();
    for (const { id } of active) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (r && r.ended_at === null) await this.stop(r);
      });
    }
    const groups = this.sql.exec<{ group_id: string }>("SELECT DISTINCT group_id FROM runs").toArray();
    for (const { group_id } of groups) {
      try {
        unwrap(await roomStub(this.env, group_id).rpc(group_id, "bots.release", null, { botId }));
      } catch (err) {
        this.log.warn("claim release failed", { groupId: group_id, err });
      }
    }
  }
}
