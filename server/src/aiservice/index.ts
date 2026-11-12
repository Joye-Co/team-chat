// AI 服务 (docs/protocol.md, "AI 服务"; 设置 › 通用 › AI 服务): an admin picks the model provider
// and API key for the server's own model calls (前台, titles, summaries, the review after a run,
// the nightly tidy). Admin-only routes; the key is encrypted at rest and never returned. Any
// member reads whether it is set up (`/status`). Registered from src/features.ts.
import {
  type AiServiceCheck,
  type AiServiceConfig,
  type AiServiceTestResult,
  TestAiServiceRequest,
  UpdateAiServiceRequest,
  UpdateAiServiceSwitchesRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { registerRoute, registerScheduled } from "../core/registry";
import type { HooksEnv } from "../integrations/secrets";
import { PING, callModel, type ModelConfig } from "./providers";
import { type AiRow, SCOPE, decryptKey, encryptKey, keyHint, loadRow, publicConfig } from "./store";
import { PING_TOOL, callTool } from "./tools";
import { CALLS_KEEP_MS, afterSwitchedOff, aiStatus, healthFields, loadSwitches, resetHealthStatements, switchStatements, switchedOffStatements } from "./uses";

export { loadModelConfig } from "./store";
export { callModel } from "./providers";
export { tracker, useConfig } from "./uses";

/** 模 1.6: the Worker secret the key is encrypted with, named when it is missing. */
export const SECRET_MISSING =
  "the Worker secret HOOK_SECRET_KEY is not set on this server (it encrypts the AI 服务 key): run `wrangler secret put HOOK_SECRET_KEY` with at least 16 characters, then save again";

/**
 * 测试连接: each check is one attempt with this timeout, no retries, so both together answer
 * well inside the clients' 30 s HTTP timeout and a timeout still reaches the admin as `error`.
 */
export const TEST_CHECK_MS = 12_000;

export function requireSecret(env: Env): void {
  const secret = (env as HooksEnv).HOOK_SECRET_KEY;
  if (!secret || secret.length < 16) throw new ApiError("internal", SECRET_MISSING, "ai.secret_missing");
}

/** What the audit log keeps of a config: never the key. */
function auditable(row: Pick<AiRow, "provider" | "model" | "base_url"> | null) {
  return row ? { provider: row.provider, model: row.model, baseUrl: row.base_url } : null;
}

/** The request's values with a key: the given one, or the saved one when the provider is unchanged. */
async function resolve(env: Env, req: UpdateAiServiceRequest, saved: AiRow | null): Promise<ModelConfig> {
  const baseUrl = req.provider === "openai-compatible" ? (req.baseUrl ?? null) : null;
  if (req.apiKey) return { provider: req.provider, model: req.model, baseUrl, apiKey: req.apiKey };
  if (!saved) throw new ApiError("invalid", "apiKey is required");
  if (saved.provider !== req.provider) throw new ApiError("invalid", "apiKey is required when the provider changes");
  requireSecret(env);
  let apiKey: string;
  try {
    apiKey = await decryptKey(env, saved.key_cipher);
  } catch {
    throw new ApiError("invalid", "the saved API key cannot be read (the Worker secret HOOK_SECRET_KEY changed): enter the key again", "ai.key_unreadable");
  }
  return { provider: req.provider, model: req.model, baseUrl, apiKey };
}

/** The admin's view: the config without the key, the switches and how each use is doing. */
async function adminConfig(env: Env, row: AiRow | null): Promise<AiServiceConfig> {
  return { ...publicConfig(row), ...(await healthFields(env.DB)) };
}

registerRoute({
  method: "GET",
  path: "/v1/settings/ai",
  auth: "owner",
  handler: async ({ env }) => adminConfig(env, await loadRow(env.DB)),
});

registerRoute({
  method: "GET",
  path: "/v1/settings/ai/status",
  auth: "member",
  handler: ({ env }) => aiStatus(env.DB),
});

registerRoute({
  method: "PUT",
  path: "/v1/settings/ai",
  auth: "owner",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const req = await body(UpdateAiServiceRequest);
    const before = await loadRow(env.DB);
    const cfg = await resolve(env, req, before);
    requireSecret(env);
    const now = Date.now();
    const cipher = req.apiKey ? await encryptKey(env, cfg.apiKey) : (before as AiRow).key_cipher;
    const row: AiRow = { provider: cfg.provider, model: cfg.model, base_url: cfg.baseUrl, key_cipher: cipher, key_hint: keyHint(cfg.apiKey), updated_at: now };
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO ai_service (scope, provider, model, base_url, key_cipher, key_hint, updated_at, updated_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (scope) DO UPDATE SET provider = excluded.provider, model = excluded.model, base_url = excluded.base_url,
           key_cipher = excluded.key_cipher, key_hint = excluded.key_hint, updated_at = excluded.updated_at, updated_by = excluded.updated_by`,
      ).bind(SCOPE, row.provider, row.model, row.base_url, row.key_cipher, row.key_hint, now, a.memberId),
      ...resetHealthStatements(env.DB),
      auditStatement(env.DB, a, { action: "settings.ai", target: SCOPE, details: { before: auditable(before), after: auditable(row), keyChanged: Boolean(req.apiKey) } }, now),
    ]);
    return adminConfig(env, row);
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/settings/ai",
  auth: "owner",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    const before = await loadRow(env.DB);
    if (!before) return adminConfig(env, null);
    await env.DB.batch([
      env.DB.prepare("DELETE FROM ai_service WHERE scope = ?").bind(SCOPE),
      ...resetHealthStatements(env.DB),
      auditStatement(env.DB, a, { action: "settings.ai", target: SCOPE, details: { before: auditable(before), after: null, keyChanged: true } }),
    ]);
    return adminConfig(env, null);
  },
});

registerRoute({
  method: "PUT",
  path: "/v1/settings/ai/switches",
  auth: "owner",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const req = await body(UpdateAiServiceSwitchesRequest);
    const before = await loadSwitches(env.DB);
    const after = { ...before, ...Object.fromEntries(Object.entries(req).filter(([, v]) => v !== undefined)) };
    const now = Date.now();
    // A use switched off makes no calls: its record goes, and the alert with it when no other use
    // still fails (else the alert would stay up until the use is switched on again).
    const sw = switchStatements(env.DB, req, a.memberId, now);
    const off = switchedOffStatements(env.DB, req);
    const res = await env.DB.batch([
      ...sw,
      ...off,
      auditStatement(env.DB, a, { action: "settings.ai", target: SCOPE, details: { switches: { before, after } } }, now),
    ]);
    if (off.length > 0) await afterSwitchedOff(env.DB, res[sw.length + off.length - 1], req, now, a.memberId);
    return adminConfig(env, await loadRow(env.DB));
  },
});

registerRoute({
  method: "POST",
  path: "/v1/settings/ai/test",
  auth: "owner",
  async handler({ env, body }): Promise<AiServiceTestResult> {
    const req = await body(TestAiServiceRequest);
    // Saving needs the secret: say so before the admin saves, whether or not a key is given.
    requireSecret(env);
    const cfg = await resolve(env, req, await loadRow(env.DB));
    const started = Date.now();
    // 模 1.7: titles and summaries make a plain text call; the front desk, the review and the
    // tidy a forced tool call, which some models cannot make. Both are tried; each says how it went.
    const once = { retries: 0, timeoutMs: TEST_CHECK_MS };
    const textRes = await callModel(cfg, PING, once);
    const text: AiServiceCheck = textRes.ok ? { ok: true } : { ok: false, error: textRes.error };
    let tool: AiServiceCheck | undefined;
    if (textRes.ok) {
      const toolRes = await callTool(cfg, PING_TOOL, once);
      tool = toolRes.ok ? { ok: true } : { ok: false, error: toolRes.error };
    }
    const latencyMs = Date.now() - started;
    const error = !text.ok ? text.error : tool && !tool.ok ? `forced tool call: ${tool.error ?? ""}`.slice(0, 500) : undefined;
    return { ok: error === undefined, ...(error !== undefined ? { error } : {}), latencyMs, text, ...(tool ? { tool } : {}) };
  },
});

// The hourly token counts are small; the daily cron drops the ones nobody can ask for any more.
registerScheduled("0 3 * * *", async ({ env, log }) => {
  try {
    await env.DB.prepare("DELETE FROM ai_service_calls WHERE hour < ?").bind(Date.now() - CALLS_KEEP_MS).run();
  } catch (err) {
    log.warn("ai service calls prune failed", { err });
  }
});
