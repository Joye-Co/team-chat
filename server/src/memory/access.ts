// Who reads and writes a memory scope (docs/protocol.md, "Memory"): team — every member reads,
// owners write; group — its members; member (an AI member) — every member reads, owners and the
// member itself write; person — only that person.
import type { Member } from "@chat/protocol";
import { getGroup, isGroupMember } from "../core/db";
import { ApiError } from "../core/errors";

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
