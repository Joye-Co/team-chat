-- Integrations: incoming hooks and the GitHub login map. See docs/integrations.md.
-- The hook secret is stored AES-GCM encrypted with the HOOK_SECRET_KEY Worker secret, because
-- GitHub signs with the raw secret (a digest is not enough to verify HMAC signatures).
CREATE TABLE hooks (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('generic', 'github')),
  group_id TEXT NOT NULL REFERENCES groups(id),
  topic_id TEXT,
  member_id TEXT NOT NULL REFERENCES members(id),
  secret_enc TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  deleted_at INTEGER
);
CREATE INDEX hooks_group ON hooks(group_id);

-- GitHub login (lower case) -> member, for reviewers and authors in waitingOn and ownerId.
CREATE TABLE github_logins (
  login TEXT PRIMARY KEY,
  member_id TEXT NOT NULL REFERENCES members(id),
  updated_at INTEGER NOT NULL
);
