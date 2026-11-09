// 活动记录 of 团队管理 (docs/protocol.md "Activity"): who changed what in the team — members and
// levels, invites, API keys, approvals, budgets, sign-ins and machines. Migration 0013 copies those
// audit rows into `activity` with a trigger (in the transaction of each write); this module serves
// them to owners and prunes rows older than 90 days on the daily cron.
import { ACTIVITY_KINDS, ActivityQuery } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoute, registerScheduled } from "../core/registry";

/** How long activity is kept. */
export const ACTIVITY_DAYS = 90;
const DAY_MS = 86_400_000;

interface ActivityRow {
  id: number;
  at: number;
  kind: string;
  action: string;
  actor_id: string | null;
  target: string | null;
  machine_id: string | null;
  details: string;
}

function toEntry(r: ActivityRow) {
  let details: Record<string, unknown> = {};
  try {
    const v = JSON.parse(r.details) as unknown;
    if (v && typeof v === "object" && !Array.isArray(v)) details = v as Record<string, unknown>;
  } catch {
    // A row whose details do not parse keeps none.
  }
  return {
    id: r.id,
    at: r.at,
    kind: r.kind,
    action: r.action,
    ...(r.actor_id ? { actorId: r.actor_id } : {}),
    ...(r.target ? { target: r.target } : {}),
    ...(r.machine_id ? { machineId: r.machine_id } : {}),
    ...(Object.keys(details).length > 0 ? { details } : {}),
  };
}

registerRoute({
  method: "GET",
  path: "/v1/activity",
  auth: "owner",
  async handler({ env, url }) {
    const q = ActivityQuery.safeParse(Object.fromEntries(url.searchParams));
    if (!q.success) throw new ApiError("invalid", "before is an entry id; limit 1..200; kind one of " + ACTIVITY_KINDS.join(", "));
    const { before, limit, actor, kind, machine } = q.data;
    const where = ["id < ?", "at >= ?"];
    const binds: (string | number)[] = [before ?? Number.MAX_SAFE_INTEGER, Date.now() - ACTIVITY_DAYS * DAY_MS];
    if (actor) {
      where.push("actor_id = ?");
      binds.push(actor);
    }
    if (kind) {
      where.push(`kind IN (${kind.map(() => "?").join(", ")})`);
      binds.push(...kind);
    }
    if (machine) {
      where.push("machine_id = ?");
      binds.push(machine);
    }
    const rows = await env.DB.prepare(`SELECT * FROM activity WHERE ${where.join(" AND ")} ORDER BY id DESC LIMIT ?`)
      .bind(...binds, limit + 1)
      .all<ActivityRow>();
    return { entries: rows.results.slice(0, limit).map(toEntry), hasMore: rows.results.length > limit };
  },
});

registerScheduled("*", async ({ env, log }) => {
  const r = await env.DB.prepare("DELETE FROM activity WHERE at < ?").bind(Date.now() - ACTIVITY_DAYS * DAY_MS).run();
  if (r.meta.changes) log.info("activity pruned", { rows: r.meta.changes });
});
