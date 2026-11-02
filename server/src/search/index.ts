// Message search. See docs/protocol.md, "Search". The index lives in each GroupRoom (FTS5 with
// packages/search tokens); the member's gateway fans a search out to the member's rooms.
import {
  registerAfterCommit,
  registerEventWriter,
  registerFrameHandler,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";
import { roomStub } from "../core/env";
import { ROOM_SEARCH_RPC, handleSearch } from "./frame";
import { backfillPending, ensureBackfilled, indexEvent, migrateSearch1 } from "./indexer";
import { searchRoom } from "./query";

export const ROOM_BACKFILL_RPC = "search.backfill";

registerRoomMigration({ name: "search-1", up: migrateSearch1 });
registerEventWriter(indexEvent);
registerRoomRpc(ROOM_SEARCH_RPC, (room, _actor, args) => searchRoom(room, args));
registerRoomRpc(ROOM_BACKFILL_RPC, async (room, actor) => {
  if (actor) return { started: false }; // internal only
  await ensureBackfilled(room);
  return { started: true };
});
// After a deploy, the first write in an older room starts its backfill, so the first search
// does not wait for it.
registerAfterCommit((_event, h) => {
  if (!backfillPending(h.sql)) return;
  h.waitUntil(roomStub(h.env, h.groupId).rpc(h.groupId, ROOM_BACKFILL_RPC, null, {}));
});
registerFrameHandler<any>("search", { rate: "read", handle: handleSearch });
