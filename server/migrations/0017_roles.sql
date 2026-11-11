-- Usable, three roles (docs/plans/usable/design-roles.zh.md 3.1): a 所有者 is an admin
-- (level 'owner') with team_owner = 1. Every human admin today becomes a 所有者, so nobody loses
-- a power they have; AI members keep their level and are never a 所有者.
ALTER TABLE members ADD COLUMN team_owner INTEGER NOT NULL DEFAULT 0;
UPDATE members SET team_owner = 1 WHERE kind = 'human' AND level = 'owner';
