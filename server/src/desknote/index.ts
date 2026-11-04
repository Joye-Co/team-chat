// Desk notes (1.1, GAPS SF-3, docs/protocol.md "Desk notes"): a bot member's one-line priority
// for the 首页 desk. The bot sets it with `PUT /v1/desk-note`; the room stores `desk.noted` in
// the group log (so every client syncs it like any event) and keeps the newest note per member
// in `desk_notes`, rebuilt from the log on restore.
import { type DeskNote, type DeskNotesResponse, MAX_DESK_NOTE_CHARS, normalizeGroupText, PutDeskNoteRequest, type PutDeskNoteResponse } from "@chat/protocol";
import type { AuthContext } from "../core/auth";
import { isGroupMember } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { registerEventWriter, registerRoomMigration, registerRoomRpc, registerRoute } from "../core/registry";
import { actorOf } from "../files/store";

const SET_RPC = "desknote.set";
const LIST_RPC = "desknote.list";

registerRoomMigration({
  name: "desknote-1",
  up: `CREATE TABLE desk_notes (member_id TEXT PRIMARY KEY, text TEXT NOT NULL, at INTEGER NOT NULL, seq INTEGER NOT NULL);`,
});

// Live writes and archive imports keep the table in step with the log.
registerEventWriter((e, sql) => {
  if (e.type !== "desk.noted") return;
  const text = (e.data as { text?: unknown })?.text;
  if (typeof text !== "string") return;
  if (text === "") sql.exec("DELETE FROM desk_notes WHERE member_id = ?", e.actorId);
  else sql.exec("INSERT OR REPLACE INTO desk_notes (member_id, text, at, seq) VALUES (?, ?, ?, ?)", e.actorId, text, e.at, e.seq);
});

registerRoomRpc(SET_RPC, (room, actor, args): PutDeskNoteResponse => {
  const { text } = args as { text: string };
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  if (actor.kind !== "bot") throw new ApiError("forbidden", "only bot members set a desk note");
  const cur = room.sql.exec<{ text: string }>("SELECT text FROM desk_notes WHERE member_id = ?", actor.id).toArray()[0]?.text ?? "";
  if (cur === text) return { seq: null };
  const event = room.commit((tx) => tx.append({ type: "desk.noted", actorId: actor.id, data: { text } }));
  return { seq: event.seq };
});

registerRoomRpc(LIST_RPC, (room, actor): DeskNotesResponse => {
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const notes = room.sql
    .exec<{ member_id: string; text: string; at: number; seq: number }>("SELECT member_id, text, at, seq FROM desk_notes ORDER BY seq DESC")
    .toArray()
    .map((r): DeskNote => ({ memberId: r.member_id, text: r.text, at: r.at, seq: r.seq }));
  return { notes };
});

registerRoute({
  method: "PUT",
  path: "/v1/desk-note",
  auth: "member",
  async handler({ env, body, auth }) {
    const a = auth as AuthContext;
    const req = await body(PutDeskNoteRequest);
    if (a.member.kind !== "bot") throw new ApiError("forbidden", "only bot members set a desk note");
    if (!(await isGroupMember(env.DB, req.groupId, a.memberId))) throw new ApiError("not_found", "group not found");
    const text = normalizeGroupText(req.text);
    if ([...text].length > MAX_DESK_NOTE_CHARS) throw new ApiError("invalid", `a desk note is at most ${MAX_DESK_NOTE_CHARS} characters`);
    return unwrap(await roomStub(env, req.groupId).rpc(req.groupId, SET_RPC, actorOf(a), { text }));
  },
});

registerRoute({
  method: "GET",
  path: "/v1/groups/:id/desk-notes",
  auth: "member",
  async handler({ env, params, auth }) {
    const a = auth as AuthContext;
    if (!(await isGroupMember(env.DB, params.id, a.memberId))) throw new ApiError("not_found", "group not found");
    return unwrap(await roomStub(env, params.id).rpc(params.id, LIST_RPC, actorOf(a), {}));
  },
});
