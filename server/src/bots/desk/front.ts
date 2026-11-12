// BotDesk front desk (前台, agent/frontdesk.ts): a new human request is read by one model call
// before it becomes a run. The decision is stored first (先存后做), then acted on, so a retried
// delivery acts on the same decision. Without an AI service, or for anything that is not a plain
// human request, the request becomes a run as before.
import { CARD_V2_VERSION, type CardV2, isMainTopicId } from "@chat/protocol";
import { loadModelConfig } from "../../aiservice";
import type { ModelConfig } from "../../aiservice/providers";
import { type Decision, type DecideResult, FRONT_DESK_CHARS, decide } from "../../agent/frontdesk";
import { CARD_PATCH_RPC } from "../../cards2";
import { firstLine } from "../dispatch";
import { requestAuthors, requestText, selectLines, withSummary } from "../../context/render";
import { teamZone } from "../../memory/consolidate";
import { FRONT_DESK_MEMORY_BYTES, memoryIndex } from "../../memory/inject";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import type { ClaimResult, TopicMessage } from "../room";
import { DeskStall } from "./stall";

/** A question the front desk asked waits this long for the requester's answer without an @. */
export const ASK_WINDOW_MS = 60 * 60_000;
/** 接 1.13: times the front desk decides again because the topic changed while it thought. */
export const MAX_REDECIDES = 2;

/** Words of the front desk's question card (接 1.8). */
const ASK_TEXT = { label: "你的回答", send: "回答" };

/**
 * 接 1.8: the front desk's question as a Card v2 ask: it waits on the requester (the topic shows
 * 等你回答) until the window ends. The answer is typed in the card or sent as the next message.
 */
export function askCard(question: string, askerId: string, expiresAt: number): CardV2 {
  const title = firstLine(question, 200) || "?";
  const whole = question.trim();
  return {
    v: CARD_V2_VERSION,
    rev: 1,
    intent: "ask",
    title,
    fallback: whole.slice(0, 4000),
    ask: { from: [askerId], blocking: true, expiresAt },
    blocks: [
      ...(whole !== title ? [{ type: "text" as const, id: "question", text: whole.slice(0, 8000) }] : []),
      { type: "input" as const, id: "answer", kind: "textarea" as const, label: ASK_TEXT.label, required: true },
    ],
    actions: [{ id: "send", label: ASK_TEXT.send, style: "primary", inputs: ["answer"] }],
  } as CardV2;
}

type DecisionRow = { message_id: string; action: string; decision: string; reply_message_id: string | null; topic_id: string; author_id: string };

export abstract class DeskFront extends DeskStall {
  protected decisionRow(messageId: string): DecisionRow | undefined {
    return this.sql.exec<DecisionRow>("SELECT * FROM decisions WHERE message_id = ?", messageId).toArray()[0] as DecisionRow | undefined;
  }

