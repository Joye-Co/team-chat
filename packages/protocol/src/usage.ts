// 用量 of 团队管理 (docs/protocol.md "Team usage"): `GET /v1/usage?from=&to=` answers TeamUsage,
// the cost and tokens of AI members' runs that ended in [from, to), at API prices.
import { z } from "zod";
import { Id, Millis } from "./common.ts";

/** `?from=<ms>&to=<ms>`: at most 62 days; days are 24-hour buckets from `from`. */
export const UsageQuery = z
  .object({ from: z.coerce.number().int().nonnegative(), to: z.coerce.number().int().positive() })
  .refine((q) => q.to > q.from && q.to - q.from <= 62 * 86_400_000, "from < to, at most 62 days");

const Spend = {
  costUsd: z.number().nonnegative(),
  inputTokens: z.int().nonnegative(),
  outputTokens: z.int().nonnegative(),
  runs: z.int().nonnegative(),
  topics: z.int().nonnegative(),
  /** The part of `costUsd` estimated from tokens at list price (runs whose CLI reports no cost). */
  estimatedUsd: z.number().nonnegative().optional(),
};

export const TeamUsage = z.object({
  from: Millis,
  to: Millis,
  total: z.object(Spend),
  /** The same span right before `from`, to compare with. */
  previous: z.object({ costUsd: z.number().nonnegative(), topics: z.int().nonnegative() }),
  /** Sum of the AI members' weekly budgets (`weeklyBudgetCents`); absent when none has one. */
  teamBudgetCents: z.int().nonnegative().optional(),
  bots: z.array(z.object({ memberId: Id, ...Spend, budgetCents: z.int().nonnegative().optional() })).max(500),
  /** Who asked for the runs (the person behind each run). */
  initiators: z.array(z.object({ memberId: Id, ...Spend })).max(500),
  machines: z.array(z.object({ machineId: Id, ...Spend })).max(200),
  /** Who asked which AI member: one entry per (person, AI member), by cost. */
  pairs: z.array(z.object({ initiatorId: Id, memberId: Id, ...Spend })).max(2000).optional(),
  /** Per runtime and machine, so a client can tell subscription from API key by the machine's login. */
  runtimes: z.array(z.object({ runtime: z.string().min(1).max(64), machineId: Id.optional(), ...Spend })).max(500),
  /** One bucket per 24 hours from `from`, each with the cost per AI member. */
  daily: z
    .array(z.object({ start: Millis, costUsd: z.number().nonnegative(), bots: z.array(z.object({ memberId: Id, costUsd: z.number().nonnegative() })).max(500) }))
    .max(62),
});
export type TeamUsage = z.infer<typeof TeamUsage>;
