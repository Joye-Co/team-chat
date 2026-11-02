// API Worker entry: routing, auth and the WebSocket upgrade.
import { authenticate } from "./auth";
import { type Env, gatewayStub } from "./env";
import { ApiError } from "./errors";
import { INTERNAL_HEADERS } from "./gateway";
import { type CompiledRoute, compilePath, errorResponse, json, matchRoute, readJson } from "./http";
import { logger } from "./logger";
import { LIMITS } from "./ratelimit";
import { type RouteContext, type RouteDef, routes, scheduledHandlers } from "./registry";
import { registerCoreRoutes } from "./routes";

registerCoreRoutes();

let compiled: CompiledRoute<RouteDef>[] | null = null;
let compiledCount = -1;

function table(): CompiledRoute<RouteDef>[] {
  if (!compiled || compiledCount !== routes.length) {
    compiled = routes.map((r) => ({ method: r.method, path: r.path, handler: r, ...compilePath(r.path) }));
    compiledCount = routes.length;
  }
  return compiled;
}

async function handleWebSocket(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  if (request.method !== "GET") throw new ApiError("invalid", "use GET");
  if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
    return json({ error: { code: "invalid", message: "expected a websocket upgrade" } }, 426);
  }
  // 401 before the upgrade.
  const auth = await authenticate(request, env.DB, (p) => ctx.waitUntil(p));
  const headers = new Headers(request.headers);
  for (const h of Object.values(INTERNAL_HEADERS)) headers.delete(h);
  headers.delete("authorization");
  headers.set(INTERNAL_HEADERS.memberId, auth.memberId);
  headers.set(INTERNAL_HEADERS.tokenId, auth.tokenId);
  headers.set(INTERNAL_HEADERS.level, auth.member.level);
  headers.set(INTERNAL_HEADERS.kind, auth.member.kind);
  return gatewayStub(env, auth.memberId).fetch(new Request(request.url, { method: "GET", headers }));
}

export async function handleFetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const url = new URL(request.url);
  const started = Date.now();
  const log = logger.child({ method: request.method, path: url.pathname, rayId: request.headers.get("cf-ray") });
  let status = 500;
  try {
    if (url.pathname === "/v1/ws") {
      const res = await handleWebSocket(request, env, ctx);
      status = res.status;
      return res;
    }
    const m = matchRoute(table(), request.method, url.pathname);
    if (!m.route) {
      throw m.pathMatched ? new ApiError("invalid", "method not allowed") : new ApiError("not_found", "no such route");
    }
    const def = m.route.handler;
    let auth = null;
    if (def.auth !== "none") {
      auth = await authenticate(request, env.DB, (p) => ctx.waitUntil(p));
      if (def.auth === "owner" && auth.member.level !== "owner") throw new ApiError("forbidden", "owner only");
    }
    const c: RouteContext = {
      request,
      env,
      ctx,
      url,
      params: m.params,
      auth,
      log: auth ? log.child({ memberId: auth.memberId }) : log,
      body: (schema, limit) => readJson(request, schema, limit ?? def.bodyLimit ?? LIMITS.httpBodyBytes),
    };
    const out = await def.handler(c);
    const res = out instanceof Response ? out : json(out);
    status = res.status;
    return res;
  } catch (err) {
    const res = errorResponse(err);
    status = res.status;
    return res;
  } finally {
    log.info("request", { status, ms: Date.now() - started });
  }
}

/** Cron entry: runs every handler registered for this cron (or for "*"). Errors are logged. */
export async function handleScheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
  const log = logger.child({ cron: controller.cron });
  const started = Date.now();
  const due = scheduledHandlers.filter((h) => h.cron === "*" || h.cron === controller.cron);
  const results = await Promise.allSettled(
    due.map((h) => h.handler({ env, ctx, cron: controller.cron, scheduledTime: controller.scheduledTime, log })),
  );
  const failed = results.filter((r) => r.status === "rejected");
  for (const r of failed) log.error("scheduled handler failed", { err: (r as PromiseRejectedResult).reason });
  log.info("scheduled", { handlers: due.length, failed: failed.length, ms: Date.now() - started });
}
