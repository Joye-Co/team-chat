// Group room side of the bots feature: the one responsible bot per topic, `run.updated` events,
// and the hand-off of human messages and card presses to the responsible bot's BotDesk. The
// hand-off is written to an outbox in the same transaction as the event and delivered after
// commit; a failed delivery is retried by the room alarm until the desk accepts it.
import type { BotRun, CardV2, Event, Message, TopicContext } from "@chat/protocol";
import { ApiError } from "../core/errors";
import {
  type ArchivedEvent,
  registerAfterCommit,
  registerEventImporter,
  registerEventWriter,
  registerRoomAlarm,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";
import { storedCard } from "../cards2/decisions";
import { type AnswerData, type PageEventData, answerText, pageEventText } from "./answers";
import { CONTEXT_MESSAGES, type ContextMessage } from "./dispatch";
import type { Env } from "../core/env";
import type { Logger } from "../core/logger";
import { deskStub } from "./env";

registerRoomMigration({
  name: "bots-1",
  up: `
    CREATE TABLE bot_topics (
      topic_id TEXT PRIMARY KEY,
      bot_id TEXT NOT NULL,
      claimed_at INTEGER NOT NULL
    );
  `,
});

registerRoomMigration({
  name: "bots-2",
  up: `
    CREATE TABLE bot_outbox (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      kind TEXT NOT NULL,
      topic_id TEXT NOT NULL,
      payload TEXT NOT NULL,
      attempts INTEGER NOT NULL DEFAULT 0,
      next_at INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL
    );
  `,
});

function claimedBot(sql: SqlStorage, topicId: string): string | null {
  return (
    sql.exec<{ bot_id: string }>("SELECT bot_id FROM bot_topics WHERE topic_id = ?", topicId).toArray()[0]?.bot_id ?? null
  );
}

export interface ClaimResult {
  /** The topic's responsible bot after the claim (this bot, or the one that was first). */
  botId: string;
  title: string;
  messages: ContextMessage[];
  /** The topic is a side chat about this topic (1.1, `Topic.contextOf`). */
  contextOf?: TopicContext;
}

/** What `bots.context` returns: the side chat's source topic up to `uptoSeq`. */
export interface SourceContext {
  title: string;
  messages: ContextMessage[];
}

/** Messages of a side chat's source topic given to the aside run. */
export const SIDE_CHAT_CONTEXT_MESSAGES = 60;

/** Makes the calling bot the topic's responsible bot unless another bot already is. */
registerRoomRpc("bots.claim", (room, actor, args): ClaimResult => {
  const { topicId } = args as { topicId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ title: string; context: string | null }>("SELECT title, context FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  room.sql.exec(
    "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
    topicId,
    actor.id,
    Date.now(),
  );
  const messages = room.sql
    .exec<{ id: string; author_id: string; body: string }>(
      `SELECT id, author_id, body FROM messages WHERE topic_id = ? AND deleted_at IS NULL
       ORDER BY created_seq DESC LIMIT ?`,
      topicId,
      CONTEXT_MESSAGES,
    )
    .toArray()
    .reverse()
    .map((m) => ({ id: m.id, authorId: m.author_id, body: m.body }));
  const out: ClaimResult = { botId: claimedBot(room.sql, topicId) as string, title: topic.title, messages };
  // Only a DM topic can be a side chat (the room checks this when it stores the context).
  if (topic.context && room.groupKind === "dm") out.contextOf = JSON.parse(topic.context) as TopicContext;
  return out;
});

/**
 * The source topic of a side chat, read by its aside run: messages up to `uptoSeq`, oldest
 * first. The calling bot and the asker must both still be members of this group.
 */
registerRoomRpc("bots.context", (room, actor, args): SourceContext => {
  const { topicId, uptoSeq, askerId } = args as { topicId: string; uptoSeq: number; askerId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  if (!room.isMember(actor.id) || !room.isMember(askerId)) throw new ApiError("forbidden", "not a member of this group");
  const topic = room.sql.exec<{ title: string }>("SELECT title FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const messages = room.sql
    .exec<{ id: string; author_id: string; body: string }>(
      `SELECT id, author_id, body FROM messages WHERE topic_id = ? AND deleted_at IS NULL AND created_seq <= ?
       ORDER BY created_seq DESC LIMIT ?`,
      topicId,
      uptoSeq,
      SIDE_CHAT_CONTEXT_MESSAGES,
    )
    .toArray()
    .reverse()
    .map((m) => ({ id: m.id, authorId: m.author_id, body: m.body }));
  return { title: topic.title, messages };
});

/** Checks that a topic exists in this group (external bots start runs in it). */
registerRoomRpc("bots.topicExists", (room, actor, args) => {
  const { topicId } = args as { topicId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ id: string }>("SELECT id FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  return { ok: true };
});

/** Stores a `run.updated` event (actor = the bot) for clients. */
registerRoomRpc("bots.runUpdated", (room, actor, args) => {
  const { run } = args as { run: BotRun };
  if (!actor || actor.id !== run.botId) throw new ApiError("forbidden", "only the run's bot");
  const event = room.commit((tx) =>
    tx.append({ type: "run.updated", actorId: actor.id, topicId: run.topicId, data: { run } }),
  );
  return { seq: event.seq };
});

// Restore from the archive: the responsible bot of a topic is the bot of its first run.
registerEventImporter((event, sql) => {
  if (event.type !== "run.updated") return;
  const run = (event.data as { run?: BotRun }).run;
  if (!run?.topicId || !run.botId) return;
  sql.exec(
    "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
    run.topicId,
    run.botId,
    event.at,
  );
});

/** Removes a bot's topic claims in this group (the bot was deactivated). Internal. */
registerRoomRpc("bots.release", (room, actor, args) => {
  const { botId } = args as { botId: string };
  if (actor) throw new ApiError("forbidden", "internal only");
  const r = room.sql.exec("DELETE FROM bot_topics WHERE bot_id = ?", botId);
  return { released: r.rowsWritten };
});

/** What BotDesk.onMessage gets. */
export interface TopicMessage {
  groupId: string;
  topicId: string;
  messageId: string;
  authorId: string;
  /** The author's level, for approvals by text (only the requester or an owner). */
  authorLevel?: "owner" | "member";
  body: string;
  mentioned: boolean;
  /** The message the run's acknowledgement reacts to when `messageId` is a synthetic key. */
  ackMessageId?: string;
}

/** Who pressed a card button, as BotDesk.cardAction gets it. */
export interface CardPress {
  groupId: string;
  messageId: string;
  actionId: string;
  actorId: string;
  actorKind: "human" | "bot" | null;
  actorLevel: "owner" | "member" | null;
}

/** A Card v2 answer for the card's author, as BotDesk.cardAnswer gets it. */
export interface CardAnswerIn {
  groupId: string;
  topicId: string;
  /** Seq of the `card.answered` event: the answer's identity. */
  seq: number;
  answer: AnswerData;
  actorId: string;
  actorKind: "human" | "bot" | null;
  actorLevel: "owner" | "member" | null;
  /** The text form for the run (machine-protocol.md `run.input.text`). */
  text: string;
}

/** A 产出 page event for the bot that made the version, as BotDesk.artifactEvent gets it. */
export interface PageEventIn {
  groupId: string;
  topicId: string;
  seq: number;
  event: PageEventData;
  /** The run that made the version, when the bot named it. */
  runId: string | null;
  actorId: string;
  actorKind: "human" | "bot" | null;
  text: string;
}

type MemberInfo = { id: string; name: string; kind: string; level: string; deactivated: number; runner: number };

async function memberInfo(db: D1Database, ids: string[]): Promise<Map<string, MemberInfo>> {
  const uniq = [...new Set(ids)];
  const rows = await db
    .prepare(
      `SELECT m.id, m.name, m.kind, m.level, m.deactivated, r.member_id IS NOT NULL AS runner FROM members m
       LEFT JOIN bot_runners r ON r.member_id = m.id AND r.enabled = 1
       WHERE m.id IN (${uniq.map(() => "?").join(",")})`,
    )
    .bind(...uniq)
    .all<MemberInfo>();
  return new Map(rows.results.map((r) => [r.id, r]));
}

/** In a DM, the member other than `authorId` (a DM message is addressed to it, @ or not). */
function dmPeer(sql: SqlStorage, authorId: string): string | undefined {
  const kind = sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'group_kind'").toArray()[0]?.value;
  if (kind !== "dm") return undefined;
  return sql
    .exec<{ member_id: string }>("SELECT member_id FROM room_members WHERE member_id != ? LIMIT 1", authorId)
    .toArray()[0]?.member_id;
}

/** Events older than this are imports (restore from the archive), not live writes: no hand-off. */
const LIVE_WINDOW_MS = 10 * 60_000;
/** A hand-off the desk has not accepted after this long is dropped (and logged). */
export const HANDOFF_GIVE_UP_MS = 60 * 60_000;
const RETRY_MAX_MS = 5 * 60_000;

type OutboxCard = { botId: string; messageId: string; actionId: string; actorId: string };
type OutboxMessage = Omit<TopicMessage, "mentioned" | "groupId"> & { mentions: string[] };
/** The card as it was answered: the author may edit it before the hand-off is delivered. */
type OutboxAnswer = { botId: string; seq: number; answer: AnswerData; actorId: string; card: CardV2 };
type OutboxPageEvent = { botId: string; seq: number; event: PageEventData; runId: string | null; actorId: string; title: string | null };

type OutboxRow = {
  id: number;
  kind: "message" | "card" | "answer" | "artifact";
  topic_id: string;
  payload: string;
  attempts: number;
  next_at: number;
  created_at: number;
};

// In the commit transaction: the messages and card presses that may go to a bot.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.at < Date.now() - LIVE_WINDOW_MS) return;
  if (event.type === "card.actioned") {
    const { messageId, actionId } = event.data as { messageId: string; actionId: string };
    const row = sql
      .exec<{ author_id: string; topic_id: string }>("SELECT author_id, topic_id FROM messages WHERE id = ?", messageId)
      .toArray()[0];
    if (!row || claimedBot(sql, row.topic_id) !== row.author_id) return;
    const payload: OutboxCard = { botId: row.author_id, messageId, actionId, actorId: event.actorId };
    enqueue(sql, "card", row.topic_id, payload);
    return;
  }
  if (event.type === "card.answered") {
    // To the card's author; deliver() keeps only runner bots (SDK bots read the event itself).
    const answer = event.data as AnswerData;
    const stored = storedCard(sql, answer.messageId);
    if (!stored || stored.authorId === event.actorId) return;
    const payload: OutboxAnswer = { botId: stored.authorId, seq: event.seq, answer, actorId: event.actorId, card: stored.card };
    enqueue(sql, "answer", stored.topicId, payload);
    return;
  }
  if (event.type === "artifact.evented") {
    // To the bot that made the version (only versions that declare events get these events).
    const e = event.data as PageEventData;
    const v = sql
      .exec<{ created_by: string; run_id: string | null; topic_id: string; title: string }>(
        `SELECT v.created_by, v.run_id, a.topic_id, a.title FROM artifact_versions v JOIN artifacts a ON a.id = v.artifact_id
         WHERE v.artifact_id = ? AND v.version = ?`,
        e.artifactId,
        e.version,
      )
      .toArray()[0];
    if (!v || v.created_by === event.actorId) return;
    const payload: OutboxPageEvent = { botId: v.created_by, seq: event.seq, event: e, runId: v.run_id, actorId: event.actorId, title: v.title };
    enqueue(sql, "artifact", v.topic_id, payload);
    return;
  }
  if (event.type !== "message.created") return;
  const message = (event.data as { message: Message }).message;
  if (message.kind === "system") return;
  const claimed = claimedBot(sql, message.topicId);
  if (claimed === message.authorId) return;
  if (!claimed && message.mentions.length === 0 && !dmPeer(sql, message.authorId)) return;
  const payload: OutboxMessage = {
    topicId: message.topicId,
    messageId: message.id,
    authorId: message.authorId,
    body: message.body,
    mentions: message.mentions,
  };
  enqueue(sql, "message", message.topicId, payload);
});

function enqueue(sql: SqlStorage, kind: OutboxRow["kind"], topicId: string, payload: unknown): void {
  sql.exec(
    "INSERT INTO bot_outbox (kind, topic_id, payload, created_at) VALUES (?, ?, ?, ?)",
    kind,
    topicId,
    JSON.stringify(payload),
    Date.now(),
  );
}

interface DeliverContext {
  env: Env;
  groupId: string;
  sql: SqlStorage;
  log: Logger;
  wakeAt(at: number): void;
}

/** Hands one outbox row to the bot's desk. Throws when it must be tried again. */
async function deliver(c: DeliverContext, row: OutboxRow): Promise<void> {
  if (row.kind === "card") {
    const p = JSON.parse(row.payload) as OutboxCard;
    // Who pressed: BotDesk takes an approval only from a person who may answer it.
    const who = (await memberInfo(c.env.DB, [p.actorId])).get(p.actorId);
    const press: CardPress = {
      groupId: c.groupId,
      messageId: p.messageId,
      actionId: p.actionId,
      actorId: p.actorId,
      actorKind: who && !who.deactivated ? (who.kind as "human" | "bot") : null,
      actorLevel: who && !who.deactivated ? (who.level as "owner" | "member") : null,
    };
    await deskStub(c.env, p.botId).cardAction(p.botId, press);
    return;
  }
  if (row.kind === "answer" || row.kind === "artifact") return deliverToAuthor(c, row);
  const m = JSON.parse(row.payload) as OutboxMessage;
  // The responsible bot now (a run may have claimed the topic since the message was written).
  const claimed = claimedBot(c.sql, m.topicId);
  if (claimed === m.authorId) return;
  const peer = dmPeer(c.sql, m.authorId);
  if (!claimed && m.mentions.length === 0 && !peer) return;
  const byId = await memberInfo(c.env.DB, [m.authorId, ...m.mentions, ...(claimed ? [claimed] : []), ...(peer ? [peer] : [])]);
  const usable = (id: string | undefined) => {
    const x = id ? byId.get(id) : undefined;
    return Boolean(x && x.kind === "bot" && x.runner && !x.deactivated);
  };
  // A bot's messages never reach another bot: no loops.
  const author = byId.get(m.authorId);
  if (!author || author.kind !== "human" || author.deactivated) return;
  let target: string | undefined;
  if (claimed) {
    // A deactivated (or runner-disabled) bot gets no more work in topics it claimed.
    if (!usable(claimed)) return;
    target = claimed;
  } else {
    target = m.mentions.find((id) => usable(id)) ?? (usable(peer) ? peer : undefined);
  }
  if (!target) return;
  const args: TopicMessage = {
    groupId: c.groupId,
    topicId: m.topicId,
    messageId: m.messageId,
    authorId: m.authorId,
    authorLevel: author.level as "owner" | "member",
    body: m.body,
    mentioned: m.mentions.includes(target) || target === peer,
  };
  await deskStub(c.env, target).onMessage(target, args);
}

/** A card answer or a page event, to the runner bot that made the card or the 产出 version. */
async function deliverToAuthor(c: DeliverContext, row: OutboxRow): Promise<void> {
  const p = JSON.parse(row.payload) as OutboxAnswer | OutboxPageEvent;
  const byId = await memberInfo(c.env.DB, [p.botId, p.actorId]);
  const bot = byId.get(p.botId);
  // SDK bots and people read the stored event; only runner bots need a hand-off.
  if (!bot || bot.kind !== "bot" || !bot.runner || bot.deactivated) return;
  const who = byId.get(p.actorId);
  const actorKind = who && !who.deactivated ? (who.kind as "human" | "bot") : null;
  const name = who?.name ?? p.actorId;
  if (row.kind === "answer") {
    const a = p as OutboxAnswer;
    const args: CardAnswerIn = {
      groupId: c.groupId,
      topicId: row.topic_id,
      seq: a.seq,
      answer: a.answer,
      actorId: a.actorId,
      actorKind,
      actorLevel: who && !who.deactivated ? (who.level as "owner" | "member") : null,
      text: answerText(a.card, a.answer, name),
    };
    await deskStub(c.env, a.botId).cardAnswer(a.botId, args);
    return;
  }
  const e = p as OutboxPageEvent;
  const args: PageEventIn = {
    groupId: c.groupId,
    topicId: row.topic_id,
    seq: e.seq,
    event: e.event,
    runId: e.runId,
    actorId: e.actorId,
    actorKind,
    text: pageEventText(e.event, name, e.title),
  };
  await deskStub(c.env, e.botId).artifactEvent(e.botId, args);
}

/** Rooms with a drain running (one per room object; the alarm and hooks share it). */
const draining = new WeakSet<SqlStorage>();

/**
 * Delivers every due outbox row in order. A failure delays the later rows of the same topic
 * (their order matters to the run) and schedules a retry with backoff through the alarm.
 */
export async function drainOutbox(c: DeliverContext): Promise<void> {
  if (draining.has(c.sql)) return;
  draining.add(c.sql);
  try {
    // Rows written while a delivery was in flight are picked up by the next pass.
    for (let pass = 0; pass < 10; pass++) {
      const now = Date.now();
      const due = c.sql
        .exec<OutboxRow>("SELECT * FROM bot_outbox WHERE next_at <= ? ORDER BY id LIMIT 100", now)
        .toArray();
      if (due.length === 0) break;
      const blocked = new Set(
        c.sql
          .exec<{ topic_id: string }>("SELECT DISTINCT topic_id FROM bot_outbox WHERE next_at > ?", now)
          .toArray()
          .map((r) => r.topic_id),
      );
      let delivered = 0;
      for (const row of due) {
        if (blocked.has(row.topic_id)) continue;
        try {
          await deliver(c, row);
          c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
          delivered++;
        } catch (err) {
          blocked.add(row.topic_id);
          const attempts = row.attempts + 1;
          if (Date.now() - row.created_at > HANDOFF_GIVE_UP_MS) {
            c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
            c.log.error("bot hand-off dropped", { err, kind: row.kind, topicId: row.topic_id, attempts });
            continue;
          }
          const wait = Math.min(1000 * 2 ** Math.min(attempts, 20), RETRY_MAX_MS);
          c.sql.exec("UPDATE bot_outbox SET attempts = ?, next_at = ? WHERE id = ?", attempts, Date.now() + wait, row.id);
          c.log.warn("bot hand-off failed; will retry", { err, kind: row.kind, topicId: row.topic_id, attempts });
        }
      }
      if (delivered === 0) break;
    }
  } finally {
    draining.delete(c.sql);
  }
  const next = c.sql.exec<{ at: number | null }>("SELECT MIN(next_at) AS at FROM bot_outbox").toArray()[0]?.at;
  if (next !== null && next !== undefined) c.wakeAt(Math.max(next, Date.now() + 1000));
}

const HANDED_OFF = new Set(["message.created", "card.actioned", "card.answered", "artifact.evented"]);

registerAfterCommit(async (event: Event, h) => {
  if (!HANDED_OFF.has(event.type)) return;
  await drainOutbox(h);
});

registerRoomAlarm((room) => drainOutbox(room));
