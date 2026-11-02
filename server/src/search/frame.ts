// Gateway side of search: the `search` frame. With groupId it asks that room; without it, every
// room of the member in parallel, and merges the hits newest first.
import type { ClientFrameOf, SearchHit } from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError } from "../core/errors";
import type { FrameContext } from "../core/registry";
import { DEFAULT_LIMIT, type RoomSearchArgs } from "./query";

export const ROOM_SEARCH_RPC = "search.query";

type SearchFrame = ClientFrameOf<"search">;

function newestFirst(a: SearchHit, b: SearchHit): number {
  return b.createdAt - a.createdAt || b.seq - a.seq || (a.messageId < b.messageId ? 1 : -1);
}

async function groupIdsOf(db: D1Database, memberId: string): Promise<string[]> {
  const r = await db
    .prepare("SELECT group_id FROM group_members WHERE member_id = ?")
    .bind(memberId)
    .all<{ group_id: string }>();
  return r.results.map((x) => x.group_id);
}

export async function handleSearch(frame: SearchFrame, c: FrameContext): Promise<{ reply: unknown }> {
  if (frame.topicId && !frame.groupId) throw new ApiError("invalid", "topicId needs groupId");
  const limit = frame.limit ?? DEFAULT_LIMIT;
  const args: RoomSearchArgs = { query: frame.query, limit };
  if (frame.topicId) args.topicId = frame.topicId;

  let hits: SearchHit[];
  if (frame.groupId) {
    await c.requireMember(frame.groupId);
    const r = await roomStub(c.env, frame.groupId).rpc(frame.groupId, ROOM_SEARCH_RPC, c.actor, args);
    if (!r.ok) throw new ApiError(r.error.code, r.error.message);
    hits = r.value as SearchHit[];
  } else {
    const groupIds = await groupIdsOf(c.env.DB, c.actor.id);
    const results = await Promise.all(
      groupIds.map(async (gid) => {
        try {
          return { gid, r: await roomStub(c.env, gid).rpc(gid, ROOM_SEARCH_RPC, c.actor, args) };
        } catch (err) {
          c.log.error("search room call failed", { err, groupId: gid });
          return { gid, r: null };
        }
      }),
    );
    hits = [];
    let failed = 0;
    for (const { gid, r } of results) {
      if (r?.ok) hits.push(...(r.value as SearchHit[]));
      else if (r && (r.error.code === "forbidden" || r.error.code === "not_found")) continue; // membership race
      else {
        failed++;
        if (r) c.log.warn("search room refused", { groupId: gid, code: r.error.code });
      }
    }
    if (failed > 0 && failed === groupIds.length) throw new ApiError("internal", "search failed");
  }
  hits.sort(newestFirst);
  return { reply: { t: "searchResults", rid: frame.rid, hits: hits.slice(0, limit) } };
}
