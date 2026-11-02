-- A fixed-window counter of invite code attempts per client (server/src/core/ratelimit.ts):
-- the 8-character codes are short, so guessing is limited per IP.
CREATE TABLE auth_rate_hits (
  key TEXT NOT NULL,       -- 'redeem:' + SHA-256 of the client IP (the IP is not stored)
  window INTEGER NOT NULL, -- window start, ms
  count INTEGER NOT NULL,
  PRIMARY KEY (key, window)
);
