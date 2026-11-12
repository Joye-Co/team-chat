-- Each person's GitHub connection (docs/plans/usable/design-github-identity.zh.md, 3.3).
-- The token is AES-GCM encrypted with HOOK_SECRET_KEY; additional data 'github:' || member_id.
CREATE TABLE github_accounts (
  member_id TEXT PRIMARY KEY REFERENCES members(id),
  github_user_id INTEGER NOT NULL UNIQUE,  -- one GitHub account connects to one member
  login TEXT NOT NULL,
  name TEXT,                               -- GitHub profile name; NULL = use login
  token_enc TEXT NOT NULL,                 -- v1.<iv>.<ct>
  scopes TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('ok', 'invalid')),
  connected_at INTEGER NOT NULL,
  checked_at INTEGER NOT NULL
);

-- A device code sign-in the server started and polls; 15 minutes.
CREATE TABLE github_device (
  id TEXT PRIMARY KEY,
  member_id TEXT NOT NULL,
  device_enc TEXT NOT NULL,                -- encrypted device_code, additional data 'github-device:' || id
  interval_s INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX github_device_member ON github_device(member_id);
