// Which groups a run may read (docs/plans/usable/design-visibility.zh.md 3.1): an AI member
// working for a person may read every group of kind "group" that person is in, besides the run's
// own group. DMs and side chats stay with the people in them. Read from D1 on every request.
import { ApiError } from "../core/errors";
import { logger as baseLogger } from "../core/logger";
import { ANONYMOUS_ID } from "../cards2/privacy";

/** Most groups a run reads besides its own (the person's newest by `group_members.added_at`). */
export const MAX_READ_GROUPS = 200;

const log = baseLogger.child({ obj: "reach" });

/**
 * A run's person for reads: `requested_for` when it is an active human and the run is not
 * anonymous (`requested_by = ANONYMOUS_ID`, a private card's answer); else null.
 */
export async function readerOf(db: D1Database, row: { requested_by: string; requested_for: string | null }): Promise<string | null> {
  if (row.requested_by === ANONYMOUS_ID || !row.requested_for) return null;
  const m = await db
    .prepare("SELECT kind, deactivated FROM members WHERE id = ?")
    .bind(row.requested_for)
    .first<{ kind: string; deactivated: number }>();
  return m && m.kind === "human" && !m.deactivated ? row.requested_for : null;
}

/** Own group always; with a person, every group of kind "group" the person is in. */
export async function readableGroups(db: D1Database, run: { group_id: string }, forId: string | null): Promise<Set<string>> {
  const out = new Set([run.group_id]);
  if (!forId) return out;
  const rows = (
    await db
      .prepare(
        `SELECT g.id FROM groups g JOIN group_members gm ON gm.group_id = g.id
         WHERE gm.member_id = ? AND g.kind = 'group' AND g.id != ?
         ORDER BY gm.added_at DESC, g.id DESC LIMIT ?`,
      )
      .bind(forId, run.group_id, MAX_READ_GROUPS + 1)
      .all<{ id: string }>()
  ).results;
  if (rows.length > MAX_READ_GROUPS) log.warn("readable groups cut", { count: MAX_READ_GROUPS, more: true });
  for (const r of rows.slice(0, MAX_READ_GROUPS)) out.add(r.id);
  return out;
}

/** Throws `not_found` unless the run may read `groupId` (an unknown group looks the same). */
export async function requireReadable(db: D1Database, run: { group_id: string }, forId: string | null, groupId: string): Promise<void> {
  if (groupId === run.group_id) return;
  if (!(await readableGroups(db, run, forId)).has(groupId)) throw new ApiError("not_found", "not found");
}
