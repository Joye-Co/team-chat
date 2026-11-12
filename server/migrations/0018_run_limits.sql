-- Usable (盯 1.6, 接 6.2): the team's time limit for one run of an AI member, in minutes. One row
-- with scope '*'; other scopes are reserved (for example one AI member's own limit later). No row
-- = the default (120 minutes).
CREATE TABLE run_settings (
  scope TEXT PRIMARY KEY,
  limit_minutes INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  updated_by TEXT
);
