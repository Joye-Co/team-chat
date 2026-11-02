// Operations: daily archive to R2 (cron), restore (admin import routes) and core event replay.
import { registerScheduled } from "../core/registry";
import { runArchive } from "./archive";
import "./replay";
import "./state";
import { registerOpsRoutes } from "./routes";

/** Must match `triggers.crons` in server/wrangler.jsonc. */
export const ARCHIVE_CRON = "0 3 * * *";

registerOpsRoutes();

registerScheduled(ARCHIVE_CRON, async ({ env, log }) => {
  // Cron invocations may run up to 15 minutes of wall time; leave room for the last file.
  await runArchive(env, { log: log.child({ job: "archive" }), budgetMs: 10 * 60 * 1000 });
});
