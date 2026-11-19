-- A member's own AI member and the VM size of a machine (docs/plans/usable/design-personal-ai.zh.md,
-- docs/protocol.md "A member's own AI member and the VM size of a machine"). Old rows keep NULL:
-- the team's AI members and the daemon's own [vm] size, as before.
ALTER TABLE members ADD COLUMN owner_id TEXT;      -- an AI member's own person; NULL = the team's
CREATE INDEX members_owner ON members(owner_id);
ALTER TABLE machines ADD COLUMN vm_vcpus INTEGER;  -- NULL = the daemon's own [vm] vcpus
ALTER TABLE machines ADD COLUMN vm_mem_mib INTEGER;
