import type { GroupRoom } from "./room";
import type { MemberGateway } from "./gateway";

export interface Env {
  DB: D1Database;
  FILES: R2Bucket;
  MEMBER_GATEWAY: DurableObjectNamespace<MemberGateway>;
  GROUP_ROOM: DurableObjectNamespace<GroupRoom>;
  ENVIRONMENT: string;
  VERSION: string;
  ADMIN_SECRET?: string;
}

/** Every Durable Object lives near the team (Oceania hint, see AGENTS.md). */
export const LOCATION_HINT = "oc" as const;

export function gatewayStub(env: Env, memberId: string): DurableObjectStub<MemberGateway> {
  return env.MEMBER_GATEWAY.get(env.MEMBER_GATEWAY.idFromName(memberId), {
    locationHint: LOCATION_HINT,
  });
}

export function roomStub(env: Env, groupId: string): DurableObjectStub<GroupRoom> {
  return env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(groupId), { locationHint: LOCATION_HINT });
}
