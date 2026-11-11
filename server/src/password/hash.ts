// The slow password hash: PBKDF2-HMAC-SHA256 through WebCrypto, which Workers run natively.
// 100 000 iterations is the most Workers allow. Stored as
// `pbkdf2-sha256$<iterations>$<salt>$<key>` (base64url), so the count can rise later.
import { timingSafeEqualString } from "../core/crypto";

export const PBKDF2_ITERATIONS = 100_000;
const SALT_BYTES = 16;
const KEY_BITS = 256;

function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromB64url(s: string): Uint8Array {
  const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

async function derive(password: string, salt: Uint8Array, iterations: number): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations }, key, KEY_BITS);
  return new Uint8Array(bits);
}

export async function hashPassword(password: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(SALT_BYTES));
  const key = await derive(password, salt, PBKDF2_ITERATIONS);
  return `pbkdf2-sha256$${PBKDF2_ITERATIONS}$${b64url(salt)}$${b64url(key)}`;
}

/** True when `password` matches `stored`; false for a malformed hash. */
export async function verifyPassword(password: string, stored: string): Promise<boolean> {
  const [alg, iter, salt, key] = stored.split("$");
  const iterations = Number(iter);
  if (alg !== "pbkdf2-sha256" || !Number.isInteger(iterations) || iterations < 1 || iterations > PBKDF2_ITERATIONS || !salt || !key) {
    return false;
  }
  const got = await derive(password, fromB64url(salt), iterations);
  return timingSafeEqualString(b64url(got), key);
}
