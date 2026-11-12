-- Whose GitHub a run uses and the runs that wait for a person to connect it
-- (docs/plans/usable/design-github-identity.zh.md 3.3, unit G3). Old runs keep NULL: no token.
ALTER TABLE bot_runs ADD COLUMN github_member_id TEXT;  -- the person the run acts as on GitHub, set when it is assigned

CREATE TABLE github_waits (
  run_id TEXT PRIMARY KEY,
  member_id TEXT NOT NULL,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  bot_id TEXT NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN ('not_connected', 'invalid')),
  created_at INTEGER NOT NULL
);
CREATE INDEX github_waits_member ON github_waits(member_id);
CREATE INDEX github_waits_topic ON github_waits(topic_id, bot_id);
