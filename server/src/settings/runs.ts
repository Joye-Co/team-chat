// The team's time limit for one run (docs/protocol.md, "Run time limit"; 盯 1.6, 机 2.9, 接 6.2):
// one row in D1 `run_settings` with scope '*'. BotDesk reads it when it assigns a run and when it
// checks a running one; administrators change it.
import { DEFAULT_RUN_LIMIT_MINUTES, type RunSettings, UpdateRunSettingsRequest } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { registerRoute } from "../core/registry";

const TEAM = "*";

export async function loadRunSettings(db: D1Database): Promise<RunSettings> {
  const row = await db.prepare("SELECT limit_minutes, updated_at FROM run_settings WHERE scope = ?").bind(TEAM).first<{ limit_minutes: number; updated_at: number }>();
  return row ? { limitMinutes: row.limit_minutes, updatedAt: row.updated_at } : { limitMinutes: DEFAULT_RUN_LIMIT_MINUTES, updatedAt: 0 };
}

/** The limit in ms; the default when D1 cannot be read (never blocks a run). */
export async function runLimitMs(db: D1Database): Promise<number> {
  try {
    return (await loadRunSettings(db)).limitMinutes * 60_000;
  } catch {
    return DEFAULT_RUN_LIMIT_MINUTES * 60_000;
  }
}

registerRoute({
  method: "GET",
  path: "/v1/settings/runs",
  auth: "member",
  handler: ({ env }) => loadRunSettings(env.DB),
});

registerRoute({
  method: "PATCH",
  path: "/v1/settings/runs",
  auth: "owner",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const update = await body(UpdateRunSettingsRequest);
    const before = await loadRunSettings(env.DB);
    if (update.limitMinutes === undefined) return before;
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO run_settings (scope, limit_minutes, updated_at, updated_by) VALUES (?, ?, ?, ?)
         ON CONFLICT (scope) DO UPDATE SET limit_minutes = excluded.limit_minutes, updated_at = excluded.updated_at, updated_by = excluded.updated_by`,
      ).bind(TEAM, update.limitMinutes, now, a.memberId),
      auditStatement(env.DB, a, { action: "settings.runs", target: TEAM, details: { before: before.limitMinutes, after: update.limitMinutes } }, now),
    ]);
    return { limitMinutes: update.limitMinutes, updatedAt: now };
  },
});
