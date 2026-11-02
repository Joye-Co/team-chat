// chat-monitor: cron every 5 minutes (see wrangler.jsonc). No public HTTP surface.
import type { Env } from "./config";
import { runMonitor } from "./run";

export { MonitorState } from "./state";

export default {
  async fetch(): Promise<Response> {
    return new Response("not found", { status: 404 });
  },
  async scheduled(_controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    ctx.waitUntil(
      runMonitor(env).catch((err) => {
        console.error(JSON.stringify({ level: "error", svc: "chat-monitor", msg: "monitor run failed", err: String(err) }));
      }),
    );
  },
} satisfies ExportedHandler<Env>;
