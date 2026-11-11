// Who reads and writes a memory scope (docs/protocol.md, "Memory"): team — every member reads,
// owners write; group — its members; member (an AI member) — every member reads, owners and the
// member itself write; person — only that person. An AI member's memory learned in a group
// (`from_group`) is read only by that group's members; `unknown` (noted before the group was
// recorded) only by owners.
import type { Member } from "@chat/protocol";
import { getGroup, isGroupMember } from "../core/db";
import { ApiError } from "../core/errors";
import { type MemoryRow, UNKNOWN_GROUP } from "./store";

export interface Who {
  memberId: string;
  level: Member["level"];
  kind: Member["kind"];
}

export type Scope = { layer: "team" } | { layer: "group" | "member" | "person"; id: string };

export function parseScope(scope: string): Scope {
  if (scope === "team") return { layer: "team" };
  const m = /^(group|member|person):([A-Za-z0-9_-]+)$/.exec(scope);
  if (!m) throw new ApiError("invalid", "scope is team, group:<id>, member:<id> or person:<id>");
  return { layer: m[1] as "group" | "member" | "person", id: m[2] };
}

/** Throws `not_found` when the caller may not read the scope (or it does not exist), `forbidden` when it may read but not write. */
export async function requireAccess(db: D1Database, who: Who, scope: string, write: boolean): Promise<Scope> {
  const s = parseScope(scope);
  const deny = (readable: boolean) => {
    throw readable ? new ApiError("forbidden", "you cannot change this memory") : new ApiError("not_found", "memory scope not found");
  };
  switch (s.layer) {
    case "team":
      if (write && who.level !== "owner") deny(true);
      return s;
    case "group": {
      const group = await getGroup(db, s.id);
      if (!group || !(await isGroupMember(db, s.id, who.memberId))) deny(false);
      return s;
    }
    case "member": {
      const m = await db.prepare("SELECT kind FROM members WHERE id = ?").bind(s.id).first<{ kind: string }>();
      if (!m || m.kind !== "bot") deny(false);
      if (write && who.level !== "owner" && who.memberId !== s.id) deny(true);
      return s;
    }
    case "person":
      if (who.memberId !== s.id) deny(false);
      return s;
  }
}

/** Whether the caller may read this memory beyond its scope: the group it was learned in. */
export async function canReadSource(db: D1Database, who: Who, row: Pick<MemoryRow, "from_group">): Promise<boolean> {
  if (!row.from_group) return true;
  if (row.from_group === UNKNOWN_GROUP) return who.level === "owner";
  return isGroupMember(db, row.from_group, who.memberId);
}

/**
 * SQL condition on a memories row (alias `alias`): the caller reads it (same rule as
 * `canReadSource`). Binds the caller's id at `?${memberParam}` and 1 for an owner at `?${ownerParam}`.
 */
export function readableSql(alias: string, memberParam: number, ownerParam: number): string {
  return `(${alias}.from_group IS NULL OR (${alias}.from_group = '${UNKNOWN_GROUP}' AND ?${ownerParam} = 1) OR ${alias}.from_group IN (SELECT group_id FROM group_members WHERE member_id = ?${memberParam}))`;
}

/**
 * SQL condition on a memories row (alias `alias`): it may go into a prompt in the group bound at
 * `?${groupParam}`: no source group, or everyone in that group is in the source group too.
 */
export function fitsGroupSql(alias: string, groupParam: number): string {
  return `(${alias}.from_group IS NULL OR (${alias}.from_group != '${UNKNOWN_GROUP}' AND NOT EXISTS (SELECT 1 FROM group_members gx WHERE gx.group_id = ?${groupParam} AND gx.member_id NOT IN (SELECT gy.member_id FROM group_members gy WHERE gy.group_id = ${alias}.from_group))))`;
}