  /** The front desk's question whose card is `messageId` (接 1.8). */
  protected askOfCard(messageId: string): DecisionRow | undefined {
    return this.sql.exec<DecisionRow>("SELECT * FROM decisions WHERE reply_message_id = ? AND action = 'ask'", messageId).toArray()[0] as DecisionRow | undefined;
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

  /**
   * A new human request: the front desk decides, then acts. `followUp` (接 1.4): the message does
   * not @ this bot but follows its last answer; it gets no 👀 unless it becomes a run.
   */
  protected async admit(m: TopicMessage, followUp = false): Promise<void> {
    // Card answers, live events and retries carry synthetic keys: they go straight to a run.
    if (m.messageId.includes(":")) return this.start(m);
    const botId = this.botId as string;
    let row = this.decisionRow(m.messageId);
    if (!row) {
      const cfg = await loadModelConfig(this.env);
      if (!cfg) return this.start(m);
      let res: DecideResult | undefined;
      for (let round = 0; !res; round++) {
        const read = await this.readAndDecide(m, followUp, cfg, round === 0 && !followUp);
        // A side chat (单独问) is read-only and private: it keeps its own run.
        if (!read) return this.start(m);
        // 接 1.13: the topic changed while the model thought: that decision read an old topic.
        if (round < MAX_REDECIDES && read.seen !== undefined && (await this.topicChanged(m, read.seen))) {
          this.log.info("front desk decides again", { messageId: m.messageId, round });
          continue;
        }
        res = read.res;
      }
      this.sql.exec(
        `INSERT OR IGNORE INTO decisions (message_id, group_id, topic_id, author_id, action, decision, fallback, model, ms, input_tokens, output_tokens, at, system, input)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
        res.system,
        res.prompt,
      );
      this.log.info("front desk decided", { messageId: m.messageId, action: res.decision.action, ms: res.ms, fallback: res.fallback });
      row = this.decisionRow(m.messageId) as DecisionRow;
    }
    const d = JSON.parse(row.decision) as Decision;
    // An answer to the front desk's question closes it; the request and the answer go on together.
    await this.closeAsks(m);
    switch (d.action) {
      case "dispatch":
        return this.start(m, undefined, [], undefined, d.brief, d.newTask ? { newTask: true } : {});
      case "reply":
      case "ask": {
        if (!row.reply_message_id) {
          const at = { group_id: m.groupId, topic_id: m.topicId };
          const id = d.action === "ask" ? await this.postAsk(m, d.reply ?? "") : await this.post(at, `fd:${m.messageId}`, d.reply ?? "");
          this.sql.exec("UPDATE decisions SET reply_message_id = ? WHERE message_id = ?", id, m.messageId);
        }
        // A question in a topic makes this bot its responsible member, so the answer reaches it without an @.
        if (d.action === "ask" && !isMainTopicId(m.topicId)) {
          await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId });
        }
        return;
      }
      case "skip":
        // 接 1.11: the 👀 it put while reading goes away; nothing happens.
        if (!followUp) await this.unack(m.groupId, m.messageId);
        return;
    }
  }

  /**
   * One read of the topic and one front desk call on it; `seen` is the newest seq it read. Null
   * for a side chat (单独问), which keeps its own run. `react`: put 👀 on the request first.
   */
  protected async readAndDecide(m: TopicMessage, followUp: boolean, cfg: ModelConfig, react: boolean): Promise<{ res: DecideResult; seen?: number } | null> {
    const botId = this.botId as string;
    const peek = unwrap(
      await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId, peek: true }),
    ) as ClaimResult;
    if (peek.contextOf) return null;
    if (react) await this.react(m);
    const notRequest = (x: { id: string }) => x.id !== m.messageId;
    const newest = (peek.newest ?? []).filter(notRequest);
    const first = peek.first && notRequest(peek.first) ? peek.first : undefined;
    const ids = [first, ...newest].flatMap((x) => (x ? [x.authorId, x.ext.replyTo?.authorId ?? x.authorId, ...(x.ext.cardState?.answers ?? []).map((a) => a.actorId)] : []));
    const names = await this.names([...new Set([...ids, m.authorId, botId, ...requestAuthors(m.ext)])]);
    const name = (id: string) => names.get(id) ?? id;
    const total = Math.max(0, (peek.total ?? 0) - (peek.newest ?? []).length + newest.length);
    const sel = selectLines(first, newest, total, name, FRONT_DESK_CHARS, { zone: teamZone(this.env) });
    const member = await this.env.DB.prepare("SELECT role FROM members WHERE id = ?").bind(botId).first<{ role: string | null }>();
    const res = await decide(cfg, {
      botName: name(botId),
      role: member?.role ?? null,
      title: peek.title,
      context: withSummary(sel, peek.summary),
      requester: name(m.authorId),
      request: requestText(m.body, m.ext, name),
      dm: m.dm === true,
      ...(followUp ? { followUp: true } : {}),
      memory: await memoryIndex(this.env, {
        botId,
        groupId: m.groupId,
        requesterId: m.authorId,
        requesterName: name(m.authorId),
        maxBytes: FRONT_DESK_MEMORY_BYTES,
        query: requestText(m.body, m.ext, name),
      }),
    });
    return { res, seen: peek.newest?.[0]?.seq };
  }

  /**
   * 接 1.13: a person said something in the topic after `seq`. Other AI members' messages do not
   * count: in a busy 群聊 they answer other requests all the time.
   */
  protected async topicChanged(m: TopicMessage, seq: number): Promise<boolean> {
    const since = unwrap(
      await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.since", this.actor(), { topicId: m.topicId, afterSeq: seq, exceptAuthor: this.botId as string }),
    ) as { newest: { authorId: string }[] };
    const ids = [...new Set(since.newest.map((x) => x.authorId))];
    if (ids.length === 0) return false;
    const people = await this.env.DB.prepare(`SELECT 1 FROM members WHERE kind = 'human' AND id IN (${ids.map(() => "?").join(",")}) LIMIT 1`)
      .bind(...ids)
      .first();
    return people !== null;
  }

  /** 接 1.8: the question as a card ask to the requester; plain text when the room takes no card. */
  protected async postAsk(m: TopicMessage, question: string): Promise<string> {
    const at = { group_id: m.groupId, topic_id: m.topicId };
    try {
      const card = askCard(question, m.authorId, Date.now() + ASK_WINDOW_MS);
      const id = await this.post(at, `fd:${m.messageId}`, question, { cardV2: card, ...(m.dm ? {} : { mentions: [m.authorId] }) });
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `askCard:${id}`, "1");
      return id;
    } catch (err) {
      this.log.warn("ask card failed", { messageId: m.messageId, err });
      return this.post(at, `fd:${m.messageId}:text`, question);
    }
  }

  /**
   * The person's message answers the front desk's open questions to them in the topic: they
   * close, and their cards stop waiting (the answer came as a message, not in the card).
   */
  protected async closeAsks(m: TopicMessage): Promise<void> {
    const open = this.sql
      .exec<{ message_id: string; reply_message_id: string | null }>(
        "SELECT message_id, reply_message_id FROM decisions WHERE topic_id = ? AND author_id = ? AND action = 'ask' AND answered = 0 AND message_id != ?",
        m.topicId,
        m.authorId,
        m.messageId,
      )
      .toArray();
    for (const x of open) {
      this.sql.exec("UPDATE decisions SET answered = 1 WHERE message_id = ?", x.message_id);
      const card = x.reply_message_id;
      if (!card || this.meta(`askCard:${card}`) === null) continue;
      this.sql.exec("DELETE FROM meta WHERE key = ?", `askCard:${card}`);
      try {
        unwrap(
          await roomStub(this.env, m.groupId).rpc(m.groupId, CARD_PATCH_RPC, this.actor(), {
            messageId: card,
            baseRev: 1,
            ops: [{ op: "card", props: { ask: null, actions: [] } }, { op: "remove", id: "answer" }],
            clientPatchId: `fd:${card}`,
          }),
        );
      } catch (err) {
        // Answered in the card meanwhile, or expired: it no longer waits either way.
        this.log.warn("ask card close failed", { messageId: card, err });
      }
    }
  }

  /** 👀 on the request while the front desk reads it (the same reaction a run gives). */
  protected async react(m: TopicMessage): Promise<void> {
    await this.ack({ group_id: m.groupId, trigger_message_id: m.messageId } as never, m.messageId);
  }

  /** Decisions with what the model read, oldest first, for the replay set (M6). Owners only. */
  async replayCases(botId: string, since: number, limit = 200): Promise<{ id: string; at: number; system: string; prompt: string; action: string; reason: string; model: string | null }[]> {
    this.bind(botId);
    return this.sql
      .exec<{ message_id: string; at: number; system: string; input: string; action: string; decision: string; model: string | null }>(
        "SELECT message_id, at, system, input, action, decision, model FROM decisions WHERE at >= ? AND input IS NOT NULL AND fallback IS NULL ORDER BY at LIMIT ?",
        since,
        Math.min(1000, Math.max(1, limit)),
      )
      .toArray()
      .map((r) => ({ id: r.message_id, at: r.at, system: r.system, prompt: r.input, action: r.action, reason: (JSON.parse(r.decision) as { reason?: string }).reason ?? "", model: r.model }));
  }

  /** What the desk did lately (developer view, M6 metrics). */
  async metrics(botId: string, since: number): Promise<Record<string, unknown>> {
    this.bind(botId);
    const pct = (xs: number[], p: number) => (xs.length === 0 ? 0 : xs[Math.min(xs.length - 1, Math.floor((p / 100) * xs.length))]);
    const d = this.sql
      .exec<{ action: string; ms: number; input_tokens: number; output_tokens: number; fallback: string | null }>(
        "SELECT action, ms, input_tokens, output_tokens, fallback FROM decisions WHERE at >= ?",
        since,
      )
      .toArray();
    const ms = d.map((x) => x.ms).sort((a, b) => a - b);
    const byAction: Record<string, number> = { reply: 0, dispatch: 0, ask: 0, skip: 0 };
    for (const x of d) byAction[x.action] = (byAction[x.action] ?? 0) + 1;
    const runs = this.sql
      .exec<{ status: string; started_at: number | null; ended_at: number | null; interventions: number; summary: string | null; trigger_message_id: string }>(
        "SELECT status, started_at, ended_at, interventions, summary, trigger_message_id FROM runs WHERE created_at >= ? AND external = 0",
        since,
      )
      .toArray();
    const durations = runs
      .filter((r) => r.started_at !== null && r.ended_at !== null)
      .map((r) => (r.ended_at as number) - (r.started_at as number))
      .sort((a, b) => a - b);
    const count = (f: (r: (typeof runs)[number]) => boolean) => runs.filter(f).length;
    return {
      since,
      frontDesk: {
        decisions: d.length,
        byAction,
        fallbacks: d.filter((x) => x.fallback !== null).length,
        p50Ms: pct(ms, 50),
        p95Ms: pct(ms, 95),
        inputTokens: d.reduce((a, x) => a + x.input_tokens, 0),
        outputTokens: d.reduce((a, x) => a + x.output_tokens, 0),
      },
      runs: {
        total: runs.length,
        done: count((r) => r.status === "done"),
        failed: count((r) => r.status === "failed"),
        blocked: count((r) => r.status === "blocked"),
        cancelled: count((r) => r.status === "cancelled" && !(r.summary ?? "").startsWith("restarted:")),
        restarted: count((r) => (r.summary ?? "").startsWith("restarted:")),
        continued: count((r) => r.trigger_message_id.startsWith("continue:")),
        interventions: runs.reduce((a, r) => a + r.interventions, 0),
        p50Ms: pct(durations, 50),
        p95Ms: pct(durations, 95),
      },
    };
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
