// Run plans: a bot's ordered to-do list for one run. See docs/protocol.md, "Run plans".
import { z } from "zod";

/** Max items of a run plan. */
export const MAX_PLAN_ITEMS = 30;
/** Max length of one plan item's text, in Unicode code points (as a topic title). */
export const MAX_PLAN_TEXT_CHARS = 200;

/** done = finished; current = the bot works on it now; waiting = waits for a member; pending = later. */
export const RunPlanState = z.enum(["done", "current", "waiting", "pending"]);
export type RunPlanState = z.infer<typeof RunPlanState>;
export const RunPlanItem = z.object({
  text: z.string().min(1).max(MAX_PLAN_TEXT_CHARS),
  state: RunPlanState,
  /** 1.1: when the item became `current` (ms). */
  startedAt: z.int().nonnegative().optional(),
  /** 1.1: when the item became `done` (ms). */
  endedAt: z.int().nonnegative().optional(),
  /** 1.1: how long the bot expects the item to take (ms). */
  estimateMs: z.int().nonnegative().optional(),
});
export type RunPlanItem = z.infer<typeof RunPlanItem>;
/** The bot's to-do list for a run, in order. An update replaces the whole list; `[]` clears it. */
export const RunPlan = z.array(RunPlanItem).max(MAX_PLAN_ITEMS);
export type RunPlan = z.infer<typeof RunPlan>;
