// 引用提问 (REFS §A6): `send.blockRefs` refs to one part of a message block or a 产出 page.
// The server checks the sources, fills `sourceAuthorId` / `sourceTopicId`, writes one quote
// line `> ↳ {label}` per ref in front of the body (what 1.0.x readers see), keeps that prefix
// on edits, and clears the label of refs whose source is deleted.
import { type BlockRef, type BlockRefInput, MAX_BODY_CHARS, blockRefPrefix } from "@chat/protocol";
import { ApiError } from "../core/errors";
import type { RoomContext, RoomTx } from "../core/registry";
import { extOf, messageRow } from "./store";

/** The catalog text of a ref whose source was deleted. */
export const DELETED_REF_TEXT = "原消息已删除";

/** The quote lines of these refs (a deleted ref shows `原消息已删除`). */
export function prefixOf(refs: BlockRef[]): string {
  return blockRefPrefix(refs.map((r) => (r.deleted ? DELETED_REF_TEXT : r.label)));
}

/** The body with the refs' quote lines in front (and one empty line before a non-empty body). */
export function withPrefix(refs: BlockRef[], body: string): string {
  if (refs.length === 0) return body;
  const prefix = prefixOf(refs);
  return body === "" ? prefix : `${prefix}\n\n${body}`;
}

/** The body without an exact leading prefix of these refs, else unchanged. */
export function withoutPrefix(refs: BlockRef[], body: string): string {
  if (refs.length === 0) return body;
  const prefix = prefixOf(refs);
  if (body === prefix) return "";
  if (body.startsWith(`${prefix}\n\n`)) return body.slice(prefix.length + 2);
  return body;
}

/**
 * The body without the quote lines of `refs`. A body stored before the newest clear can hold an
 * older label: then the first `refs.length` lines go when each is a quote line.
 */
function stripPrefix(refs: BlockRef[], body: string): string {
  const out = withoutPrefix(refs, body);
  if (out !== body || refs.length === 0) return out;
  const lines = body.split("\n");
  if (lines.length < refs.length || !lines.slice(0, refs.length).every((l) => l.startsWith("> ↳ "))) return body;
  const rest = lines.slice(refs.length);
  if (rest.length === 0) return "";
  return rest[0] === "" ? rest.slice(1).join("\n") : body;
}

/** Checks the refs of a send and fills what the server owns. */
export function resolveRefs(room: RoomContext, inputs: BlockRefInput[]): BlockRef[] {
  return inputs.map((r) => {
    const out: BlockRef = { ...r, sourceTopicId: "" };
    if (r.messageId !== undefined) {
      const src = messageRow(room.sql, r.messageId);
      if (!src) throw new ApiError("invalid", "the referenced message is not in this group");
      if (src.deleted_at !== null) throw new ApiError("not_found", "the referenced message was deleted");
      const card = extOf(src).cardV2;
      if (r.cardRev !== undefined && (!card || r.cardRev > card.rev)) throw new ApiError("invalid", "cardRev is not a rev of this card");
      out.sourceAuthorId = src.author_id;
      out.sourceTopicId = src.topic_id;
    } else if (r.artifact) {
      if (r.cardRev !== undefined) throw new ApiError("invalid", "cardRev is only for a card message");
      const v = room.sql
        .exec<{ topic_id: string; created_by: string }>(
          `SELECT a.topic_id, v.created_by FROM artifacts a JOIN artifact_versions v ON v.artifact_id = a.id
           WHERE a.id = ? AND v.version = ?`,
          r.artifact.artifactId,
          r.artifact.version,
        )
        .toArray()[0];
      if (!v) throw new ApiError("not_found", "产出 version not found");
      out.sourceAuthorId = v.created_by;
      out.sourceTopicId = v.topic_id;
    }
    return out;
  });
}

export function checkBodyLength(body: string): void {
  if ([...body].length > MAX_BODY_CHARS) throw new ApiError("invalid", `body exceeds ${MAX_BODY_CHARS} characters with the quote lines`);
}

export function indexRefs(sql: SqlStorage, messageId: string, refs: BlockRef[]): void {
  for (const r of refs) {
    sql.exec(
      "INSERT INTO block_refs (message_id, source_message_id, artifact_id) VALUES (?, ?, ?)",
      messageId,
      r.messageId ?? null,
      r.artifact?.artifactId ?? null,
    );
  }
}

/**
 * The source `sourceId` (a message, or a 产出 with `artifact`) was deleted: every message that
 * refers to it gets the deleted ref (label "", no value, `deleted: true`), its prefix line
 * rewritten in the row and in its stored `message.created`, and a `message.updated` with the
 * new body and refs.
 */
export function clearRefsTo(room: RoomContext, tx: RoomTx, source: { messageId?: string; artifactId?: string }, actorId: string): void {
  const rows = room.sql
    .exec<{ message_id: string }>(
      source.messageId !== undefined
        ? "SELECT DISTINCT message_id FROM block_refs WHERE source_message_id = ?"
        : "SELECT DISTINCT message_id FROM block_refs WHERE artifact_id = ?",
      source.messageId ?? source.artifactId ?? "",
    )
    .toArray();
  for (const { message_id } of rows) {
    const row = messageRow(room.sql, message_id);
    if (!row || row.deleted_at !== null) continue;
    const ext = extOf(row);
    const refs = (ext.blockRefs as BlockRef[] | undefined) ?? [];
    const hit = (r: BlockRef) =>
      source.messageId !== undefined ? r.messageId === source.messageId : r.artifact?.artifactId === source.artifactId;
    if (!refs.some((r) => hit(r) && !r.deleted)) continue;
    const next = refs.map((r): BlockRef => {
      if (!hit(r)) return r;
      const { value: _v, ...rest } = r;
      return { ...rest, label: "", deleted: true };
    });
    const body = withPrefix(next, withoutPrefix(refs, row.body));
    const newExt = { ...ext, blockRefs: next };
    room.sql.exec("UPDATE messages SET body = ?, ext = ? WHERE id = ?", body, JSON.stringify(newExt), message_id);
    room.sql.exec(
      `UPDATE events SET data = json_set(json_set(data, '$.message.body', ?), '$.message.blockRefs', json(?))
       WHERE message_id = ? AND type = 'message.created'`,
      body,
      JSON.stringify(next),
      message_id,
    );
    // Earlier edits and clears stored the old quote lines and ref values in `message.updated`.
    const updates = room.sql
      .exec<{ seq: number; data: string }>("SELECT seq, data FROM events WHERE message_id = ? AND type = 'message.updated'", message_id)
      .toArray();
    for (const u of updates) {
      const data = JSON.parse(u.data) as Record<string, unknown>;
      let dirty = false;
      if (typeof data.body === "string") {
        const was = data.body;
        data.body = withPrefix(next, stripPrefix(refs, was));
        dirty = data.body !== was;
      }
      if (Array.isArray(data.blockRefs)) {
        data.blockRefs = next;
        dirty = true;
      }
      if (dirty) room.sql.exec("UPDATE events SET data = ? WHERE seq = ?", JSON.stringify(data), u.seq);
    }
    const seq = tx.nextSeq();
    tx.append({
      type: "message.updated",
      actorId,
      topicId: row.topic_id,
      messageId: message_id,
      data: { messageId: message_id, body, editedAt: row.edited_at ?? row.created_at, streaming: false, blockRefs: next },
    });
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, row.topic_id);
  }
}
