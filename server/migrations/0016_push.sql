-- Phones that get pushes (docs/protocol.md, "Push"). One row per push token; a device belongs to
-- the sign-in (token) that registered it and goes away with it.
CREATE TABLE push_devices (
  id TEXT PRIMARY KEY,
  member_id TEXT NOT NULL,
  token_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  push_token TEXT NOT NULL UNIQUE,
  app_id TEXT NOT NULL,
  environment TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX push_devices_member ON push_devices(member_id);
CREATE INDEX push_devices_token ON push_devices(token_id);
