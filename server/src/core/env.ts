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
  /** Encrypts integration hook secrets and the AI 服务 key at rest (Worker secret). */
  HOOK_SECRET_KEY?: string;
  /** Tests only: bodies containing this marker get a fake AI title. */
  AUTOTITLE_FAKE?: string;
}

/** Every Durable Object lives near the team (Oceania hint, see AGENTS.md). */
export const LOCATION_HINT = "oc" as const;

/**
 * Public RPC surface of a Durable Object class. Stubs are typed with the class's own method
 * signatures (all arguments and results are plain JSON-like data), which keeps type checking
 * fast; the generated `DurableObjectStub<T>` types get too deep for the protocol schemas.
 */
export type Rpc<T> = { [K in keyof T as T[K] extends (...args: any[]) => Promise<unknown> ? K : never]: T[K] };

/** The member's gateway: `idFromName(memberId)`, location hint `oc`. */
export function gatewayStub(env: Env, memberId: string): Rpc<MemberGateway> {
  return env.MEMBER_GATEWAY.get(env.MEMBER_GATEWAY.idFromName(memberId), {
    locationHint: LOCATION_HINT,
  }) as unknown as Rpc<MemberGateway>;
}

/** The group's room: `idFromName(groupId)`, location hint `oc`. */
export function roomStub(env: Env, groupId: string): Rpc<GroupRoom> {
  return env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(groupId), {
    locationHint: LOCATION_HINT,
  }) as unknown as Rpc<GroupRoom>;
}
