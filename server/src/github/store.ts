// GitHub connections in D1 (`github_accounts`, `github_device`). Tokens and device codes are
// AES-GCM encrypted with HOOK_SECRET_KEY; the additional data binds a token to its member
// (`github:<memberId>`) and a device code to its sign-in (`github-device:<id>`), so rows cannot
// be swapped. Decrypted values are returned to callers and never logged.
import { FEATURE_GITHUB, type GithubAccount } from "@chat/protocol";
import product from "../../../product.json";
import { featureTransient } from "../core/gateway";
import { type Env, gatewayStub } from "../core/env";
import { logger } from "../core/logger";
import { decryptSecret, encryptSecret } from "../integrations/secrets";
import type { GithubEnv } from "./api";

/** The OAuth App client id: the Worker variable `GITHUB_CLIENT_ID`, else product.json. */
export function clientId(env: GithubEnv): string | null {
  const id = (env.GITHUB_CLIENT_ID || (product as { githubClientId?: string }).githubClientId || "").trim();
  return id || null;
}

/** Why nobody can connect GitHub on this server, or null when they can. */
export function unavailableReason(env: GithubEnv): "no_client_id" | "no_secret_key" | null {
  if (!clientId(env)) return "no_client_id";
  // Same rule as the key derivation in integrations/secrets.ts.
  if (!env.HOOK_SECRET_KEY || env.HOOK_SECRET_KEY.length < 16) return "no_secret_key";
  return null;
}

export type AccountRow = {
  member_id: string;
  github_user_id: number;
  login: string;
  name: string | null;
  token_enc: string;
  scopes: string;
  status: "ok" | "invalid";
  connected_at: number;
  checked_at: number;
};

export function loadAccount(db: D1Database, memberId: string): Promise<AccountRow | null> {
  return db.prepare("SELECT * FROM github_accounts WHERE member_id = ?").bind(memberId).first<AccountRow>();
}

export function toAccount(row: AccountRow): GithubAccount {
  const a: GithubAccount = { login: row.login, status: row.status, connectedAt: row.connected_at };
  if (row.name) a.name = row.name;
  return a;
}

const tokenAad = (memberId: string) => `github:${memberId}`;
const deviceAad = (deviceId: string) => `github-device:${deviceId}`;

export function encryptToken(env: GithubEnv, memberId: string, token: string): Promise<string> {
  return encryptSecret(env, tokenAad(memberId), token);
}

export function decryptToken(env: GithubEnv, memberId: string, stored: string): Promise<string> {
  return decryptSecret(env, tokenAad(memberId), stored);
}

export function encryptDeviceCode(env: GithubEnv, deviceId: string, code: string): Promise<string> {
  return encryptSecret(env, deviceAad(deviceId), code);
}

export function decryptDeviceCode(env: GithubEnv, deviceId: string, stored: string): Promise<string> {
  return decryptSecret(env, deviceAad(deviceId), stored);
}

/**
 * The person's token for a run (G3's machine route), or null when not connected or marked
 * invalid. The caller must not log it.
 */
export async function loadToken(env: GithubEnv, memberId: string): Promise<{ token: string; login: string } | null> {
  const row = await loadAccount(env.DB, memberId);
  if (!row || row.status !== "ok") return null;
  return { token: await decryptToken(env, memberId, row.token_enc), login: row.login };
}

/** Marks the connection invalid (GitHub answered 401) and tells the person's clients. */
export async function markInvalid(env: Env, memberId: string): Promise<void> {
  const now = Date.now();
  const r = await env.DB.prepare("UPDATE github_accounts SET status = 'invalid', checked_at = ? WHERE member_id = ? AND status = 'ok'")
    .bind(now, memberId)
    .run();
  if (!r.meta.changes) return;
  const row = await loadAccount(env.DB, memberId);
  await pushStatus(env, memberId, row ? toAccount(row) : undefined);
}

/** `github.status` to the person's own connections that said `github/1`. Best effort. */
export async function pushStatus(env: Env, memberId: string, account: GithubAccount | undefined): Promise<void> {
  const event: Record<string, unknown> = { type: "github.status" };
  if (account) event.account = account;
  try {
    await gatewayStub(env, memberId).deliver([featureTransient(FEATURE_GITHUB, event)]);
  } catch (err) {
    logger.error("github status push failed", { memberId, err });
  }
}

/** A deactivated member's connection and sign-ins in progress go away. */
export async function forgetMember(env: Env, memberId: string): Promise<void> {
  await env.DB.batch([
    env.DB.prepare("DELETE FROM github_accounts WHERE member_id = ?").bind(memberId),
    env.DB.prepare("DELETE FROM github_device WHERE member_id = ?").bind(memberId),
  ]);
}
