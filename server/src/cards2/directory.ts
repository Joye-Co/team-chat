// GAPS PC-16: drops the cached member directory (./state) in the rooms of changed members.
import { roomStub } from "../core/env";
import { logger } from "../core/logger";
import { registerMembersChanged, registerRoomRpc } from "../core/registry";
import { forgetDirectory } from "./state";

export const DIRECTORY_CHANGED_RPC = "cards.directoryChanged";

registerRoomRpc(DIRECTORY_CHANGED_RPC, () => {
  forgetDirectory();
  return { ok: true };
});

registerMembersChanged(async (env, memberIds) => {
  const ids = [...new Set(memberIds)];
  if (ids.length === 0) return;
  const rows = await env.DB.prepare(`SELECT DISTINCT group_id FROM group_members WHERE member_id IN (${ids.map(() => "?").join(",")})`)
    .bind(...ids)
    .all<{ group_id: string }>();
  const done = await Promise.allSettled(rows.results.map((r) => roomStub(env, r.group_id).rpc(r.group_id, DIRECTORY_CHANGED_RPC, null, {})));
  const failed = done.filter((d) => d.status === "rejected").length;
  if (failed > 0) logger.warn("directory refresh failed in some rooms", { failed, rooms: done.length });
});
