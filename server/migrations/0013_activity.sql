-- 活动记录 (团队管理, docs/protocol.md "Activity"): the admin events of the audit log, kept 90 days.
-- A trigger copies them as they are written, in the same transaction as the write; the cron prunes
-- rows older than 90 days. `kind` is what the page filters by; `machine_id` is set for machine,
-- key and CLI sign-in events.
CREATE TABLE activity (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  audit_id INTEGER NOT NULL,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  action TEXT NOT NULL,
  actor_id TEXT,
  target TEXT,
  machine_id TEXT,
  details TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX activity_at ON activity(at);
CREATE INDEX activity_kind ON activity(kind, at);
CREATE INDEX activity_actor ON activity(actor_id, at);
CREATE INDEX activity_machine ON activity(machine_id, at);

CREATE TRIGGER activity_from_audit AFTER INSERT ON audit
WHEN NEW.action IN ('member.update', 'member.deactivate', 'bot.create', 'bot.runner.set', 'bot.runner.delete',
  'token.revoke', 'invite.create', 'invite.redeem', 'machine.secret.set', 'machine.secret.delete', 'run.approval',
  'settings.approvals', 'email.sign_in', 'machine.login.start', 'machine.create', 'machine.revoke')
BEGIN
  INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details)
  VALUES (
    NEW.id,
    NEW.at,
    CASE
      WHEN NEW.action = 'member.update' AND json_extract(NEW.details, '$.level') IS NOT NULL THEN 'level'
      WHEN NEW.action = 'member.update' AND json_extract(NEW.details, '$.weeklyBudgetCents') IS NOT NULL THEN 'budget'
      WHEN NEW.action LIKE 'invite.%' THEN 'invite'
      WHEN NEW.action LIKE 'machine.secret.%' THEN 'key'
      WHEN NEW.action IN ('run.approval', 'settings.approvals') THEN 'approval'
      WHEN NEW.action IN ('email.sign_in', 'machine.login.start') THEN 'login'
      WHEN NEW.action LIKE 'machine.%' THEN 'machine'
      ELSE 'member'
    END,
    NEW.action,
    NEW.actor_id,
    NEW.target,
    CASE WHEN NEW.action LIKE 'machine.%' THEN NEW.target END,
    NEW.details
  );
END;

-- The last 90 days of the audit log, so the page is not empty after the upgrade.
INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details)
SELECT
  id,
  at,
  CASE
    WHEN action = 'member.update' AND json_extract(details, '$.level') IS NOT NULL THEN 'level'
    WHEN action = 'member.update' AND json_extract(details, '$.weeklyBudgetCents') IS NOT NULL THEN 'budget'
    WHEN action LIKE 'invite.%' THEN 'invite'
    WHEN action LIKE 'machine.secret.%' THEN 'key'
    WHEN action IN ('run.approval', 'settings.approvals') THEN 'approval'
    WHEN action IN ('email.sign_in', 'machine.login.start') THEN 'login'
    WHEN action LIKE 'machine.%' THEN 'machine'
    ELSE 'member'
  END,
  action,
  actor_id,
  target,
  CASE WHEN action LIKE 'machine.%' THEN target END,
  details
FROM audit
WHERE action IN ('member.update', 'member.deactivate', 'bot.create', 'bot.runner.set', 'bot.runner.delete',
  'token.revoke', 'invite.create', 'invite.redeem', 'machine.secret.set', 'machine.secret.delete', 'run.approval',
  'settings.approvals', 'email.sign_in', 'machine.login.start', 'machine.create', 'machine.revoke')
  AND at >= CAST(strftime('%s', 'now') AS INTEGER) * 1000 - 90 * 86400000
ORDER BY id;
