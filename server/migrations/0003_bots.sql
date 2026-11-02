-- Bots on the server: machines (daemon credentials), runner-backed bot configuration and an
-- index of bot runs (the run log itself lives in each bot's BotDesk Durable Object).
CREATE TABLE machines (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  labels TEXT NOT NULL DEFAULT '[]',
  -- Only the SHA-256 hex digest of the machine token (tm_...) is stored.
  token_hash TEXT NOT NULL UNIQUE,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER,
  last_seen_at INTEGER
);

CREATE TABLE bot_runners (
  member_id TEXT PRIMARY KEY REFERENCES members(id),
  runtime TEXT NOT NULL,
  labels TEXT NOT NULL DEFAULT '[]',
  repo_url TEXT,
  repo_path TEXT,
  base_branch TEXT,
  model TEXT,
  env_allowlist TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  updated_by TEXT,
  updated_at INTEGER NOT NULL
);

CREATE TABLE bot_runs (
  id TEXT PRIMARY KEY,
  bot_id TEXT NOT NULL,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  machine_id TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX bot_runs_bot ON bot_runs(bot_id, created_at);
CREATE INDEX bot_runs_topic ON bot_runs(topic_id, created_at);
