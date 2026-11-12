-- AI 服务 (server/src/aiservice/uses.ts), usable 模 1.1, 1.4, 1.9: the per-use switches, the last
-- call of each use and the failing alert, and the tokens of the server's own model calls.

-- What the team's model is used for: frontDesk, titles, memory. A missing row is on.
CREATE TABLE ai_service_switches (
  name TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  updated_by TEXT
);

-- The last call of each use (front_desk, title, summary, retro, tidy); `failures` in a row.
CREATE TABLE ai_service_health (
  use TEXT PRIMARY KEY,
  at INTEGER NOT NULL,
  ok INTEGER NOT NULL,
  error TEXT,
  failures INTEGER NOT NULL DEFAULT 0,
  ok_at INTEGER
);

-- At most one row (scope '*'): raised after several failed calls in a row, deleted by a good call.
CREATE TABLE ai_service_alert (
  scope TEXT PRIMARY KEY,
  since INTEGER NOT NULL,
  use TEXT NOT NULL,
  error TEXT NOT NULL
);

-- Calls and tokens per hour, use and model (用量's 前台和整理 line).
CREATE TABLE ai_service_calls (
  hour INTEGER NOT NULL,
  use TEXT NOT NULL,
  model TEXT NOT NULL,
  calls INTEGER NOT NULL DEFAULT 0,
  failures INTEGER NOT NULL DEFAULT 0,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  cache_write_tokens INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hour, use, model)
);
