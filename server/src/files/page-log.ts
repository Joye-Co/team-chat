// The page log of a 产出 (GAPS SF-2): the page events its versions got, newest first, for every
// member of the group (页面里的回应 on the 产出 page). Events stay out of the group log; the room
// keeps them in `page_events` (files/bridge.ts) for KEEP_PAGE_LOG_MS.
import { type ArtifactEventsResponse, type ArtifactPageEvent, MAX_ARTIFACT_EVENTS_PAGE } from "@chat/protocol";
import { z } from "zod";
import type { AuthContext } from "../core/auth";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { registerRoomRpc, registerRoute } from "../core/registry";
import { artifactGroup } from "./bridge";
import { artifactRow } from "./room";
import { actorOf } from "./store";

export const ARTIFACT_EVENTS_RPC = "artifacts.events";

type Row = { event_id: string; artifact_id: string; version: number; name: string | null; kind: string; payload: string | null; actor_id: string; at: number; replied_at: number | null };

registerRoomRpc(ARTIFACT_EVENTS_RPC, (room, actor, args): ArtifactEventsResponse => {
  const { artifactId, before, limit } = args as { artifactId: string; before?: string; limit: number };
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  if (!artifactRow(room, artifactId)) throw new ApiError("not_found", "产出 not found");
  const rows = room.sql
    .exec<Row>(
      `SELECT event_id, artifact_id, version, name, kind, payload, actor_id, at, replied_at FROM page_events
       WHERE artifact_id = ? AND event_id < ? AND name IS NOT NULL ORDER BY event_id DESC LIMIT ?`,
      artifactId,
      before ?? "~",
      limit + 1,
    )
    .toArray() as Row[];
  const events = rows.slice(0, limit).map((r): ArtifactPageEvent => {
    const e: ArtifactPageEvent = {
      eventId: r.event_id,
      artifactId: r.artifact_id,
      version: r.version,
      name: r.name as string,
      kind: r.kind === "ask" ? "ask" : "emit",
      actorId: r.actor_id,
      at: r.at,
    };
    if (r.payload !== null) e.payload = JSON.parse(r.payload);
    if (r.replied_at !== null) e.replied = true;
    return e;
  });
  return { events, hasMore: rows.length > limit };
});

const Query = z.object({
  /** An event id: list older events. */
  before: z.string().min(1).max(64).optional(),
  limit: z.coerce.number().int().min(1).max(MAX_ARTIFACT_EVENTS_PAGE).default(100),
});

registerRoute({
  method: "GET",
  path: "/v1/artifacts/:id/events",
  auth: "member",
  async handler({ env, url, params, auth }) {
    const a = auth as AuthContext;
    const q = Query.safeParse(Object.fromEntries(url.searchParams));
    if (!q.success) throw new ApiError("invalid", "bad before or limit");
    const groupId = await artifactGroup(env, params.id, a);
    return unwrap(await roomStub(env, groupId).rpc(groupId, ARTIFACT_EVENTS_RPC, actorOf(a), { artifactId: params.id, ...q.data }));
  },
});
