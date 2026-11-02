// chat-monitor: cron every 5 minutes (see wrangler.jsonc). One HTTP route, for operators:
//   POST /test-alert   Authorization: Bearer <MONITOR_ADMIN_SECRET>   body {"sink"?: "<target name>"}
// sends one alert worded as a test through the real alert path. Without the secret the route
// does not exist (404), like every other path.
import type { Env } from "./config";
import { type Deps, runMonitor, sendTestAlert } from "./run";

export { MonitorState } from "./state";

const json = (body: unknown, status = 200) => Response.json(body, { status, headers: { "cache-control": "no-store" } });

async function digest(s: string): Promise<ArrayBuffer> {
  return crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
}

/** Constant-time check of the bearer token against the admin secret. */
async function authorized(req: Request, secret: string): Promise<boolean> {
  const m = /^Bearer (.+)$/.exec(req.headers.get("authorization") ?? "");
  if (!m) return false;
  const [a, b] = await Promise.all([digest(m[1]), digest(secret)]);
  return crypto.subtle.timingSafeEqual(a, b);
}

export async function handleFetch(req: Request, env: Env, deps: Partial<Deps> = {}): Promise<Response> {
  const url = new URL(req.url);
  const secret = typeof env.MONITOR_ADMIN_SECRET === "string" && env.MONITOR_ADMIN_SECRET.length >= 16 ? env.MONITOR_ADMIN_SECRET : null;
  if (url.pathname !== "/test-alert" || !secret) return json({ error: { code: "not_found" } }, 404);
  if (req.method !== "POST") return json({ error: { code: "method_not_allowed" } }, 405);
  if (!(await authorized(req, secret))) return json({ error: { code: "unauthorized" } }, 401);
  let sink: string | undefined;
  const text = await req.text();
  if (text.trim() !== "") {
    let body: unknown;
    try {
      body = JSON.parse(text);
    } catch {
      return json({ error: { code: "invalid", message: "body must be JSON" } }, 400);
    }
    const s = (body as { sink?: unknown } | null)?.sink;
    if (body === null || typeof body !== "object" || Array.isArray(body) || (s !== undefined && (typeof s !== "string" || s === ""))) {
      return json({ error: { code: "invalid", message: "body is {\"sink\"?: string}" } }, 400);
    }
    sink = s as string | undefined;
  }
  const out = await sendTestAlert(env, { sink }, deps);
  if (out.ok) return json(out);
  const status = { unknown_target: 400, too_soon: 429, no_sink: 503, post_failed: 502 }[out.reason];
  return json({ error: { code: out.reason, message: out.detail } }, status);
}

export default {
  fetch: (req: Request, env: Env) => handleFetch(req, env),
  async scheduled(_controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    ctx.waitUntil(
      runMonitor(env).catch((err) => {
        console.error(JSON.stringify({ level: "error", svc: "chat-monitor", msg: "monitor run failed", err: String(err) }));
      }),
    );
  },
} satisfies ExportedHandler<Env>;
