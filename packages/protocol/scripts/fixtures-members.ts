// 1.1 member fields of the 成员 page: 汇报给 (reportsTo) and the weekly spending limit (限额).
// Called by gen-fixtures.ts.
import { MAX_WEEKLY_BUDGET_CENTS } from "../src/index.ts";
import type { InteractCtx } from "./fixtures-v11-interact.ts";

export function addMemberFixtures({ ok, bad, ID }: InteractCtx) {
  const forge = { id: ID.bot, kind: "bot", name: "Forge", role: "开发", level: "member", character: "o08" };
  ok("entities/member-reports-budget.json", "Member", { ...forge, reportsTo: ID.owner, weeklyBudgetCents: 5000 }, "a bot that reports to Joye with a weekly limit of 50 USD");
  ok("entities/member-budget-zero.json", "Member", { ...forge, weeklyBudgetCents: 0 }, "a limit of 0: the member may spend nothing");
  bad("entities/member-budget-negative.json", "Member", { ...forge, weeklyBudgetCents: -1 }, "the limit is not negative");
  bad("entities/member-budget-fraction.json", "Member", { ...forge, weeklyBudgetCents: 12.5 }, "the limit is whole cents");
  ok("http/update-member-request-reports-budget.json", "UpdateMemberRequest", { reportsTo: ID.owner, weeklyBudgetCents: 5000 }, "owner sets who the member reports to and its weekly limit");
  ok("http/update-member-request-clear-reports-budget.json", "UpdateMemberRequest", { reportsTo: null, weeklyBudgetCents: null }, "null clears both");
  ok("http/update-member-request-budget-max.json", "UpdateMemberRequest", { weeklyBudgetCents: MAX_WEEKLY_BUDGET_CENTS }, "limit: the largest weekly limit");
  bad("http/update-member-request-budget-too-big.json", "UpdateMemberRequest", { weeklyBudgetCents: MAX_WEEKLY_BUDGET_CENTS + 1 }, "limit: above the largest weekly limit");
}
