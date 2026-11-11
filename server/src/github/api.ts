// The only place the server talks to GitHub for sign-in: the device code flow and `GET /user`.
// Bases come from `GITHUB_BASE` / `GITHUB_API_BASE` so tests and e2e point them at a fake GitHub.
// Nothing GitHub answers (it may hold a token) goes into an error message or a log line.
import { z } from "zod";
import product from "../../../product.json";
import { ApiError } from "../core/errors";
import type { HooksEnv } from "../integrations/secrets";

export type GithubEnv = HooksEnv & {
  /** Overrides product.json `githubClientId` (an OAuth App with Device Flow on). */
  GITHUB_CLIENT_ID?: string;
  /** Default `https://github.com`. */
  GITHUB_BASE?: string;
  /** Default `https://api.github.com`. */
  GITHUB_API_BASE?: string;
};

/** What the server asks for: push code, open pull requests, common `gh` commands, workflow files. */
export const GITHUB_SCOPES = "repo read:org workflow";

const USER_AGENT = `${product.slug}-server`;

function base(env: GithubEnv): string {
  return (env.GITHUB_BASE || "https://github.com").replace(/\/+$/, "");
}

function apiBase(env: GithubEnv): string {
  return (env.GITHUB_API_BASE || "https://api.github.com").replace(/\/+$/, "");
}

function unreachable(): ApiError {
  return new ApiError("internal", "GitHub did not answer as expected", "github.unreachable");
}

async function json(url: string, init: RequestInit): Promise<{ status: number; body: unknown }> {
  let res: Response;
  try {
    res = await fetch(url, init);
  } catch {
    throw unreachable();
  }
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    // not JSON: the caller sees null
  }
  return { status: res.status, body };
}

const DeviceCode = z.object({
  device_code: z.string().min(1).max(512),
  user_code: z.string().min(1).max(32),
  verification_uri: z.string().min(1).max(2048),
  expires_in: z.number().int().positive(),
  interval: z.number().int().positive().optional(),
});
export type DeviceCode = z.infer<typeof DeviceCode>;

/** `POST /login/device/code`. */
export async function startDeviceCode(env: GithubEnv, clientId: string): Promise<DeviceCode> {
  const r = await json(`${base(env)}/login/device/code`, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/json", "user-agent": USER_AGENT },
    body: JSON.stringify({ client_id: clientId, scope: GITHUB_SCOPES }),
  });
  const parsed = DeviceCode.safeParse(r.body);
  if (r.status !== 200 || !parsed.success) throw unreachable();
  return parsed.data;
}

export type TokenAnswer =
  | { kind: "token"; token: string; scopes: string }
  | { kind: "pending" }
  | { kind: "slow_down"; interval?: number }
  | { kind: "expired" }
  | { kind: "denied" };

const TokenBody = z.object({
  access_token: z.string().min(1).max(1024).optional(),
  scope: z.string().max(1024).optional(),
  error: z.string().max(128).optional(),
  interval: z.number().int().positive().optional(),
});

/** `POST /login/oauth/access_token` with the device code grant. GitHub answers errors with 200. */
export async function pollAccessToken(env: GithubEnv, clientId: string, deviceCode: string): Promise<TokenAnswer> {
  const r = await json(`${base(env)}/login/oauth/access_token`, {
    method: "POST",
    headers: { accept: "application/json", "content-type": "application/json", "user-agent": USER_AGENT },
    body: JSON.stringify({ client_id: clientId, device_code: deviceCode, grant_type: "urn:ietf:params:oauth:grant-type:device_code" }),
  });
  const parsed = TokenBody.safeParse(r.body);
  if (!parsed.success) throw unreachable();
  const b = parsed.data;
  if (b.access_token) return { kind: "token", token: b.access_token, scopes: b.scope ?? "" };
  switch (b.error) {
    case "authorization_pending":
      return { kind: "pending" };
    case "slow_down":
      return { kind: "slow_down", interval: b.interval };
    case "expired_token":
    case "incorrect_device_code":
      return { kind: "expired" };
    case "access_denied":
      return { kind: "denied" };
    default:
      // device_flow_disabled, incorrect_client_credentials, unsupported_grant_type, or no answer.
      throw new ApiError("conflict", "GitHub refused the sign-in for this server", "github.refused");
  }
}

const GithubUser = z.object({
  id: z.number().int().positive(),
  login: z.string().min(1).max(100),
  name: z.string().max(255).nullish(),
});
export type GithubUser = z.infer<typeof GithubUser>;

/** `GET /user`; null when GitHub answers 401 (the token no longer works). */
export async function fetchUser(env: GithubEnv, token: string): Promise<GithubUser | null> {
  const r = await json(`${apiBase(env)}/user`, {
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "user-agent": USER_AGENT,
      "x-github-api-version": "2022-11-28",
    },
  });
  if (r.status === 401) return null;
  const parsed = GithubUser.safeParse(r.body);
  if (r.status !== 200 || !parsed.success) throw unreachable();
  return parsed.data;
}
