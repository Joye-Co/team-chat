-- Personal and team machines (docs/plans/usable/design-roles.zh.md 3.3, docs/protocol.md
-- "Roles: 所有者 and personal machines"). Old machines keep owner_id NULL: team machines, as before.
ALTER TABLE machines ADD COLUMN owner_id TEXT;     -- NULL = a team machine; else the person whose own machine it is
CREATE INDEX machines_owner ON machines(owner_id);

-- "My work for this AI member runs on my machine": rows only on personal machines, set by the owner.
CREATE TABLE machine_bots (
  machine_id TEXT NOT NULL REFERENCES machines(id),
  bot_id TEXT NOT NULL REFERENCES members(id),
  repo_path TEXT,                                  -- the owner's code folder on this machine
  created_at INTEGER NOT NULL,
  PRIMARY KEY (machine_id, bot_id)
);
CREATE INDEX machine_bots_bot ON machine_bots(bot_id);

-- 活动记录 (kind machine) also lists renames, ownership changes and checked AI members.
CREATE TRIGGER activity_from_audit_machine_owner AFTER INSERT ON audit
WHEN NEW.action IN ('machine.update', 'machine.bots.set', 'machine.bots.delete')
BEGIN
  INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details)
  VALUES (NEW.id, NEW.at, 'machine', NEW.action, NEW.actor_id, NEW.target, NEW.target, NEW.details);
END;
