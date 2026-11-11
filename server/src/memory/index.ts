// 沉淀 (memory, docs/protocol.md "Memory"; agent-harness.zh.md section 7): the `/v1/memories`
// routes, the run's memory note, the review after a run (retro.ts, called by the desk), the
// daily consolidation (on every cron trigger, hourly; it runs once a day after 03:00 team time) and the
// index prompts carry (inject.ts).
import { registerScheduled } from "../core/registry";
import { consolidateAll } from "./consolidate";
import { registerMemoryRoutes } from "./routes";

export { memoryIndex, EXECUTOR_MEMORY_BYTES, FRONT_DESK_MEMORY_BYTES } from "./inject";
export { retroAfterRun } from "./retro";

registerMemoryRoutes();

registerScheduled("*", async ({ env, log }) => {
  const res = await consolidateAll(env, { log });
  if (res.scopes.length > 0 || res.ops > 0) log.info("memory consolidated", { day: res.day, scopes: res.scopes.length, ops: res.ops });
});
