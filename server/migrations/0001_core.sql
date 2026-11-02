-- Core directory: members, credentials, invites, groups, membership, audit.
CREATE TABLE members (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('human', 'bot')),
  name TEXT NOT NULL,
  role TEXT,
  level TEXT NOT NULL CHECK (level IN ('owner', 'member')),
  avatar_url TEXT,
  deactivated INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  created_by TEXT
);

-- Only the SHA-256 hex digest of a token is stored.
CREATE TABLE tokens (
  id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  member_id TEXT NOT NULL REFERENCES members(id),
  label TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER
);
CREATE INDEX tokens_member ON tokens(member_id);

-- Only the SHA-256 hex digest of the normalized invite code is stored. Single use.
CREATE TABLE invites (
  id TEXT PRIMARY KEY,
  code_hash TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('human', 'bot')),
  level TEXT NOT NULL CHECK (level IN ('owner', 'member')),
  role TEXT,
  created_by TEXT NOT NULL REFERENCES members(id),
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  used_at INTEGER,
  used_by TEXT
);

CREATE TABLE groups (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('group', 'dm')),
  name TEXT NOT NULL,
  dm_key TEXT UNIQUE,
  is_default INTEGER NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE group_members (
  group_id TEXT NOT NULL REFERENCES groups(id),
  member_id TEXT NOT NULL REFERENCES members(id),
  added_at INTEGER NOT NULL,
  added_by TEXT,
  PRIMARY KEY (group_id, member_id)
);
CREATE INDEX group_members_member ON group_members(member_id);

CREATE TABLE audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  actor_id TEXT,
  credential_id TEXT,
  action TEXT NOT NULL,
  target TEXT,
  details TEXT NOT NULL DEFAULT '{}',
  at INTEGER NOT NULL
);
CREATE INDEX audit_at ON audit(at);
CREATE INDEX audit_actor ON audit(actor_id, at);
