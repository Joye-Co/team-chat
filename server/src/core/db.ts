// D1 access for the core directory: members, groups and membership.
import { CharacterId, type Group, type Member } from "@chat/protocol";

export interface MemberRow {
  id: string;
  kind: "human" | "bot";
  name: string;
  role: string | null;
  level: "owner" | "member";
  avatar_url: string | null;
  deactivated: number;
  created_at: number;
  /** Character id (migration 0004); absent before it ran. */
  character?: string | null;
  /** 1.1 (migration 0008): 汇报给 and the weekly spending limit; absent before it ran. */
  reports_to?: string | null;
  weekly_budget_cents?: number | null;
  /** Usable (migration 0017_roles): 1 for a 所有者; absent before it ran. */
  team_owner?: number;
  /** Hide and delete (migration 0020): set when the AI member was deleted; absent before it ran. */
  deleted_at?: number | null;
}

export interface GroupRow {
  id: string;
  kind: "group" | "dm";
  name: string;
  dm_key: string | null;
  is_default: number;
  created_by: string;
  created_at: number;
  /** 1.1 */
  purpose?: string | null;
  update_seq?: number;
  /** Hide and delete (migration 0020): set when the 群 was deleted. getGroup leaves it out. */
  deleted_at?: number | null;
}

export function toMember(row: MemberRow): Member {
  const m: Member = {
    id: row.id,
    kind: row.kind,
    name: row.name,
    role: row.role,
    level: row.level,
    avatarUrl: row.avatar_url,
  };
  if (row.deactivated) m.deactivated = true;
  if (row.deleted_at) {
    m.deleted = true;
    m.deactivated = true;
  }
  if (isTeamOwner(row)) m.teamOwner = true;
  // Only known ids reach the wire: clients reject unknown ones.
  const character = CharacterId.safeParse(row.character);
  if (character.success) m.character = character.data;
  if (row.reports_to) m.reportsTo = row.reports_to;
  if (typeof row.weekly_budget_cents === "number" && row.weekly_budget_cents >= 0) m.weeklyBudgetCents = row.weekly_budget_cents;
  return m;
}

/** A 所有者: a human admin marked team_owner (design-roles 3.1). */
export function isTeamOwner(row: Pick<MemberRow, "kind" | "level" | "team_owner">): boolean {
  return row.team_owner === 1 && row.level === "owner" && row.kind === "human";
}

export async function getMember(db: D1Database, id: string): Promise<MemberRow | null> {
  return db.prepare("SELECT * FROM members WHERE id = ?").bind(id).first<MemberRow>();
}

export async function listMembers(db: D1Database): Promise<MemberRow[]> {
  const r = await db.prepare("SELECT * FROM members ORDER BY created_at, id").all<MemberRow>();
  return r.results;
}

export async function getGroup(db: D1Database, id: string): Promise<GroupRow | null> {
  // A deleted group answers not_found everywhere (docs/protocol.md, "Hide and delete").
  return db.prepare("SELECT * FROM groups WHERE id = ? AND deleted_at IS NULL").bind(id).first<GroupRow>();
}

export async function groupMemberIds(db: D1Database, groupId: string): Promise<string[]> {
  const r = await db
    .prepare("SELECT member_id FROM group_members WHERE group_id = ? ORDER BY added_at, member_id")
    .bind(groupId)
    .all<{ member_id: string }>();
  return r.results.map((x) => x.member_id);
}

export async function isGroupMember(db: D1Database, groupId: string, memberId: string): Promise<boolean> {
  const r = await db
    .prepare("SELECT 1 AS x FROM group_members WHERE group_id = ? AND member_id = ?")
    .bind(groupId, memberId)
    .first();
  return r !== null;
}

/** Groups of one member with their member ids, in one query. */
export async function groupsOfMember(
  db: D1Database,
  memberId: string,
): Promise<{ row: GroupRow; memberIds: string[] }[]> {
  const r = await db
    .prepare(
      `SELECT g.*, (SELECT json_group_array(member_id) FROM
         (SELECT member_id FROM group_members WHERE group_id = g.id ORDER BY added_at, member_id)) AS member_ids
       FROM groups g JOIN group_members gm ON gm.group_id = g.id
       WHERE gm.member_id = ? AND g.deleted_at IS NULL ORDER BY g.created_at, g.id`,
    )
    .bind(memberId)
    .all<GroupRow & { member_ids: string }>();
  return r.results.map(({ member_ids, ...row }) => ({ row, memberIds: JSON.parse(member_ids) as string[] }));
}

/**
 * The Group as one viewer sees it. A DM is named after the other member.
 */
export function viewGroup(
  row: GroupRow,
  memberIds: string[],
  viewerId: string,
  names: Map<string, string>,
): Group {
  let name = row.name;
  if (row.kind === "dm") {
    const other = memberIds.find((id) => id !== viewerId) ?? viewerId;
    name = names.get(other) ?? name;
  }
  return {
    id: row.id,
    kind: row.kind,
    name,
    memberIds,
    createdBy: row.created_by,
    createdAt: row.created_at,
    ...(row.purpose ? { purpose: row.purpose } : {}),
  };
}
