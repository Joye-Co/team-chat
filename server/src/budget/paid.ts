// 模 4.1 (docs/plans/usable.zh.md section 4, decided): runs on a subscription sign-in cost the team
// nothing extra, so they do not count against the weekly budget (budget/index.ts) nor toward
// 到了预算 in 活动记录 (activity/events.ts).
import type { UsageRun } from "../bots/desk/external";
import { linkStub } from "../bots/env";
import type { Env } from "../core/env";

/** How each runtime's CLI on a machine is signed in ("subscription" | "api_key"); {} when unknown. */
export type LoginMethodsOf = (machineId: string) => Promise<Record<string, string>>;

/** From each machine's latest heartbeat. */
export const machineLoginMethods =
  (env: Env): LoginMethodsOf =>
  (machineId) =>
    linkStub(env, machineId)
      .loginMethods()
      .catch(() => ({}));

/**
 * The runs that count against a budget: all but those that ran on a subscription sign-in. A run
 * keeps the sign-in it was sent with (`loginMethod`); only a run from before that was kept falls
 * back to its machine's sign-in now (the rule 用量 uses for 订阅里已包含). A run with no machine,
 * or a sign-in nobody reported, counts.
 */
export async function paidRuns(runs: UsageRun[], methodsOf: LoginMethodsOf): Promise<UsageRun[]> {
  const ids = [...new Set(runs.flatMap((r) => (!r.loginMethod && r.machineId && r.runtime ? [r.machineId] : [])))];
  const methods = new Map(await Promise.all(ids.map(async (id) => [id, await methodsOf(id)] as const)));
  const methodOf = (r: UsageRun) => r.loginMethod || (r.machineId && r.runtime ? methods.get(r.machineId)?.[r.runtime] : undefined);
  return runs.filter((r) => methodOf(r) !== "subscription");
}
