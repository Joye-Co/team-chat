// Emoji reactions. See docs/protocol.md, "Reactions". A `react` frame goes to the group room,
// which appends `reaction.changed` when my reaction changes; an event writer keeps the current
// list in the message's `ext.reactions`, for live writes and archive imports alike, so history,
// send results and exports carry it without a second table.
import { MAX_REACTIONS, type ClientFrameOf, type Reaction } from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { type ArchivedEvent, registerEventWriter, registerFrameHandler, registerRoomRpc } from "../core/registry";

type ReactFrame = ClientFrameOf<"react">;
type ReactArgs = { messageId: string; emoji: string; on: boolean };
type Row = { topic_id: string; deleted_at: number | null; ext: string | null };

export const ROOM_REACT_RPC = "reactions.set";

/** Adds or removes `memberId` under `emoji`. Returns the new list, or null when nothing changes. */
export function applyReaction(list: Reaction[], emoji: string, memberId: string, on: boolean): Reaction[] | null {
  const ix = list.findIndex((r) => r.emoji === emoji);
  if (on) {
    if (ix >= 0) {
      if (list[ix].memberIds.includes(memberId)) return null;
      return list.map((r, i) => (i === ix ? { emoji, memberIds: [...r.memberIds, memberId] } : r));
    }
    return [...list, { emoji, memberIds: [memberId] }];
  }
  if (ix < 0 || !list[ix].memberIds.includes(memberId)) return null;
  const left = list[ix].memberIds.filter((m) => m !== memberId);
  return left.length === 0 ? list.filter((_, i) => i !== ix) : list.map((r, i) => (i === ix ? { emoji, memberIds: left } : r));
}

function parseExt(ext: string | null): Record<string, unknown> {
  return ext ? (JSON.parse(ext) as Record<string, unknown>) : {};
}

function reactionsOf(ext: Record<string, unknown>): Reaction[] {
  return Array.isArray(ext.reactions) ? (ext.reactions as Reaction[]) : [];
}

function messageRow(sql: SqlStorage, id: string): Row | undefined {
  return sql.exec<Row>("SELECT topic_id, deleted_at, ext FROM messages WHERE id = ?", id).toArray()[0] as Row | undefined;
}

/** Keeps `messages.ext.reactions` in step with the log (live writes and imports). */
function writeReaction(event: ArchivedEvent, sql: SqlStorage): void {
  if (event.type !== "reaction.changed") return;
  const d = event.data as ReactArgs;
  const row = messageRow(sql, d.messageId);
  if (!row) return;
  const ext = parseExt(row.ext);
  const next = applyReaction(reactionsOf(ext), d.emoji, event.actorId, d.on);
  if (!next) return;
  if (next.length > 0) ext.reactions = next;
  else delete ext.reactions;
  sql.exec(
    "UPDATE messages SET ext = ? WHERE id = ?",
    Object.keys(ext).length > 0 ? JSON.stringify(ext) : null,
    d.messageId,
  );
}

registerEventWriter(writeReaction);

registerRoomRpc(ROOM_REACT_RPC, (room, actor, args) => {
  if (!actor) throw new ApiError("internal", "actor required");
  const { messageId, emoji, on } = args as ReactArgs;
  return room.commit((tx) => {
    const row = messageRow(room.sql, messageId);
    if (!row || row.deleted_at !== null) throw new ApiError("not_found", "message not found");
    const list = reactionsOf(parseExt(row.ext));
    const next = applyReaction(list, emoji, actor.id, on);
    if (!next) return { changed: false };
    if (next.length > MAX_REACTIONS) throw new ApiError("invalid", `at most ${MAX_REACTIONS} emoji per message`);
    const seq = tx.nextSeq();
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, row.topic_id);
    tx.append({ type: "reaction.changed", actorId: actor.id, topicId: row.topic_id, messageId, data: { messageId, emoji, on } });
    return { changed: true };
  });
});

registerFrameHandler<ReactFrame>("react", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const args: ReactArgs = { messageId: frame.messageId, emoji: frame.emoji, on: frame.on };
    return unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, ROOM_REACT_RPC, c.actor, args));
  },
});
