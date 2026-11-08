// BotDesk front desk (前台, agent/frontdesk.ts): a new human request is read by one model call
// before it becomes a run. The decision is stored first (先存后做), then acted on, so a retried
// delivery acts on the same decision. Without an AI service, or for anything that is not a plain
// human request, the request becomes a run as before.
import { isMainTopicId } from "@chat/protocol";
import { loadModelConfig } from "../../aiservice";
import { type Decision, FRONT_DESK_CHARS, decide } from "../../agent/frontdesk";
import { selectLines } from "../../context/render";
import { FRONT_DESK_MEMORY_BYTES, memoryIndex } from "../../memory/inject";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import type { ClaimResult, TopicMessage } from "../room";
import { DeskStall } from "./stall";

/** A question the front desk asked waits this long for the requester's answer without an @. */
export const ASK_WINDOW_MS = 60 * 60_000;

type DecisionRow = { message_id: string; action: string; decision: string; reply_message_id: string | null; topic_id: string; author_id: string };

export abstract class DeskFront extends DeskStall {
  protected decisionRow(messageId: string): DecisionRow | undefined {
    return this.sql.exec<DecisionRow>("SELECT * FROM decisions WHERE message_id = ?", messageId).toArray()[0] as DecisionRow | undefined;
  }

  /** The requester answers the front desk's open question in the topic (no @ needed). */
  protected openAsk(m: TopicMessage): DecisionRow | undefined {
    return this.sql
      .exec<DecisionRow>(
        "SELECT * FROM decisions WHERE topic_id = ? AND author_id = ? AND action = 'ask' AND answered = 0 AND at > ? ORDER BY at DESC LIMIT 1",
        m.topicId,
        m.authorId,
        Date.now() - ASK_WINDOW_MS,
      )
      .toArray()[0] as DecisionRow | undefined;
  }

  /** A new human request: the front desk decides, then acts. */
  protected async admit(m: TopicMessage): Promise<void> {
    // Card answers, live events and retries carry synthetic keys: they go straight to a run.
    if (m.messageId.includes(":")) return this.start(m);
    const botId = this.botId as string;
    let row = this.decisionRow(m.messageId);
    if (!row) {
      const cfg = await loadModelConfig(this.env);
      if (!cfg) return this.start(m);
      const peek = unwrap(
        await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId, peek: true }),
      ) as ClaimResult;
      // A side chat (单独问) is read-only and private: it keeps its own run. Another bot's topic is not ours.
      if (peek.contextOf) return this.start(m);
      if (peek.botId !== botId) return;
      await this.react(m);
      const notRequest = (x: { id: string }) => x.id !== m.messageId;
      const newest = (peek.newest ?? []).filter(notRequest);
      const first = peek.first && notRequest(peek.first) ? peek.first : undefined;
      const ids = [first, ...newest].flatMap((x) => (x ? [x.authorId, ...(x.ext.cardState?.answers ?? []).map((a) => a.actorId)] : []));
      const names = await this.names([...new Set([...ids, m.authorId, botId])]);
      const name = (id: string) => names.get(id) ?? id;
      const total = Math.max(0, (peek.total ?? 0) - (peek.newest ?? []).length + newest.length);
      const sel = selectLines(first, newest, total, name, FRONT_DESK_CHARS);
      const member = await this.env.DB.prepare("SELECT role FROM members WHERE id = ?").bind(botId).first<{ role: string | null }>();
      const res = await decide(cfg, {
        botName: name(botId),
        role: member?.role ?? null,
        title: peek.title,
        context: sel.text,
        requester: name(m.authorId),
        request: m.body,
        dm: false,
        memory: await memoryIndex(this.env, { botId, groupId: m.groupId, requesterId: m.authorId, requesterName: name(m.authorId), maxBytes: FRONT_DESK_MEMORY_BYTES }),
      });
      this.sql.exec(
        `INSERT OR IGNORE INTO decisions (message_id, group_id, topic_id, author_id, action, decision, fallback, model, ms, input_tokens, output_tokens, at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        m.messageId,
        m.groupId,
        m.topicId,
        m.authorId,
        res.decision.action,
        JSON.stringify(res.decision),
        res.fallback ?? null,
        cfg.model,
        res.ms,
        res.usage?.inputTokens ?? 0,
        res.usage?.outputTokens ?? 0,
        Date.now(),
      );
      this.log.info("front desk decided", { messageId: m.messageId, action: res.decision.action, ms: res.ms, fallback: res.fallback });
      row = this.decisionRow(m.messageId) as DecisionRow;
    }
    const d = JSON.parse(row.decision) as Decision;
    // An answer to the front desk's question closes it; the request and the answer go on together.
    this.sql.exec("UPDATE decisions SET answered = 1 WHERE topic_id = ? AND author_id = ? AND action = 'ask' AND message_id != ?", m.topicId, m.authorId, m.messageId);
    switch (d.action) {
      case "dispatch":
        return this.start(m, undefined, [], undefined, d.brief);
      case "reply":
      case "ask": {
        if (!row.reply_message_id) {
          const id = await this.post({ group_id: m.groupId, topic_id: m.topicId }, `fd:${m.messageId}`, d.reply ?? "");
          this.sql.exec("UPDATE decisions SET reply_message_id = ? WHERE message_id = ?", id, m.messageId);
        }
        // A question in a topic makes this bot its responsible member, so the answer reaches it without an @.
        if (d.action === "ask" && !isMainTopicId(m.topicId)) {
          await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId });
        }
        return;
      }
      case "skip":
        return;
    }
  }

  /** 👀 on the request while the front desk reads it (the same reaction a run gives). */
  protected async react(m: TopicMessage): Promise<void> {
    await this.ack({ group_id: m.groupId, trigger_message_id: m.messageId } as never, m.messageId);
  }

  /** The front desk's decisions in a topic, newest first (developer view). */
  async decisions(botId: string, topicId: string, limit = 20): Promise<unknown[]> {
    this.bind(botId);
    return this.sql
      .exec<Record<string, SqlStorageValue>>("SELECT * FROM decisions WHERE topic_id = ? ORDER BY at DESC LIMIT ?", topicId, Math.min(100, Math.max(1, limit)))
      .toArray()
      .map((r) => ({ ...r, decision: JSON.parse(r.decision as string) }));
  }
}
