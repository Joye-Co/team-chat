// The team's AI 服务 row in D1 `ai_service` (scope '*'): read for the owner without the key,
// and with the decrypted key for the server's own model calls.
import type { AiProvider, AiServiceConfig } from "@chat/protocol";
import type { Env } from "../core/env";
import { decryptSecret, encryptSecret } from "../integrations/secrets";
import type { ModelConfig } from "./providers";

export const SCOPE = "*";
/** Additional data of the key's ciphertext, so it cannot be swapped with a hook secret. */
const AAD = `ai-service:${SCOPE}`;

export type AiRow = {
  provider: AiProvider;
  model: string;
  base_url: string | null;
  key_cipher: string;
  key_hint: string;
  updated_at: number;
};

export const UNSET: AiServiceConfig = { provider: null, model: "", baseUrl: null, keySet: false, keyHint: null, updatedAt: 0 };

export function loadRow(db: D1Database): Promise<AiRow | null> {
  return db
    .prepare("SELECT provider, model, base_url, key_cipher, key_hint, updated_at FROM ai_service WHERE scope = ?")
    .bind(SCOPE)
    .first<AiRow>();
}

export function publicConfig(row: AiRow | null): AiServiceConfig {
  if (!row) return UNSET;
  return { provider: row.provider, model: row.model, baseUrl: row.base_url, keySet: true, keyHint: row.key_hint, updatedAt: row.updated_at };
}

export function keyHint(key: string): string {
  return key.slice(-4);
}

export function encryptKey(env: Env, key: string): Promise<string> {
  return encryptSecret(env, AAD, key);
}

export function decryptKey(env: Env, stored: string): Promise<string> {
  return decryptSecret(env, AAD, stored);
}

/** The config for a model call, or null when the team has not set one up (or it cannot be read). */
export async function loadModelConfig(env: Env): Promise<ModelConfig | null> {
  const row = await loadRow(env.DB);
  if (!row) return null;
  return { provider: row.provider, model: row.model, baseUrl: row.base_url, apiKey: await decryptKey(env, row.key_cipher) };
}
