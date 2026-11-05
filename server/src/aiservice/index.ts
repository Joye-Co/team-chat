// AI 服务 (docs/protocol.md, "AI 服务"; 设置 › 通用 › AI 服务): the owner picks the model provider
// and API key for the server's own model calls. Owner-only routes; the key is encrypted at rest
// and never returned. Registered from src/features.ts.
import { type AiServiceTestResult, TestAiServiceRequest, UpdateAiServiceRequest } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { PING, callModel, type ModelConfig } from "./providers";
import { type AiRow, SCOPE, UNSET, decryptKey, encryptKey, keyHint, loadRow, publicConfig } from "./store";

export { loadModelConfig } from "./store";
export { callModel } from "./providers";

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
  return { provider: req.provider, model: req.model, baseUrl, apiKey: await decryptKey(env, saved.key_cipher) };
}

registerRoute({
  method: "GET",
  path: "/v1/settings/ai",
  auth: "owner",
  handler: async ({ env }) => publicConfig(await loadRow(env.DB)),
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
      auditStatement(env.DB, a, { action: "settings.ai", target: SCOPE, details: { before: auditable(before), after: auditable(row), keyChanged: Boolean(req.apiKey) } }, now),
    ]);
    return publicConfig(row);
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/settings/ai",
  auth: "owner",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    const before = await loadRow(env.DB);
    if (!before) return UNSET;
    await env.DB.batch([
      env.DB.prepare("DELETE FROM ai_service WHERE scope = ?").bind(SCOPE),
      auditStatement(env.DB, a, { action: "settings.ai", target: SCOPE, details: { before: auditable(before), after: null, keyChanged: true } }),
    ]);
    return UNSET;
  },
});

registerRoute({
  method: "POST",
  path: "/v1/settings/ai/test",
  auth: "owner",
  async handler({ env, body }): Promise<AiServiceTestResult> {
    const req = await body(TestAiServiceRequest);
    const cfg = await resolve(env, req, await loadRow(env.DB));
    const started = Date.now();
    const result = await callModel(cfg, PING);
    const latencyMs = Date.now() - started;
    return result.ok ? { ok: true, latencyMs } : { ok: false, error: result.error, latencyMs };
  },
});
