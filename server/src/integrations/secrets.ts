// Hook secrets: generation, encryption at rest and constant-time checks.
import { timingSafeEqualString } from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";

/** The Worker secret that encrypts hook secrets at rest (`wrangler secret put HOOK_SECRET_KEY`). */
export type HooksEnv = Env & { HOOK_SECRET_KEY?: string };

const enc = new TextEncoder();

function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromB64url(s: string): Uint8Array {
  const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** `whk_` + 43 base64url characters (32 random bytes). */
export function newHookSecret(): string {
  return `whk_${b64url(crypto.getRandomValues(new Uint8Array(32)))}`;
}

async function aesKey(env: HooksEnv): Promise<CryptoKey> {
  const secret = env.HOOK_SECRET_KEY;
  if (!secret || secret.length < 16) throw new ApiError("internal", "hooks are not configured on this server");
  const raw = await crypto.subtle.digest("SHA-256", enc.encode(secret));
  return crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt", "decrypt"]);
}

/** `v1.<iv>.<ciphertext>`; the hook id is the additional data, so rows cannot be swapped. */
export async function encryptSecret(env: HooksEnv, hookId: string, secret: string): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: enc.encode(hookId) },
    await aesKey(env),
    enc.encode(secret),
  );
  return `v1.${b64url(iv)}.${b64url(new Uint8Array(ct))}`;
}

export async function decryptSecret(env: HooksEnv, hookId: string, stored: string): Promise<string> {
  const [v, iv, ct] = stored.split(".");
  if (v !== "v1" || !iv || !ct) throw new ApiError("internal", "bad stored hook secret");
  try {
    const pt = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: fromB64url(iv), additionalData: enc.encode(hookId) },
      await aesKey(env),
      fromB64url(ct),
    );
    return new TextDecoder().decode(pt);
  } catch {
    throw new ApiError("internal", "cannot decrypt hook secret");
  }
}

export function checkSecret(given: string, expected: string): Promise<boolean> {
  return timingSafeEqualString(given, expected);
}

/** HMAC-SHA256 hex signature as GitHub sends it: `sha256=<hex>`. */
export async function signGithub(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", enc.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, enc.encode(body)));
  return `sha256=${[...sig].map((b) => b.toString(16).padStart(2, "0")).join("")}`;
}

/** Verifies `X-Hub-Signature-256`. `crypto.subtle.verify` compares in constant time. */
export async function verifyGithub(secret: string, body: string, header: string | null): Promise<boolean> {
  const m = /^sha256=([0-9a-f]{64})$/i.exec(header?.trim() ?? "");
  if (!m) return false;
  const hex = m[1];
  const sig = new Uint8Array(32);
  for (let i = 0; i < 32; i++) sig[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  const key = await crypto.subtle.importKey("raw", enc.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  return crypto.subtle.verify("HMAC", key, sig, enc.encode(body));
}
