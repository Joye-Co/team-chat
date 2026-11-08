// BotDesk input: topic messages for the bot (new runs, follow-up input, text approvals), card
// presses on approval cards, and stopping runs (cancel by a member, a deactivated bot).
import type { Approver, BotRun } from "@chat/protocol";
import { roomStub } from "../../core/env";
import { ApiError, type Result, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { TEXT, approvalDecision } from "../dispatch";
import { linkStub } from "../env";
import { answerText } from "../answers";
import { refsText } from "../refs";
import type { CardAnswerIn, CardPress, TopicMessage } from "../room";
import { ACTIVE, type RunRow, result } from "./core";
import { DeskDelegate } from "./delegate";
import { isProgressQuestion, progressAnswer } from "../../agent/supervise";

export abstract class DeskInput extends DeskDelegate {
  // -------------------------------------------------------------------------
  // Messages from the topic
  // -------------------------------------------------------------------------

  /** A human message in a topic this bot is responsible for, or one that mentions it. */
  /**
   * A message for this bot, from the room's outbox. The room retries until this returns, so it
   * throws on failure and is idempotent by message id: a message that started a run or was
   * passed to one is not handled again.
   */
  async onMessage(botId: string, m: TopicMessage): Promise<void> {
    this.bind(botId);
    if (this.handledMessage(m.messageId)) return;
    if (this.messagesInFlight.has(m.messageId)) throw new ApiError("conflict", "message is being handled");
    this.messagesInFlight.add(m.messageId);
    try {
      if (!(await this.botActive())) return;
      const active = this.activeRun(m.topicId);
      if (active) return await this.serial(active.id, () => this.toActive(active.id, m));
      // Without an @, only the answer to the front desk's open question counts.
      if (!m.mentioned && !this.openAsk(m)) return;
      await this.admit({ ...m, mentioned: true });
    } catch (err) {
      this.log.error("onMessage failed", { err, topicId: m.topicId });
      throw err;
    } finally {
      this.messagesInFlight.delete(m.messageId);
    }
  }

  protected readonly messagesInFlight = new Set<string>();

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
      // Ended meanwhile: treat as a new request when it mentions the bot.
      if (m.mentioned) await this.admit(m);
      return;
    }
    const link = r.machine_id ? linkStub(this.env, r.machine_id) : null;
    const decision = r.approval_id ? approvalDecision(m.body) : null;
    // The room hook sends only human messages; other members' 同意 stays plain input.
    if (r.approval_id && decision && link && this.mayApprove(r, m.authorId, "human", m.authorLevel, await this.approver())) {
      await this.auditApproval(r, m.authorId, decision, "text");
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
      await link.toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision } as never);
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
   * and the references section; context only for sources the run did not see yet.
   */
  protected async inputText(r: RunRow, m: TopicMessage): Promise<string> {
    if (!m.refs || m.refs.length === 0) return m.body;
    const ids = m.refs.map((x) => x.fromId).filter((x): x is string => Boolean(x));
    const names = await this.names([m.authorId, ...ids]);
    const text = [m.body, refsText(names.get(m.authorId) ?? m.authorId, m.refs, names, this.seenRefs(r.id))].filter((x) => x !== "").join("\n\n");
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
    const decision = a.actionId.startsWith("approve:") ? "approve" : a.actionId.startsWith("deny:") ? "deny" : null;
    if (!decision) return;
    if (!this.mayApprove(r, a.actorId, a.actorKind, a.actorLevel, await this.approver())) {
      this.log.info("approval press ignored", { runId: r.id, actorId: a.actorId, kind: a.actorKind });
      return;
    }
    await this.auditApproval(r, a.actorId, decision, "card");
    await linkStub(this.env, r.machine_id).toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision } as never);
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
      const active = this.activeRun(a.topicId);
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
