// Topic links (1.1, docs/protocol.md "Topic links"): the room stores `topic.linked` with the
// newest state of a pull request a topic names. The GitHub adapter (deliver.ts) calls it as the
// hook's bot whenever the link's state, checks or size change.
import { TopicLink } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoomRpc } from "../core/registry";

export const TOPIC_LINK_RPC = "links.set";

registerRoomRpc(TOPIC_LINK_RPC, (room, actor, args) => {
  if (!actor || actor.kind !== "bot" || !room.isMember(actor.id)) throw new ApiError("forbidden", "integration bots only");
  const { topicId, link: raw } = args as { topicId: string; link: unknown };
  const parsed = TopicLink.safeParse(raw);
  if (!parsed.success) throw new ApiError("invalid", "bad topic link");
  const topic = room.sql.exec<{ id: string }>("SELECT id FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const event = room.commit((tx) => {
    const seq = tx.nextSeq();
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, topicId);
    return tx.append({ type: "topic.linked", actorId: actor.id, topicId, data: { topicId, link: parsed.data } });
  });
  return { seq: event.seq };
});
