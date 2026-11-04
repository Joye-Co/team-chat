-- 1.1 成员 page: who a member reports to (汇报给) and its weekly spending limit in US cents (限额),
-- both set by an owner through PATCH /v1/members/:id. NULL = none (docs/protocol.md, Member).
ALTER TABLE members ADD COLUMN reports_to TEXT;
ALTER TABLE members ADD COLUMN weekly_budget_cents INTEGER;
