// Group room side of the bots feature: the AI members working in each topic (participants.ts),
// `run.updated` events, and the hand-off of human messages and card presses to the BotDesks of
// the members they are for: each AI member a message mentions, else the topic's routing owner
// (the member that most recently started a run there), or the DM's other member. The hand-off is
// written to an outbox in the same transaction as the event and delivered after commit; a failed
// delivery is retried by the room alarm until every desk accepted it.
import { ACK_EMOJI, type BlockRef, type BotRun, type CardV2, type Event, isMainTopicId, type Message, RUN_KIND_ASIDE, type TopicContext } from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError } from "../core/errors";
import { ROOM_REACT_RPC } from "../reactions";
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
import { ANONYMOUS_ID, storedPrivacy, viewMessage } from "../cards2/privacy";
import type { AnswerData } from "./answers";
import { CONTEXT_MESSAGES, type ContextMessage } from "./dispatch";
import type { Env } from "../core/env";
import type { Logger } from "../core/logger";
import { deskStub } from "./env";
import { FRONT_DESK_ASK_PREFIX } from "../agent/frontdesk";
import { type RefSource, resolveRefs, stripRefPrefix } from "./refs";
import { type ContextRow, type RequestExt, parseExt, requestExt } from "../context/render";
import { PARTICIPANTS_MIGRATION, type WorkState, addressedParticipants, claimTopic, isParticipant, onRunUpdated, publishWorkState, routingOwner, topicBots, workState } from "./participants";
import { giveUpChainRow, sendChainRow, stepReport } from "./chains";
import { giveUpRelayRow, isRelayMessage, relayReport, sendRelayRow } from "./relay/engine";
import type { RoomContext } from "../core/registry";

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

// Usable (design-relay R1): the AI members that ran in each topic. After bots-1, which it reads.
registerRoomMigration({ name: "participants-1", up: PARTICIPANTS_MIGRATION });

/**
 * The bot a card press in this topic goes to: the card's author when it ran in the topic (in
 * the 群聊, when it has run there), otherwise the topic's routing owner.
 */
export function cardOwner(sql: SqlStorage, topicId: string, authorId: string): string | null {
  if (!isMainTopicId(topicId)) return isParticipant(sql, topicId, authorId) ? authorId : routingOwner(sql, topicId);
  const ran = sql
    .exec("SELECT 1 FROM events WHERE type = 'run.updated' AND topic_id = ? AND actor_id = ? LIMIT 1", topicId, authorId)
    .toArray();
  return ran.length > 0 ? authorId : null;
}

export interface ClaimResult {
  /** The calling bot (several AI members may work in one topic). */
  botId: string;
  title: string;
  messages: ContextMessage[];
  /** The topic's first message (Run context). */
  first?: ContextRow;
  /** The newest messages, newest first (at most `CONTEXT_SCAN`). */
  newest?: ContextRow[];
  /** Messages of the topic (not deleted). */
  total?: number;
  /** The topic's rolling summary (summary/), when it has one. */
  summary?: { text: string; uptoSeq: number };
  /** The topic is a side chat about this topic (1.1, `Topic.contextOf`). */
  contextOf?: TopicContext;
  /** With `contextOf`: the side chat's own person (the DM's other member), the asker. */
  askerId?: string;
}

/** What `bots.context` returns: the side chat's source topic up to `uptoSeq`. */
export interface SourceContext {
  title: string;
  messages: ContextMessage[];
}

/** Newest messages read for a run's prompt; the character budget picks from these. */
export const CONTEXT_SCAN = 400;

type ContextDbRow = { id: string; created_seq: number; author_id: string; created_at: number; body: string; kind: string | null; ext: string | null };
const CONTEXT_COLS = "id, created_seq, author_id, created_at, body, kind, ext";

function toContextRow(r: ContextDbRow): ContextRow {
  return { id: r.id, seq: r.created_seq, authorId: r.author_id, at: r.created_at, body: r.body, kind: r.kind ?? "text", ext: parseExt(r.ext) };
}

/**
 * Messages of a topic as context rows, as `viewerId` may see them (private cards hide their
 * voters): `where` adds conditions on top of topic and not deleted.
 */
export function contextRows(sql: SqlStorage, viewerId: string, topicId: string, where: string, params: unknown[], order: "ASC" | "DESC", limit: number): ContextRow[] {
  return sql
    .exec<ContextDbRow>(
      `SELECT ${CONTEXT_COLS} FROM messages WHERE topic_id = ? AND deleted_at IS NULL ${where} ORDER BY created_seq ${order} LIMIT ?`,
      topicId,
      ...params,
      limit,
    )
    .toArray()
    .map((r) => {
      const row = toContextRow(r);
      if (row.ext.cardState) {
        const seen = viewMessage({ id: row.id, cardState: row.ext.cardState } as unknown as Message, viewerId, sql);
        row.ext = { ...row.ext, cardState: seen.cardState as ContextRow["ext"]["cardState"] };
      }
      if (row.ext.artifactRefs?.length) row.ext = { ...row.ext, artifactRefs: withArtifactTitles(sql, row.ext.artifactRefs) };
      return row;
    });
}

