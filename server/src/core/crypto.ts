import { CROCKFORD } from "./ids";

const encoder = new TextEncoder();

export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function base64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** `tc_` + 43 base64url characters (32 random bytes). */
export function newToken(): string {
  return `tc_${base64url(crypto.getRandomValues(new Uint8Array(32)))}`;
}

export const TOKEN_PATTERN = /^tc_[A-Za-z0-9_-]{43}$/;

/** 12 Crockford characters shown as XXXX-XXXX-XXXX. */
export function newInviteCode(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(12));
  let raw = "";
  for (const b of bytes) raw += CROCKFORD[b & 31];
  return `${raw.slice(0, 4)}-${raw.slice(4, 8)}-${raw.slice(8, 12)}`;
}

/** Crockford decoding rules: case-insensitive, I/L -> 1, O -> 0, dashes and spaces ignored. */
export function normalizeInviteCode(code: string): string {
  return code
    .toUpperCase()
    .replace(/[\s-]/g, "")
    .replace(/[IL]/g, "1")
    .replace(/O/g, "0");
}

/** Constant-time string comparison (compares SHA-256 digests so lengths do not leak). */
export async function timingSafeEqualString(a: string, b: string): Promise<boolean> {
  const [da, db] = await Promise.all([
    crypto.subtle.digest("SHA-256", encoder.encode(a)),
    crypto.subtle.digest("SHA-256", encoder.encode(b)),
  ]);
  const subtle = crypto.subtle as SubtleCrypto & {
    timingSafeEqual?: (a: ArrayBuffer, b: ArrayBuffer) => boolean;
  };
  if (typeof subtle.timingSafeEqual === "function") return subtle.timingSafeEqual(da, db);
  const x = new Uint8Array(da);
  const y = new Uint8Array(db);
  let diff = 0;
  for (let i = 0; i < x.length; i++) diff |= x[i] ^ y[i];
  return diff === 0;
}
