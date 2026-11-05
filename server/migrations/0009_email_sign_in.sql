-- Email sign-in (docs/protocol.md, "Email sign-in"): one sign-in email per member, the pending
-- 6-digit codes (SHA-256 digests only) and, in local and e2e only, an outbox for tests.
CREATE TABLE member_emails (
  member_id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  verified_at INTEGER NOT NULL
);

-- purpose: 'sign_in' (member_id '') or 'add' (the member adding the address).
CREATE TABLE email_codes (
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  member_id TEXT NOT NULL,
  code_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (email, purpose, member_id)
);

CREATE TABLE email_outbox (
  email TEXT NOT NULL,
  purpose TEXT NOT NULL,
  code TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX email_outbox_email ON email_outbox (email, at);