/** 产出 references with the 产出's current title, so a run's lines name what they point at. */
function withArtifactTitles(sql: SqlStorage, refs: NonNullable<ContextRow["ext"]["artifactRefs"]>): NonNullable<ContextRow["ext"]["artifactRefs"]> {
  return refs.map((a) => {
    const title = sql.exec<{ title: string }>("SELECT title FROM artifacts WHERE id = ?", a.artifactId).toArray()[0]?.title;
    return title ? { ...a, title } : a;
  });
}

function topicCount(sql: SqlStorage, topicId: string, where = "", params: unknown[] = []): number {
  return (
    sql.exec<{ n: number }>(`SELECT COUNT(*) AS n FROM messages WHERE topic_id = ? AND deleted_at IS NULL ${where}`, topicId, ...params).toArray()[0]?.n ?? 0
  );
}

/** Messages of a side chat's source topic given to the aside run. */
export const SIDE_CHAT_CONTEXT_MESSAGES = 60;

/**
 * Reads a topic for a run and makes the calling bot its routing owner (`ownerId`: a follow-up
 * without an @ goes to it). Several AI members may work in one topic; the 群聊 has no owner.
 */
registerRoomRpc("bots.claim", (room, actor, args): ClaimResult => {
  // peek: read the topic without becoming its routing owner (the front desk reads first).
  const { topicId, peek } = args as { topicId: string; peek?: boolean };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ title: string; context: string | null }>("SELECT title, context FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const main = isMainTopicId(topicId);
  if (!main && !peek) room.commit((tx) => claimTopic(room, tx, actor.id, topicId));
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
  const out: ClaimResult = { botId: actor.id, title: topic.title, messages };
  out.newest = contextRows(room.sql, actor.id, topicId, "", [], "DESC", CONTEXT_SCAN);
  out.first = contextRows(room.sql, actor.id, topicId, "", [], "ASC", 1)[0];
  out.total = topicCount(room.sql, topicId);
  try {
    const s = room.sql
      .exec<{ text: string; upto_seq: number }>("SELECT text, upto_seq FROM topic_summaries WHERE topic_id = ? AND text != ''", topicId)
      .toArray()[0];
    if (s) out.summary = { text: s.text, uptoSeq: s.upto_seq };
  } catch {
    // No summary table (a room before summary-1 runs its migrations): no summary.
  }
  // Only a DM topic can be a side chat (the room checks this when it stores the context).
  if (topic.context && room.groupKind === "dm") {
    out.contextOf = JSON.parse(topic.context) as TopicContext;
    const asker = room.memberIds().find((id) => id !== actor.id);
    if (asker) out.askerId = asker;
  }
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

/**
 * Messages of a topic after `afterSeq` that are not `exceptAuthor`'s, newest first (a resumed
 * run's new input), with how many there are.
 */
registerRoomRpc("bots.since", (room, actor, args) => {
  const { topicId, afterSeq, exceptAuthor } = args as { topicId: string; afterSeq: number; exceptAuthor: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const where = "AND created_seq > ? AND author_id != ?";
  return {
    newest: contextRows(room.sql, actor.id, topicId, where, [afterSeq, exceptAuthor], "DESC", CONTEXT_SCAN),
    total: topicCount(room.sql, topicId, where, [afterSeq, exceptAuthor]),
  };
});

/** A page of a topic for chat-run `topic.history`, oldest first (machine-protocol.md, "Topic context"). */
registerRoomRpc("bots.history", (room, actor, args) => {
  const { topicId, before, after, limit } = args as { topicId: string; before?: number; after?: number; limit: number };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const conds: string[] = [];
  const params: unknown[] = [];
  if (before !== undefined) {
    conds.push("AND created_seq < ?");
    params.push(before);
  }
  if (after !== undefined) {
    conds.push("AND created_seq > ?");
    params.push(after);
  }
  // Reading forward (after) goes oldest first; otherwise the newest page before the point.
  const forward = after !== undefined && before === undefined;
  const rows = contextRows(room.sql, actor.id, topicId, conds.join(" "), params, forward ? "ASC" : "DESC", limit + 1);
  const hasMore = rows.length > limit;
  const page = rows.slice(0, limit);
  return { rows: forward ? page : page.reverse(), hasMore };
});

/** One message of a topic, whole (chat-run `topic.message`). */
registerRoomRpc("bots.message", (room, actor, args) => {
  const { topicId, messageId } = args as { topicId: string; messageId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const row = contextRows(room.sql, actor.id, topicId, "AND id = ?", [messageId], "ASC", 1)[0];
  if (!row) throw new ApiError("not_found", "message not found");
  return { row };
});

/** The author and body of a message that started a run (retrying a run made before 1.1). */
registerRoomRpc("bots.request", (room, actor, args) => {
  const { messageId } = args as { messageId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const m = room.sql
    .exec<{ author_id: string; body: string }>("SELECT author_id, body FROM messages WHERE id = ? AND deleted_at IS NULL", messageId)
    .toArray()[0];
  return m ? { authorId: m.author_id, body: m.body } : null;
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

/**
 * A run's latest report for its topic (usable, design-relay R1): the desk sends it in place of
 * a `topic.update`; the room keeps it on the bot's row and rolls the topic's status up from all
 * the AI members working there.
 */
registerRoomRpc("bots.workState", (room, actor, args) => {
  const s = args as WorkState;
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const topic = room.sql.exec("SELECT 1 FROM topics WHERE id = ?", s.topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  room.commit((tx) => {
    // design-relay R3: a relay moves on first, so the topic's state written next shows where it is.
    relayReport(room, actor.id, s);
    workState(room, tx, actor.id, s);
    // design-relay R2: the run's step of its chain (result back to the giver, stops).
    stepReport(room, actor.id, s);
  });
  return { ok: true };
});

// Restore from the archive: the routing owner of a topic is the bot of its latest run (the
// event writer below rebuilds the participants).
registerEventImporter((event, sql) => {
  if (event.type !== "run.updated") return;
  const run = (event.data as { run?: BotRun }).run;
  if (!run?.topicId || !run.botId || isMainTopicId(run.topicId) || run.kind === RUN_KIND_ASIDE) return;
  sql.exec(
    `INSERT INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)
     ON CONFLICT (topic_id) DO UPDATE SET bot_id = excluded.bot_id, claimed_at = excluded.claimed_at
     WHERE excluded.bot_id = bot_topics.bot_id OR excluded.claimed_at >= bot_topics.claimed_at`,
    run.topicId,
    run.botId,
    run.createdAt,
  );
});

/** Removes a bot's topic claims and participant rows in this group (the bot was deactivated). Internal. */
registerRoomRpc("bots.release", (room, actor, args) => {
  const { botId } = args as { botId: string };
  if (actor) throw new ApiError("forbidden", "internal only");
  const topics = room.sql.exec<{ topic_id: string }>("SELECT topic_id FROM topic_bots WHERE bot_id = ?", botId).toArray().map((x) => x.topic_id);
  room.sql.exec("DELETE FROM topic_bots WHERE bot_id = ?", botId);
  const r = room.sql.exec("DELETE FROM bot_topics WHERE bot_id = ?", botId);
  // Clients drop the member from the topic's participants and see the status of the rest.
  if (topics.length > 0) {
    room.commit((tx) => {
      for (const t of topics) {
        if (topicBots(room.sql, t).length > 0) publishWorkState(room, tx, botId, t);
        else room.updateTopic(tx, botId, t, { extra: { participants: [] } });
      }
    });
  }
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
  /** The body; with refs, the question without the server's quote lines. */
  body: string;
  mentioned: boolean;
  /** The message the run's acknowledgement reacts to when `messageId` is a synthetic key. */
  ackMessageId?: string;
  /** 1.1 (引用提问): the message's refs, resolved by the room. */
  refs?: RefSource[];
  /** Author of the message this one quotes (`replyTo`), if any. */
  replyToAuthorId?: string;
  /** The message this one quotes (`replyTo`), if any: a reply to an approval card answers it. */
  replyToMessageId?: string;
  /** The message's files, 产出 and the message it quotes: rendered after the body for the run and the front desk. */
  ext?: RequestExt;
  /** The group is a direct chat (私聊) with the bot. */
  dm?: boolean;
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
  actorName: string;
  /** The card at the answered rev. */
  card: CardV2;
  /** A counts-only poll: the votes per option now, in place of the voter's choice. */
  tally?: Record<string, number>;
}

export type MemberInfo = { id: string; name: string; kind: string; level: string; deactivated: number; runner: number };

export async function memberInfo(db: D1Database, ids: string[]): Promise<Map<string, MemberInfo>> {
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

/** Newest messages of a 群聊 searched for an open card ask (接 1.8). */
const ASK_SCAN = 200;

/**
 * 接 1.8: in a 群聊, where nothing routes a message without an @, the members whose front desk
 * question waits on `authorId`: the person's next message goes to them while the question is
 * open and not expired. Other open card asks (a run's own cards) do not route talk: they are
 * answered in the card, or with an @ or a reply (接 1.6).
 */
export function askingMembers(sql: SqlStorage, topicId: string, authorId: string, now = Date.now()): string[] {
  if (!isMainTopicId(topicId)) return [];
  return sql
    .exec<{ author_id: string }>(
      `SELECT DISTINCT m.author_id FROM (
         SELECT author_id, client_msg_id, ext FROM messages WHERE topic_id = ? AND deleted_at IS NULL ORDER BY created_seq DESC LIMIT ?
       ) m, json_each(json_extract(m.ext, '$.cardState.ask.from')) f
       WHERE m.client_msg_id LIKE '${FRONT_DESK_ASK_PREFIX}%' AND json_extract(m.ext, '$.cardState.ask.status') = 'open'
         AND f.value = ? AND m.author_id != ? AND json_extract(m.ext, '$.cardState.ask.expiresAt') > ?`,
      topicId,
      ASK_SCAN,
      authorId,
      authorId,
      now,
    )
    .toArray()
    .map((r) => r.author_id);
}

/** Events older than this are imports (restore from the archive), not live writes: no hand-off. */
const LIVE_WINDOW_MS = 10 * 60_000;
/** A hand-off the desk has not accepted after this long is dropped (and logged). */
export const HANDOFF_GIVE_UP_MS = 60 * 60_000;
const RETRY_MAX_MS = 5 * 60_000;

type OutboxCard = { botId: string; messageId: string; actionId: string; actorId: string };
/** `done`: the bots that already took the message (a retry skips them). */
type OutboxMessage = Omit<TopicMessage, "mentioned" | "groupId" | "refs"> & {
  mentions: string[];
  blockRefs?: BlockRef[];
  done?: string[];
  /** The members it was routed to on its first delivery (a retry goes to the same ones). */
  targets?: { id: string; mentioned: boolean }[];
};
/**
 * The card as it was answered: the author may edit it before the hand-off is delivered.
 * `anonymous`: the card is private (cards2/privacy), so the run gets no actor; such a row holds
 * `ANONYMOUS_ID` as `actorId` (rows written before held the voter) and delivery finds the voter's
 * kind through the card event (`answer.eventId`). `poll`: a poll that shows counts only; the row
 * holds no values and the run gets the tally.
 */
type OutboxAnswer = { botId: string; seq: number; answer: AnswerData; actorId: string; card: CardV2; anonymous?: boolean; poll?: boolean };

/** An answer of a counts-only poll without what the voter chose. */
function withoutValues(a: AnswerData): AnswerData {
  const { value: _v, inputs: _i, editedValues: _e, ...rest } = a;
  return rest;
}

type OutboxRow = {
  id: number;
  /**
   * "artifact" rows (page events before 1.1 made them transient) are dropped. "part" and
   * "cancel" (design-relay R2, chains.ts): a split-off's result for its giver, a stop for a person.
   * "relay" (design-relay R3, relay/engine.ts): a step of a relay for its member.
   */
  kind: "message" | "card" | "answer" | "artifact" | "part" | "cancel" | "relay";
  topic_id: string;
  payload: string;
  attempts: number;
  next_at: number;
  created_at: number;
};

// In the commit transaction: the AI members working in a topic.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type === "run.updated") onRunUpdated(event, sql);
});

// In the commit transaction: the messages and card presses that may go to a bot.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.at < Date.now() - LIVE_WINDOW_MS) return;
  if (event.type === "card.actioned") {
    const { messageId, actionId } = event.data as { messageId: string; actionId: string };
    const row = sql
      .exec<{ author_id: string; topic_id: string }>("SELECT author_id, topic_id FROM messages WHERE id = ?", messageId)
      .toArray()[0];
    if (!row || cardOwner(sql, row.topic_id, row.author_id) !== row.author_id) return;
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
    // A private card's voter never leaves the room, not even in the outbox (cards2/privacy).
    const privacy = storedPrivacy(sql, answer.messageId);
    if (privacy) {
      payload.anonymous = true;
      payload.actorId = ANONYMOUS_ID;
    }
    if (privacy === "poll") {
      payload.poll = true;
      payload.answer = withoutValues(answer);
    }
    enqueue(sql, "answer", stored.topicId, payload);
    return;
  }
  if (event.type !== "message.created") return;
  const message = (event.data as { message: Message }).message;
  if (message.kind === "system") return;
  const owner = routingOwner(sql, message.topicId);
  if (
    message.mentions.length === 0 &&
    (!owner || owner === message.authorId) &&
    !dmPeer(sql, message.authorId) &&
    askingMembers(sql, message.topicId, message.authorId).length === 0
  )
    return;
  const payload: OutboxMessage = {
    topicId: message.topicId,
    messageId: message.id,
    authorId: message.authorId,
    body: message.body,
    mentions: message.mentions,
  };
  if (message.blockRefs && message.blockRefs.length > 0) payload.blockRefs = message.blockRefs;
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

/**
 * One outbox row's hand-offs: the members it goes to and how it goes to one of them. Null: nobody
 * gets the row. Throws when it must be tried again.
 */
interface Plan {
  targets: string[];
  done: string[];
  send(id: string): Promise<unknown>;
  record(done: string[]): void;
}

async function plan(c: DeliverContext, row: OutboxRow): Promise<Plan | null> {
  const single = (botId: string, send: () => Promise<unknown>): Plan => ({ targets: [botId], done: [], send, record: () => {} });
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
    return single(p.botId, () => deskStub(c.env, p.botId).cardAction(p.botId, press));
  }
  if (row.kind === "artifact") return null;
  if (row.kind === "part" || row.kind === "cancel") {
    const p = JSON.parse(row.payload) as { botId: string };
    return single(p.botId, () => sendChainRow(c.env, c.groupId, row.kind, row.payload));
  }
  if (row.kind === "relay") {
    const p = JSON.parse(row.payload) as { botId: string };
    return single(p.botId, () => sendRelayRow(c.env, c.groupId, c.sql, row.payload));
  }
  if (row.kind === "answer") {
    const a = JSON.parse(row.payload) as OutboxAnswer;
    const args = await answerArgs(c, row, a);
    return args ? single(a.botId, () => deskStub(c.env, a.botId).cardAnswer(a.botId, args)) : null;
  }
  const m = JSON.parse(row.payload) as OutboxMessage;
  // design-relay R3: an arrow message started a relay: the room wakes its steps one after the
  // other, so the message goes to none of the members it mentions.
  if (!m.targets && isRelayMessage(c.sql, m.messageId)) return null;
  const peer = dmPeer(c.sql, m.authorId);
  // The routing owner now (a run may have started in the topic since the message was written).
  const owner = m.targets ? null : routingOwner(c.sql, m.topicId);
  const ext = c.sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", m.messageId).toArray()[0]?.ext ?? null;
  const parsed = parseExt(ext);
  const quoted = parsed.replyTo?.authorId;
  // A message without an @ that quotes a participant or answers its approval goes to that member;
  // in a 群聊, one that the front desk's open question waits on goes to the member that asked (接 1.8).
  const addressed =
    !m.targets && m.mentions.length === 0 && !peer
      ? [...addressedParticipants(c.sql, m.topicId, m.authorId, m.body, quoted), ...askingMembers(c.sql, m.topicId, m.authorId)]
      : [];
  if (!m.targets && m.mentions.length === 0 && !owner && !peer && addressed.length === 0) return null;
  const byId = await memberInfo(c.env.DB, [m.authorId, ...m.mentions, ...addressed, ...(owner ? [owner] : []), ...(peer ? [peer] : [])]);
  // A bot's messages never reach another bot: no loops.
  const author = byId.get(m.authorId);
  if (!author || author.kind !== "human" || author.deactivated) return null;
  // Routed once: a retry goes to the same members (and keeps their order with later rows).
  const targets =
    m.targets ??
    messageTargets(
      m,
      owner,
      peer,
      (id) => {
        const x = byId.get(id);
        return Boolean(x && x.kind === "bot" && x.runner && !x.deactivated);
      },
      addressed,
    );
  if (targets.length === 0) return null;
  if (!m.targets) {
    m.targets = targets;
    c.sql.exec("UPDATE bot_outbox SET payload = ? WHERE id = ?", JSON.stringify(m), row.id);
  }
  const base: TopicMessage = {
    groupId: c.groupId,
    topicId: m.topicId,
    messageId: m.messageId,
    authorId: m.authorId,
    authorLevel: author.level as "owner" | "member",
    body: m.body,
    mentioned: false,
  };
  if (m.blockRefs && m.blockRefs.length > 0) {
    base.body = stripRefPrefix(m.body, m.blockRefs);
    base.refs = resolveRefs(c.sql, m.blockRefs);
  }
  if (quoted) base.replyToAuthorId = quoted;
  if (parsed.replyTo?.messageId) base.replyToMessageId = parsed.replyTo.messageId;
  const carried = requestExt(parsed);
  if (carried) base.ext = carried;
  if (peer) base.dm = true;
  const mentioned = new Map(targets.map((t) => [t.id, t.mentioned]));
  return {
    targets: targets.map((t) => t.id),
    done: m.done ?? [],
    send: (id) => deskStub(c.env, id).onMessage(id, { ...base, mentioned: mentioned.get(id) === true }),
    record: (done) => {
      m.done = done;
      if (targets.length > 1) c.sql.exec("UPDATE bot_outbox SET payload = ? WHERE id = ?", JSON.stringify(m), row.id);
    },
  };
}

/**
 * Each member gets its own hand-off, all at once (接 1.9): every target not yet done is tried,
 * one that took it is not handed it again on a retry, and one member's failure does not hold
 * back the others (the row is retried for the failed ones only). A real failure is thrown before
 * a {@link LaneWaits}.
 */
export async function handOffEach<T extends { id: string }>(
  targets: T[],
  alreadyDone: string[],
  send: (t: T) => Promise<unknown>,
  record: (done: string[]) => void,
): Promise<void> {
  const done = new Set(alreadyDone);
  const left = targets.filter((t) => !done.has(t.id));
  const results = await Promise.allSettled(
    left.map(async (t) => {
      await send(t);
      done.add(t.id);
      record([...done]);
    }),
  );
  const failed = results.flatMap((r) => (r.status === "rejected" ? [r.reason as unknown] : []));
  if (failed.length === 0) return;
  const failure = failed.find((e) => !(e instanceof LaneWaits));
  // Only lanes that wait behind an earlier row: the row waits too (deliverRow).
  if (failure === undefined) throw failed[0];
  // 接 6.1: the members whose hand-off failed say so when the room gives up.
  const ids = left.filter((_, i) => results[i].status === "rejected" && !((results[i] as PromiseRejectedResult).reason instanceof LaneWaits)).map((t) => t.id);
  throw new HandOffFailed(failure, ids);
}

/** The members a message could not be handed to (接 6.1: they say so when the room gives up). */
export class HandOffFailed extends Error {
  constructor(
    readonly failure: unknown,
    readonly botIds: string[],
  ) {
    super(failure instanceof Error ? failure.message : String(failure));
    this.name = "HandOffFailed";
  }
}

/** 接 6.1: what a member says in the topic when a message for it was dropped. */
export const UNDELIVERED_TEXT = (excerpt: string) =>
  excerpt ? `「${excerpt}」这条消息一个小时都没能交到我这里，我没有收到。需要的话请再发一次。` : "有一条消息一个小时都没能交到我这里，我没有收到。需要的话请再发一次。";

/**
 * The AI members a message was for when its hand-off failed before it reached any desk (the
 * lookup itself failed): the DM peer, else the members it mentions, else the routing owner (as
 * `messageTargets`), only active AI members.
 */
async function likelyBots(c: DeliverContext, m: OutboxMessage): Promise<string[]> {
  const peer = dmPeer(c.sql, m.authorId);
  const owner = routingOwner(c.sql, m.topicId);
  const ids = [...new Set(peer ? [peer] : m.mentions.length > 0 ? m.mentions : owner ? [owner] : [])].filter((id) => id !== m.authorId);
  if (ids.length === 0) return [];
  const rows = await c.env.DB.prepare(`SELECT id FROM members WHERE kind = 'bot' AND deactivated = 0 AND id IN (${ids.map(() => "?").join(",")})`)
    .bind(...ids)
    .all<{ id: string }>();
  return rows.results.map((r) => r.id);
}

/**
 * 接 6.1: the room gave up handing a row to its members: each one says so in the topic and takes
 * back its 👀 on the message. Bots only (a card press or answer names its bot; a message the
 * members that failed). Never throws.
 */
async function undelivered(c: DeliverContext, row: OutboxRow, err: unknown): Promise<void> {
  try {
    const p = JSON.parse(row.payload) as Partial<OutboxMessage & OutboxCard & OutboxAnswer>;
    const botIds = err instanceof HandOffFailed ? err.botIds : row.kind !== "message" ? (p.botId ? [p.botId] : []) : await likelyBots(c, p as OutboxMessage);
    const messageId = row.kind === "answer" ? p.answer?.messageId : p.messageId;
    const excerpt = row.kind === "message" && typeof p.body === "string" ? [...p.body.replace(/\s+/g, " ").trim()].slice(0, 30).join("") : "";
    const room = roomStub(c.env, c.groupId);
    for (const botId of botIds) {
      const actor = { id: botId, level: "member" as const, kind: "bot" as const };
      if (messageId) await room.rpc(c.groupId, ROOM_REACT_RPC, actor, { messageId, emoji: ACK_EMOJI, on: false }).catch(() => undefined);
      const sent = await room.send(c.groupId, actor, {
        t: "send",
        rid: "bot",
        groupId: c.groupId,
        topicId: row.topic_id,
        clientMsgId: `undelivered:${row.id}:${botId}`.slice(0, 64),
        body: UNDELIVERED_TEXT(excerpt),
      } as never);
      if (!sent.ok) c.log.warn("undelivered note failed", { topicId: row.topic_id, err: sent.error });
    }
  } catch (e) {
    c.log.warn("undelivered note failed", { topicId: row.topic_id, err: e });
  }
}

/**
 * Who a person's message goes to (接 1.1, 1.2): in a DM the other member; otherwise each AI
 * member it mentions, each on its own (`mentioned`); a message without any @ to the participants
 * it quotes or answers (`addressed`), else to the topic's routing owner (`mentioned: false`: the
 * desk decides whether it is a follow-up). Only usable runner bots.
 */
export function messageTargets(
  m: { authorId: string; mentions: string[] },
  owner: string | null,
  peer: string | undefined,
  usable: (id: string) => boolean,
  addressed: string[] = [],
): { id: string; mentioned: boolean }[] {
  if (peer) return usable(peer) ? [{ id: peer, mentioned: true }] : [];
  const named = [...new Set(m.mentions)].filter((id) => id !== m.authorId && usable(id));
  if (named.length > 0) return named.map((id) => ({ id, mentioned: true }));
  const to = m.mentions.length === 0 ? [...new Set(addressed)].filter((id) => id !== m.authorId && usable(id)) : [];
  if (to.length > 0) return to.map((id) => ({ id, mentioned: false }));
  if (m.mentions.length === 0 && owner && owner !== m.authorId && usable(owner)) return [{ id: owner, mentioned: false }];
  return [];
}

/** The voter of a stored answer, from the room's card event (private cards keep it only there). */
function answerActor(sql: SqlStorage, a: OutboxAnswer): string {
  if (a.actorId !== ANONYMOUS_ID || !a.answer.eventId) return a.actorId;
  return sql.exec<{ actor_id: string }>("SELECT actor_id FROM card_events WHERE event_id = ?", a.answer.eventId).toArray()[0]?.actor_id ?? a.actorId;
}

/** The tally of a poll card now (cards2 state), for a counts-only poll's author. */
function currentTally(sql: SqlStorage, messageId: string): Record<string, number> | undefined {
  const row = sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", messageId).toArray()[0];
  if (!row?.ext) return undefined;
  return (JSON.parse(row.ext) as { cardState?: { tally?: Record<string, number> } }).cardState?.tally;
}

/** A card answer, for the runner bot that made the card; null for anyone else. */
async function answerArgs(c: DeliverContext, row: OutboxRow, a: OutboxAnswer): Promise<CardAnswerIn | null> {
  const voter = answerActor(c.sql, a);
  const byId = await memberInfo(c.env.DB, [a.botId, voter]);
  const bot = byId.get(a.botId);
  // SDK bots and people read the stored event; only runner bots need a hand-off.
  if (!bot || bot.kind !== "bot" || !bot.runner || bot.deactivated) return null;
  const who = byId.get(voter);
  const args: CardAnswerIn = {
    groupId: c.groupId,
    topicId: row.topic_id,
    seq: a.seq,
    // Rows written before the poll rule may still hold the values.
    answer: a.poll ? withoutValues(a.answer) : a.answer,
    actorId: a.anonymous ? ANONYMOUS_ID : a.actorId,
    actorKind: who && !who.deactivated ? (who.kind as "human" | "bot") : null,
    actorLevel: who && !who.deactivated && !a.anonymous ? (who.level as "owner" | "member") : null,
    actorName: a.anonymous ? ANONYMOUS_ID : (who?.name ?? a.actorId),
    card: a.card,
  };
  if (a.poll) {
    const tally = currentTally(c.sql, a.answer.messageId);
    if (tally) args.tally = tally;
  }
  return args;
}

/**
 * 接 1.9: hand-offs run in lanes, one per (topic, member): in order within a lane, lanes at the
 * same time, so one member that thinks (a front desk call takes up to 30 s) does not hold back
 * the others. Per room object: the rows being delivered, each lane's tail and each topic's
 * routing step (the rows of a topic are routed in order, so they join each lane in order).
 */
interface Lane {
  tail: Promise<void>;
  pending: number;
  /** The row whose hand-off failed in this lane; later hand-offs wait until its retry is stored. */
  failedBy: number | null;
}
interface DrainState {
  inflight: Set<number>;
  lanes: Map<string, Lane>;
  topics: Map<string, Promise<unknown>>;
  /** Each topic's rows without an @ that wait for the topic to be idle before they are routed. */
  routes: Map<string, Promise<unknown>>;
}
const drains = new WeakMap<SqlStorage, DrainState>();

/** A hand-off that waits for an earlier row of its lane (no attempt is counted). */
export class LaneWaits extends Error {
  constructor() {
    super("an earlier hand-off of this lane waits for its retry");
  }
}

function drainState(sql: SqlStorage): DrainState {
  let st = drains.get(sql);
  if (!st) {
    st = { inflight: new Set(), lanes: new Map(), topics: new Map(), routes: new Map() };
    drains.set(sql, st);
  }
  return st;
}

const laneKey = (topicId: string, memberId: string) => `${topicId}\u0000${memberId}`;

/** Runs `fn` after the earlier hand-offs of the lane; it waits instead while an earlier one failed. */
function onLane(st: DrainState, key: string, rowId: number, fn: () => Promise<unknown>): Promise<void> {
  let lane = st.lanes.get(key);
  if (!lane) {
    lane = { tail: Promise.resolve(), pending: 0, failedBy: null };
    st.lanes.set(key, lane);
  }
  const l = lane;
  l.pending++;
  const run = l.tail.then(async () => {
    if (l.failedBy !== null) throw new LaneWaits();
    try {
      await fn();
    } catch (err) {
      l.failedBy = rowId;
      throw err;
    }
  });
  l.tail = run.then(
    () => {},
    () => {},
  );
  void l.tail.then(() => {
    l.pending--;
    if (l.pending === 0 && l.failedBy === null && st.lanes.get(key) === l) st.lanes.delete(key);
  });
  return run;
}

/** After a row's retry is stored: the stored row holds its lanes from now on. */
function releaseLanes(st: DrainState, rowId: number): void {
  for (const [key, lane] of st.lanes) {
    if (lane.failedBy !== rowId) continue;
    lane.failedBy = null;
    if (lane.pending === 0) st.lanes.delete(key);
  }
}

/** Waits for the topic's hand-offs under way. */
async function topicIdle(st: DrainState, topicId: string): Promise<void> {
  const prefix = laneKey(topicId, "");
  await Promise.all([...st.lanes].filter(([k]) => k.startsWith(prefix)).map(([, l]) => l.tail));
}

/**
 * The lanes held by rows that wait for a retry (their members not yet done); a row that was never
 * routed holds its whole topic.
 */
function heldLanes(sql: SqlStorage, now: number): { lanes: Set<string>; topics: Set<string> } {
  const lanes = new Set<string>();
  const topics = new Set<string>();
  for (const r of sql.exec<OutboxRow>("SELECT * FROM bot_outbox WHERE next_at > ?", now).toArray()) {
    const p = JSON.parse(r.payload) as { botId?: string; targets?: { id: string }[]; done?: string[] };
    if (r.kind !== "message" && p.botId) {
      lanes.add(laneKey(r.topic_id, p.botId));
      continue;
    }
    if (r.kind !== "message" || !p.targets) {
      topics.add(r.topic_id);
      continue;
    }
    for (const t of p.targets) if (!(p.done ?? []).includes(t.id)) lanes.add(laneKey(r.topic_id, t.id));
  }
  return { lanes, topics };
}

/**
 * A message without an @ in a 话题 goes where the topic routes it, and the routing owner may change
 * when an earlier hand-off starts a run: it is routed once the topic's earlier hand-offs are done.
 * The 群聊 and a DM have no routing owner to wait for.
 */
function routesLater(c: DeliverContext, row: OutboxRow): boolean {
  if (row.kind !== "message" || isMainTopicId(row.topic_id)) return false;
  const m = JSON.parse(row.payload) as OutboxMessage;
  return !m.targets && m.mentions.length === 0 && !dmPeer(c.sql, m.authorId);
}

/** Delivers one row through its lanes, then deletes it, or stores its retry. */
async function deliverRow(c: DeliverContext, st: DrainState, row: OutboxRow, held: Set<string>): Promise<void> {
  try {
    // The rows of a topic are routed one after another, so their hand-offs join each lane in
    // order. A row that waits for the topic to be idle before it is routed (routesLater) waits on
    // its own chain: later rows with fixed members (an @, a DM) do not wait behind it.
    const later = routesLater(c, row);
    const chains = later ? st.routes : st.topics;
    const before = later ? Promise.all([st.topics.get(row.topic_id), st.routes.get(row.topic_id)]).then(() => topicIdle(st, row.topic_id)) : (st.topics.get(row.topic_id) ?? Promise.resolve());
    const step = before.then(async () => {
      const p = await plan(c, row);
      if (!p) return null;
      const started = new Map<string, Promise<void>>();
      for (const id of p.targets) {
        if (p.done.includes(id)) continue;
        const key = laneKey(row.topic_id, id);
        const go = held.has(key) ? Promise.reject(new LaneWaits()) : onLane(st, key, row.id, () => p.send(id));
        go.catch(() => {});
        started.set(id, go);
      }
      return { p, started };
    });
    const tail = step.catch(() => {});
    chains.set(row.topic_id, tail);
    void tail.then(() => {
      if (chains.get(row.topic_id) === tail) chains.delete(row.topic_id);
    });
    const routed = await step;
    if (routed) {
      const { p, started } = routed;
      await handOffEach(
        p.targets.map((id) => ({ id })),
        p.done,
        (t) => started.get(t.id) ?? Promise.resolve(),
        (done) => p.record(done),
      );
    }
    c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
  } catch (err) {
    // Behind an earlier row of the same member: it stays due and goes after that one.
    if (err instanceof LaneWaits) return;
    const attempts = row.attempts + 1;
    if (Date.now() - row.created_at > HANDOFF_GIVE_UP_MS) {
      if (row.kind === "part" || row.kind === "cancel" || row.kind === "relay") {
        // Giving up writes the topic: only the room's alarm can (an after-commit hook cannot).
        if (!(c as Partial<RoomContext>).commit) {
          c.sql.exec("UPDATE bot_outbox SET attempts = ?, next_at = ? WHERE id = ?", attempts, Date.now() + 1000, row.id);
          c.wakeAt(Date.now() + 1000);
          return;
        }
        if (row.kind === "relay") giveUpRelayRow(c as RoomContext, c.log, row.payload);
        else giveUpChainRow(c as RoomContext, c.log, row.kind, row.payload);
      }
      c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
      c.log.error("bot hand-off dropped", { err, kind: row.kind, topicId: row.topic_id, attempts });
      await undelivered(c, row, err);
      return;
    }
    const wait = Math.min(1000 * 2 ** Math.min(attempts, 20), RETRY_MAX_MS);
    c.sql.exec("UPDATE bot_outbox SET attempts = ?, next_at = ? WHERE id = ?", attempts, Date.now() + wait, row.id);
    c.log.warn("bot hand-off failed; will retry", { err, kind: row.kind, topicId: row.topic_id, attempts });
  } finally {
    releaseLanes(st, row.id);
    st.inflight.delete(row.id);
  }
}

/**
 * Delivers every due outbox row (接 1.9): each as soon as its lanes are free, in order within a
 * (topic, member) lane. A failure holds the later rows of the same lane (their order matters to
 * the run) and schedules a retry with backoff through the alarm.
 */
export async function drainOutbox(c: DeliverContext): Promise<void> {
  const st = drainState(c.sql);
  const now = Date.now();
  const held = heldLanes(c.sql, now);
  const due = c.sql.exec<OutboxRow>("SELECT * FROM bot_outbox WHERE next_at <= ? ORDER BY id LIMIT 200", now).toArray();
  const jobs: Promise<void>[] = [];
  for (const row of due) {
    if (st.inflight.has(row.id)) continue;
    if (held.topics.has(row.topic_id)) continue;
    st.inflight.add(row.id);
    jobs.push(deliverRow(c, st, row, held.lanes));
  }
  await Promise.all(jobs);
  // Rows under way are not due again: the drain that runs them wakes the room when they fail.
  let next: number | null = null;
  for (const r of c.sql.exec<{ id: number; next_at: number }>("SELECT id, next_at FROM bot_outbox").toArray()) {
    if (!st.inflight.has(r.id) && (next === null || r.next_at < next)) next = r.next_at;
  }
  if (next !== null) c.wakeAt(Math.max(next, Date.now() + 1000));
}

// run.updated, topic.updated and group.memberRemoved: chain rows (part, cancel) go out at once.
const HANDED_OFF = new Set(["message.created", "card.actioned", "card.answered", "run.updated", "topic.updated", "group.memberRemoved"]);

registerAfterCommit(async (event: Event, h) => {
  if (!HANDED_OFF.has(event.type)) return;
  await drainOutbox(h);
});

registerRoomAlarm((room) => drainOutbox(room));
