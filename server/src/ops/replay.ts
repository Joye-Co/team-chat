// Rebuilds the core room tables (topics, messages, mentions, follow flags) from imported archive
// events, exactly as the room's write paths in server/src/core/room.ts build them. Keep in step.
// Membership is not replayed: on restore the room takes its members from D1, which is restored
// first. Feature modules register their own importers for their tables and event types.
import { isMainTopicId } from "@chat/protocol";
import { type ArchivedEvent, registerEventImporter } from "../core/registry";
import { scrubDeletedMessage } from "../core/scrub";
import { setFollowing } from "../core/topicstate";

const CORE_MESSAGE_KEYS = new Set([
  "id",
  "clientMsgId",
  "groupId",
  "topicId",
  "authorId",
  "body",
  "mentions",
  "createdSeq",
  "createdAt",
  "editedAt",
  "deletedAt",
  "streaming",
  "kind",
]);
const CORE_UPDATE_KEYS = new Set(["messageId", "body", "editedAt", "streaming"]);

type Obj = Record<string, unknown>;

function extOf(obj: Obj, core: Set<string>): Obj | null {
  const ext: Obj = {};
  let n = 0;
  for (const [k, v] of Object.entries(obj)) {
    if (core.has(k) || v === undefined) continue;
    ext[k] = v;
    n++;
  }
  return n > 0 ? ext : null;
}

function touch(sql: SqlStorage, topicId: string | null | undefined, seq: number, addMessage = false): void {
  if (!topicId) return;
  sql.exec(
    "UPDATE topics SET last_seq = ?, message_count = message_count + ? WHERE id = ?",
    seq,
    addMessage ? 1 : 0,
    topicId,
  );
}

function messageTopic(sql: SqlStorage, messageId: string): string | null {
  return sql.exec<{ topic_id: string }>("SELECT topic_id FROM messages WHERE id = ?", messageId).toArray()[0]?.topic_id ?? null;
}

export function replayCoreEvent(e: ArchivedEvent, sql: SqlStorage): void {
  const data = (e.data ?? {}) as Obj;
  switch (e.type) {
    case "topic.created": {
      const t = data.topic as Obj;
      sql.exec(
        `INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count,
                             waiting_on, owner_id, last_message_at, context)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)`,
        t.id as string,
        t.title as string,
        (t.status as string) ?? "open",
        t.createdBy as string,
        t.createdAt as number,
        t.rootMessageId as string,
        e.seq,
        JSON.stringify(Array.isArray(t.waitingOn) ? t.waitingOn : []),
        (t.ownerId as string | null | undefined) ?? (t.createdBy as string),
        (t.lastMessageAt as number | undefined) ?? null,
        t.contextOf ? JSON.stringify(t.contextOf) : null,
      );
      return;
    }
    case "topic.updated": {
      const topicId = data.topicId as string;
      const row = sql
        .exec<{ waiting_on: string; owner_id: string | null }>("SELECT waiting_on, owner_id FROM topics WHERE id = ?", topicId)
        .toArray()[0];
      if (!row) return;
      const sets: string[] = ["last_seq = ?"];
      const args: unknown[] = [e.seq];
      const set = (col: string, v: unknown) => {
        sets.push(`${col} = ?`);
        args.push(v);
      };
      if (typeof data.title === "string") set("title", data.title);
      if (typeof data.status === "string") set("status", data.status);
      if (Array.isArray(data.waitingOn)) {
        set("waiting_on", JSON.stringify(data.waitingOn));
        const before = JSON.parse(row.waiting_on ?? "[]") as string[];
        for (const id of data.waitingOn as string[]) if (!before.includes(id)) setFollowing(sql, id, topicId, true);
      }
      if (data.ownerId !== undefined) {
        set("owner_id", data.ownerId);
        if (data.ownerId && data.ownerId !== row.owner_id) setFollowing(sql, data.ownerId as string, topicId, true);
      }
      if (typeof data.lastMessageAt === "number") set("last_message_at", data.lastMessageAt);
      if (data.contextOf && typeof data.contextOf === "object") set("context", JSON.stringify(data.contextOf));
      // The room writes a topic.updated with only topicId when a delete leaves no message.
      if (Object.keys(data).length === 1) set("last_message_at", null);
      sql.exec(`UPDATE topics SET ${sets.join(", ")} WHERE id = ?`, ...args, topicId);
      return;
    }
    case "message.created": {
      const m = data.message as Obj;
      const mentions = Array.isArray(m.mentions) ? (m.mentions as string[]) : [];
      const ext = extOf(m, CORE_MESSAGE_KEYS);
      sql.exec(
        `INSERT INTO messages (id, client_msg_id, topic_id, author_id, body, mentions, created_seq, created_at,
                               edited_at, deleted_at, streaming, ext, kind)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        m.id as string,
        m.clientMsgId as string,
        m.topicId as string,
        m.authorId as string,
        (m.body as string) ?? "",
        JSON.stringify(mentions),
        e.seq,
        m.createdAt as number,
        (m.editedAt as number | null) ?? null,
        (m.deletedAt as number | null) ?? null,
        m.streaming === true ? 1 : 0,
        ext ? JSON.stringify(ext) : null,
        (m.kind as string) ?? "text",
      );
      for (const id of new Set(mentions)) {
        sql.exec("INSERT OR IGNORE INTO mentions (member_id, seq, message_id) VALUES (?, ?, ?)", id, e.seq, m.id as string);
      }
      // The room does not auto-follow the 群聊.
      if (!isMainTopicId(m.topicId as string)) {
        for (const id of new Set([m.authorId as string, ...mentions])) setFollowing(sql, id, m.topicId as string, true);
      }
      sql.exec(
        "UPDATE topics SET last_message_at = ? WHERE id = ? AND root_message_id != ?",
        m.createdAt as number,
        m.topicId as string,
        m.id as string,
      );
      touch(sql, m.topicId as string, e.seq, true);
      return;
    }
    case "message.updated": {
      const id = data.messageId as string;
      const row = sql.exec<{ ext: string | null; edited_at: number | null }>("SELECT ext, edited_at FROM messages WHERE id = ?", id).toArray()[0];
      if (!row) return;
      const extra = extOf(data, CORE_UPDATE_KEYS);
      const ext = extra ? { ...(row.ext ? (JSON.parse(row.ext) as Obj) : {}), ...extra } : null;
      // cards2: a server-made state update (card answer, retract, expiry; a cleared ref) has no
      // cardV2 and carries cardState or blockRefs. It is not an edit: edited_at stays.
      const stateOnly = data.cardV2 === undefined && (data.cardState !== undefined || data.blockRefs !== undefined);
      sql.exec(
        "UPDATE messages SET body = ?, edited_at = ?, streaming = ?, ext = COALESCE(?, ext) WHERE id = ?",
        (data.body as string) ?? "",
        stateOnly ? row.edited_at : ((data.editedAt as number) ?? e.at),
        data.streaming === true ? 1 : 0,
        ext ? JSON.stringify(ext) : null,
        id,
      );
      touch(sql, e.topicId ?? messageTopic(sql, id), e.seq);
      return;
    }
    case "message.deleted": {
      const id = data.messageId as string;
      // Same scrub as a live delete: the archive may hold the content from before the delete.
      scrubDeletedMessage(sql, id, (data.deletedAt as number) ?? e.at);
      touch(sql, e.topicId ?? messageTopic(sql, id), e.seq);
      return;
    }
    default:
      // group.* events: membership comes from D1. Feature events: their own importers.
      return;
  }
}

registerEventImporter(replayCoreEvent);
