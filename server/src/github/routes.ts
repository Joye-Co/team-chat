// A person connects their own GitHub (docs/protocol.md, "GitHub sign-in"): the server starts
// GitHub's device code flow and polls it, so the token stays on the server; the app only shows
// the code. The token never goes into a response, a log line or an audit detail here.
import type { GithubDeviceResponse, GithubPollResponse, GithubStatusResponse } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { logger } from "../core/logger";
import { registerRoute } from "../core/registry";
import { fetchUser, type GithubEnv, pollAccessToken, startDeviceCode } from "./api";
import { connectedHooks, loadWaits } from "./hooks";
import {
  clientId,
  decryptDeviceCode,
  encryptDeviceCode,
  encryptToken,
  loadAccount,
  pushStatus,
  toAccount,
  unavailableReason,
} from "./store";

/** A device code sign-in lasts at most this long. */
const DEVICE_TTL_MS = 15 * 60_000;

type DeviceRow = { id: string; member_id: string; device_enc: string; interval_s: number; expires_at: number };

function person(auth: AuthContext | null): AuthContext {
  const a = auth as AuthContext;
  if (a.member.kind !== "human") throw new ApiError("forbidden", "only people connect GitHub", "github.people_only");
  return a;
}

/** The client id when the server can connect GitHub; otherwise the reason, as an error. */
function requireAvailable(env: GithubEnv): string {
  const reason = unavailableReason(env);
  if (reason) throw new ApiError("conflict", "GitHub sign-in is not set up on this server", `github.${reason}`);
  return clientId(env) as string;
}

/** True when a UNIQUE constraint on `github_user_id` refused the row. */
function isTaken(err: unknown): boolean {
  return /UNIQUE/i.test(String((err as Error)?.message ?? err)) && /github_user_id/i.test(String((err as Error)?.message ?? err));
}

export function registerGithubRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/me/github",
    auth: "member",
    async handler({ env, auth }): Promise<GithubStatusResponse> {
      const a = person(auth);
      const reason = unavailableReason(env);
      const row = await loadAccount(env.DB, a.memberId);
      const out: GithubStatusResponse = { available: reason === null, waits: await loadWaits(env, a.memberId) };
      if (reason) out.unavailableReason = reason;
      if (row) out.account = toAccount(row);
      return out;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/me/github/device",
    auth: "member",
    async handler({ env, auth }): Promise<GithubDeviceResponse> {
      const a = person(auth);
      const client = requireAvailable(env);
      const code = await startDeviceCode(env, client);
      const now = Date.now();
      const id = ulid(now);
      const interval = code.interval ?? 5;
      const expiresAt = now + Math.min(code.expires_in * 1000, DEVICE_TTL_MS);
      // One sign-in in progress per person; expired ones from anybody go too.
      await env.DB.batch([
        env.DB.prepare("DELETE FROM github_device WHERE member_id = ? OR expires_at < ?").bind(a.memberId, now),
        env.DB.prepare("INSERT INTO github_device (id, member_id, device_enc, interval_s, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)").bind(
          id,
          a.memberId,
          await encryptDeviceCode(env, id, code.device_code),
          interval,
          expiresAt,
          now,
        ),
      ]);
      return { deviceId: id, userCode: code.user_code, verificationUri: code.verification_uri, interval, expiresAt };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/me/github/device/:id/poll",
    auth: "member",
    async handler({ env, auth, params }): Promise<GithubPollResponse> {
      const a = person(auth);
      const client = requireAvailable(env);
      const row = await env.DB.prepare("SELECT id, member_id, device_enc, interval_s, expires_at FROM github_device WHERE id = ?")
        .bind(params.id)
        .first<DeviceRow>();
      // Somebody else's sign-in reads as not found.
      if (!row || row.member_id !== a.memberId) throw new ApiError("not_found", "no such GitHub sign-in");
      const drop = () => env.DB.prepare("DELETE FROM github_device WHERE id = ?").bind(row.id).run();
      if (row.expires_at <= Date.now()) {
        await drop();
        return { state: "expired" };
      }
      const answer = await pollAccessToken(env, client, await decryptDeviceCode(env, row.id, row.device_enc));
      switch (answer.kind) {
        case "pending":
          return { state: "pending" };
        case "slow_down":
          await env.DB.prepare("UPDATE github_device SET interval_s = ? WHERE id = ?")
            .bind(answer.interval ?? row.interval_s + 5, row.id)
            .run();
          return { state: "slow_down" };
        case "expired":
        case "denied":
          await drop();
          return { state: answer.kind };
      }

      const user = await fetchUser(env, answer.token);
      if (!user) {
        // A fresh token GitHub does not accept: treat it as a refused sign-in.
        await drop();
        return { state: "denied" };
      }
      const other = await env.DB.prepare("SELECT member_id FROM github_accounts WHERE github_user_id = ?")
        .bind(user.id)
        .first<{ member_id: string }>();
      if (other && other.member_id !== a.memberId) {
        await drop();
        return { state: "taken" };
      }

      const now = Date.now();
      const name = user.name?.trim() || null;
      try {
        await env.DB.batch([
          env.DB.prepare(
            `INSERT INTO github_accounts (member_id, github_user_id, login, name, token_enc, scopes, status, connected_at, checked_at)
             VALUES (?, ?, ?, ?, ?, ?, 'ok', ?, ?)
             ON CONFLICT (member_id) DO UPDATE SET github_user_id = excluded.github_user_id, login = excluded.login,
               name = excluded.name, token_enc = excluded.token_enc, scopes = excluded.scopes, status = 'ok',
               connected_at = excluded.connected_at, checked_at = excluded.checked_at`,
          ).bind(a.memberId, user.id, user.login, name, await encryptToken(env, a.memberId, answer.token), answer.scopes, now, now),
          env.DB.prepare("DELETE FROM github_device WHERE id = ?").bind(row.id),
          // Logins an owner set by hand win.
          env.DB.prepare("INSERT INTO github_logins (login, member_id, updated_at) VALUES (lower(?), ?, ?) ON CONFLICT (login) DO NOTHING").bind(
            user.login,
            a.memberId,
            now,
          ),
          auditStatement(env.DB, a, { action: "github.connect", target: a.memberId, details: { login: user.login, githubUserId: user.id } }, now),
        ]);
      } catch (err) {
        if (!isTaken(err)) throw err;
        await drop();
        return { state: "taken" };
      }

      const saved = await loadAccount(env.DB, a.memberId);
      const account = saved ? toAccount(saved) : { login: user.login, status: "ok", connectedAt: now };
      await pushStatus(env, a.memberId, account);
      for (const hook of connectedHooks) {
        try {
          await hook(env, a.memberId);
        } catch (err) {
          logger.error("github connected hook failed", { memberId: a.memberId, err });
        }
      }
      return { state: "done", account };
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/me/github",
    auth: "member",
    async handler({ env, auth }) {
      const a = person(auth);
      const before = await loadAccount(env.DB, a.memberId);
      await env.DB.batch([
        env.DB.prepare("DELETE FROM github_accounts WHERE member_id = ?").bind(a.memberId),
        env.DB.prepare("DELETE FROM github_device WHERE member_id = ?").bind(a.memberId),
        ...(before ? [auditStatement(env.DB, a, { action: "github.disconnect", target: a.memberId, details: { login: before.login } })] : []),
      ]);
      if (before) await pushStatus(env, a.memberId, undefined);
      return { ok: true };
    },
  });
}
