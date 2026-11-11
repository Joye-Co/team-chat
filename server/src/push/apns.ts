// Apple Push Notification service over HTTP/2 with token auth (an ES256 JWT signed by the
// operator's APNs key). The key and ids are Worker secrets; see docs/protocol.md, "Push".
import type { Env } from "../core/env";

export type PushEnv = Env & {
  /** The APNs auth key (.p8 file contents, PKCS#8 PEM). Secret. */
  APNS_KEY?: string;
  /** The key's 10-character id. */
  APNS_KEY_ID?: string;
  /** The Apple developer team id. */
  APNS_TEAM_ID?: string;
  /** Tests and local fakes only: replaces https://api.push.apple.com and the sandbox host. */
  APNS_ORIGIN?: string;
};

export function apnsConfigured(env: PushEnv): boolean {
  return !!env.APNS_KEY && !!env.APNS_KEY_ID && !!env.APNS_TEAM_ID;
}

export function apnsOrigin(env: PushEnv, environment: string): string {
  if (env.APNS_ORIGIN) return env.APNS_ORIGIN;
  return environment === "sandbox" ? "https://api.sandbox.push.apple.com" : "https://api.push.apple.com";
}

const b64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const b64urlText = (text: string) => b64url(new TextEncoder().encode(text));

function pemBytes(pem: string): Uint8Array {
  const body = pem.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
  return Uint8Array.from(atob(body), (c) => c.charCodeAt(0));
}

/** One JWT per key per isolate, renewed after 50 minutes (APNs takes 20–60 minute old tokens). */
let cached: { keyId: string; jwt: string; at: number } | null = null;
const JWT_TTL_MS = 50 * 60_000;

export async function apnsJwt(env: PushEnv, now = Date.now()): Promise<string> {
  const keyId = env.APNS_KEY_ID as string;
  if (cached && cached.keyId === keyId && now - cached.at < JWT_TTL_MS) return cached.jwt;
  const key = await crypto.subtle.importKey("pkcs8", pemBytes(env.APNS_KEY as string), { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
  const head = b64urlText(JSON.stringify({ alg: "ES256", kid: keyId }));
  const claims = b64urlText(JSON.stringify({ iss: env.APNS_TEAM_ID, iat: Math.floor(now / 1000) }));
  const sig = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, new TextEncoder().encode(`${head}.${claims}`));
  const jwt = `${head}.${claims}.${b64url(new Uint8Array(sig))}`;
  cached = { keyId, jwt, at: now };
  return jwt;
}

/** Tests: forget the cached JWT. */
export function resetApnsJwt(): void {
  cached = null;
}

export interface ApnsRequest {
  token: string;
  appId: string;
  environment: string;
  payload: Record<string, unknown>;
  /** 10 = now (alerts), 5 = when convenient (badge only). */
  priority: 10 | 5;
  collapseId?: string;
  /** Unix seconds after which APNs drops the push; 0 = try once. */
  expiration: number;
}

/** `gone`: the token is no longer valid; remove the device. `retry`: try again later. */
export type ApnsResult = "sent" | "gone" | "retry" | "failed";

export async function sendApns(env: PushEnv, r: ApnsRequest): Promise<{ result: ApnsResult; status: number; reason?: string }> {
  const headers: Record<string, string> = {
    authorization: `bearer ${await apnsJwt(env)}`,
    "apns-topic": r.appId,
    "apns-push-type": "alert",
    "apns-priority": String(r.priority),
    "apns-expiration": String(r.expiration),
    "content-type": "application/json",
  };
  if (r.collapseId) headers["apns-collapse-id"] = r.collapseId;
  let res: Response;
  try {
    res = await fetch(`${apnsOrigin(env, r.environment)}/3/device/${r.token}`, { method: "POST", headers, body: JSON.stringify(r.payload) });
  } catch {
    return { result: "retry", status: 0 };
  }
  if (res.status === 200) {
    await res.body?.cancel();
    return { result: "sent", status: 200 };
  }
  let reason: string | undefined;
  try {
    reason = ((await res.json()) as { reason?: string }).reason;
  } catch {
    // no JSON body
  }
  if (res.status === 410 || reason === "BadDeviceToken" || reason === "DeviceTokenNotForTopic" || reason === "Unregistered") {
    return { result: "gone", status: res.status, reason };
  }
  if (reason === "ExpiredProviderToken") resetApnsJwt();
  if (res.status === 429 || res.status >= 500 || reason === "ExpiredProviderToken") return { result: "retry", status: res.status, reason };
  return { result: "failed", status: res.status, reason };
}
