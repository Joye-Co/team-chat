// Side chats (单独问, docs/protocol.md "Side chats"): a send with `context` in my DM with a bot.
// The group room stores the context as the topic's `contextOf`; this validator makes the
// cross-group checks first: the DM is with a bot, the sender and the bot are both members of
// the source group, and the source topic exists. It runs for WebSocket and HTTP sends.
import type { AsideOrigin, ClientFrameOf, TopicContext } from "@chat/protocol";
import { getGroup, getMember, groupMemberIds, isGroupMember } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { registerFrameValidator, registerMessageExtension, registerRoomRpc } from "../core/registry";

export const SIDE_CHAT_SOURCE_RPC = "sidechat.source";
export const SIDE_CHAT_ANSWER_RPC = "sidechat.answer";

registerFrameValidator("send", async (frame, c) => {
  const f = frame as ClientFrameOf<"send">;
  if (!f.context) return;
  const dm = await getGroup(c.env.DB, f.groupId);
  if (!dm || dm.kind !== "dm") throw new ApiError("invalid", "context is only for a side chat in a DM");
  const botId = (await groupMemberIds(c.env.DB, f.groupId)).find((id) => id !== c.actor.id);
  const bot = botId ? await getMember(c.env.DB, botId) : null;
  if (!bot || bot.kind !== "bot") throw new ApiError("invalid", "side chats are with bots");
  await c.requireMember(f.context.groupId);
  if (!(await isGroupMember(c.env.DB, f.context.groupId, bot.id))) {
    throw new ApiError("forbidden", "the bot is not a member of that group");
  }
  unwrap(await roomStub(c.env, f.context.groupId).rpc(f.context.groupId, SIDE_CHAT_SOURCE_RPC, c.actor, { topicId: f.context.topicId }));
});

/** The source topic of a side chat exists and the caller is a member. */
registerRoomRpc(SIDE_CHAT_SOURCE_RPC, (room, actor, args) => {
  const { topicId } = args as { topicId: string };
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const t = room.sql.exec<{ last_seq: number }>("SELECT last_seq FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!t) throw new ApiError("not_found", "topic not found");
  return { lastSeq: t.last_seq };
});

// ---------------------------------------------------------------------------
// Post to topic (1.1, GAPS SF-4): `send.fromAside` names the side chat answer a message posts
// to the side chat's source topic. Stored on the message, and posted once.
// ---------------------------------------------------------------------------

registerFrameValidator("send", async (frame, c) => {
  const f = frame as ClientFrameOf<"send"> & { fromAside?: AsideOrigin };
  if (!f.fromAside) return;
  if (!f.topicId) throw new ApiError("invalid", "fromAside needs the source topic", "aside.bad_origin");
  await c.requireMember(f.fromAside.groupId);
  unwrap(
    await roomStub(c.env, f.fromAside.groupId).rpc(f.fromAside.groupId, SIDE_CHAT_ANSWER_RPC, c.actor, {
      ...f.fromAside,
      sourceGroupId: f.groupId,
      sourceTopicId: f.topicId,
    }),
  );
});

/** In the asker's DM: the side chat is about `sourceTopicId` and the answer is in it. */
registerRoomRpc(SIDE_CHAT_ANSWER_RPC, (room, actor, args) => {
  const a = args as AsideOrigin & { sourceGroupId: string; sourceTopicId: string };
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const t = room.sql.exec<{ context: string | null }>("SELECT context FROM topics WHERE id = ?", a.topicId).toArray()[0];
  const ctx = t?.context ? (JSON.parse(t.context) as TopicContext) : null;
  if (!ctx || ctx.groupId !== a.sourceGroupId || ctx.topicId !== a.sourceTopicId) {
    throw new ApiError("invalid", "fromAside must name a side chat about this topic", "aside.bad_origin");
  }
  const m = room.sql.exec("SELECT 1 FROM messages WHERE id = ? AND topic_id = ? AND deleted_at IS NULL", a.messageId, a.topicId).toArray()[0];
  if (!m) throw new ApiError("not_found", "side chat answer not found");
  return { ok: true };
});

/** Stores `fromAside` on the message; a second post of the same answer is `conflict`. */
registerMessageExtension((frame, room) => {
  const o = (frame as { fromAside?: AsideOrigin }).fromAside;
  if (!o) return undefined;
  const topicId = (frame as { topicId?: string }).topicId;
  const dup = room.sql
    .exec<{ id: string }>(
      "SELECT id FROM messages WHERE topic_id = ? AND deleted_at IS NULL AND json_extract(ext, '$.fromAside.messageId') = ?",
      topicId ?? "",
      o.messageId,
    )
    .toArray()[0];
  if (dup) throw new ApiError("conflict", `this answer is already posted as ${dup.id}`, "aside.already_posted");
  return { fromAside: { groupId: o.groupId, topicId: o.topicId, messageId: o.messageId } };
});
