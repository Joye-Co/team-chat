-- 到了预算先停下来问 (server/src/budget): what an owner added to an AI member's budget for one week
-- only (这周再加 $X). The weekly budget itself stays `members.weekly_budget_cents`.
CREATE TABLE budget_grants (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  member_id TEXT NOT NULL,
  week_start INTEGER NOT NULL,
  extra_cents INTEGER NOT NULL,
  granted_by TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX budget_grants_week ON budget_grants(member_id, week_start);
