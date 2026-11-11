-- Password and code sign-in (docs/protocol.md, "Password sign-in" and "Code sign-in").
-- Passwords are stored only as PBKDF2 hashes; codes and secrets only as SHA-256 digests.
CREATE TABLE member_passwords (
  member_id TEXT PRIMARY KEY,
  hash TEXT NOT NULL,       -- pbkdf2-sha256$<iterations>$<salt>$<key>
  updated_at INTEGER NOT NULL
);

-- kind 'offer': a signed-in member (member_id) shows the code; a new device claims it.
-- kind 'request': a new device (device_name, secret_hash) shows the code; a member approves it.
-- state: 'waiting', 'approved' (request only), 'used'.
CREATE TABLE qr_sign_ins (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  code_hash TEXT NOT NULL,
  member_id TEXT NOT NULL DEFAULT '',
  device_name TEXT NOT NULL DEFAULT '',
  secret_hash TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX qr_sign_ins_code ON qr_sign_ins (code_hash, kind);
CREATE INDEX qr_sign_ins_member ON qr_sign_ins (member_id, kind, state);

-- 活动记录 (kind login) also lists the new sign-in ways.
CREATE TRIGGER activity_from_audit_sign_in AFTER INSERT ON audit
WHEN NEW.action IN ('password.sign_in', 'qr.sign_in')
BEGIN
  INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details)
  VALUES (NEW.id, NEW.at, 'login', NEW.action, NEW.actor_id, NEW.target, NULL, NEW.details);
END;
