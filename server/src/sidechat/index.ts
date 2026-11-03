// Side chats (单独问, docs/protocol.md "Side chats"): a send with `context` in my DM with a bot.
// The group room stores the context as the topic's `contextOf`; this validator makes the
// cross-group checks first: the DM is with a bot, the sender and the bot are both members of
// the source group, and the source topic exists. It runs for WebSocket and HTTP sends.
import type { ClientFrameOf } from "@chat/protocol";
import { getGroup, getMember, groupMemberIds, isGroupMember } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { registerFrameValidator, registerRoomRpc } from "../core/registry";

export const SIDE_CHAT_SOURCE_RPC = "sidechat.source";

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
