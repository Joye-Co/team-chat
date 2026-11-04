-- Settings: the team's approval rules for AI members. scope '*' is the team row; other scopes are
-- reserved (for example one AI member's own rules later). JSON as in docs/protocol.md.
CREATE TABLE approval_policies (
  scope TEXT PRIMARY KEY,
  policy TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  updated_by TEXT
);
