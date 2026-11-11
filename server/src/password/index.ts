// Password sign-in (docs/protocol.md, "Password sign-in"): an optional password per person,
// stored only as a PBKDF2 hash, used with the sign-in email. Registered from src/features.ts.
// Passwords are never logged.
import {
  MyPasswordResponse,
  PasswordSignInRequest,
  type RedeemResponse,
  RemovePasswordRequest,
  SetPasswordRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { sha256Hex } from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { refundAttempt, takeAttempt } from "../core/ratelimit";
import { registerRoute } from "../core/registry";
import { hashPassword, verifyPassword } from "./hash";
import { ipKey, signInAs } from "./signin";

const CHECK_PER_IP = { windowMs: 60_000, max: 10 };
const WRONG_PER_ADDRESS = { windowMs: 3600_000, max: 10 };
/** A token this young may change the password without the current one. */
export const FRESH_SIGN_IN_MS = 10 * 60_000;

/** Hashed when an address has no password, so the answer time does not tell. */
let dummyHash: Promise<string> | null = null;

function humanOnly(a: AuthContext): void {
  if (a.member.kind !== "human") throw new ApiError("forbidden", "only people have passwords");
}

async function storedHash(env: Env, memberId: string): Promise<{ hash: string; updated_at: number } | null> {
  return env.DB.prepare("SELECT hash, updated_at FROM member_passwords WHERE member_id = ?")
    .bind(memberId)
    .first<{ hash: string; updated_at: number }>();
}

async function mine(env: Env, memberId: string): Promise<MyPasswordResponse> {
  const row = await storedHash(env, memberId);
  return { set: !!row, updatedAt: row?.updated_at ?? null };
}

/**
 * Checks `currentPassword` before a change: needed when one is set, unless this token signed
 * in within FRESH_SIGN_IN_MS.
 */
async function checkCurrent(env: Env, request: Request, a: AuthContext, current: string | undefined): Promise<void> {
  const row = await storedHash(env, a.memberId);
  if (!row) return;
  const token = await env.DB.prepare("SELECT created_at FROM tokens WHERE id = ?")
    .bind(a.tokenId)
    .first<{ created_at: number }>();
  if (token && Date.now() - token.created_at < FRESH_SIGN_IN_MS) return;
  const refused = new ApiError("invalid", "the current password is missing or wrong", "password.current");
  if (current === undefined) throw refused;
  const attempt = await takeAttempt(env.DB, `password-ip:${await ipKey(request)}`, CHECK_PER_IP);
  if (!(await verifyPassword(current, row.hash))) throw refused;
  await refundAttempt(env.DB, attempt);
}

registerRoute({
  method: "GET",
  path: "/v1/me/password",
  auth: "member",
  async handler({ env, auth }): Promise<MyPasswordResponse> {
    return mine(env, (auth as AuthContext).memberId);
  },
});

registerRoute({
  method: "PUT",
  path: "/v1/me/password",
  auth: "member",
  async handler({ env, auth, body, request }): Promise<MyPasswordResponse> {
    const a = auth as AuthContext;
    humanOnly(a);
    const { password, currentPassword } = await body(SetPasswordRequest);
    await checkCurrent(env, request, a, currentPassword);
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO member_passwords (member_id, hash, updated_at) VALUES (?, ?, ?)
         ON CONFLICT (member_id) DO UPDATE SET hash = excluded.hash, updated_at = excluded.updated_at`,
      ).bind(a.memberId, await hashPassword(password), now),
      auditStatement(env.DB, a, { action: "password.set", target: a.memberId }, now),
    ]);
    return { set: true, updatedAt: now };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/me/password/remove",
  auth: "member",
  async handler({ env, auth, body, request }): Promise<MyPasswordResponse> {
    const a = auth as AuthContext;
    humanOnly(a);
    const { currentPassword } = await body(RemovePasswordRequest);
    await checkCurrent(env, request, a, currentPassword);
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare("DELETE FROM member_passwords WHERE member_id = ?").bind(a.memberId),
      auditStatement(env.DB, a, { action: "password.remove", target: a.memberId }, now),
    ]);
    return { set: false, updatedAt: null };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/auth/password/sign-in",
  auth: "none",
  async handler({ env, body, request }): Promise<RedeemResponse> {
    const ipAttempt = await takeAttempt(env.DB, `password-ip:${await ipKey(request)}`, CHECK_PER_IP);
    const { email, password, deviceName } = await body(PasswordSignInRequest);
    const addressAttempt = await takeAttempt(env.DB, `password-address:${await sha256Hex(email)}`, WRONG_PER_ADDRESS);
    const owner = await env.DB.prepare(
      `SELECT e.member_id, p.hash FROM member_emails e
       JOIN members m ON m.id = e.member_id
       JOIN member_passwords p ON p.member_id = e.member_id
       WHERE e.email = ? AND m.deactivated = 0 AND m.kind = 'human'`,
    )
      .bind(email)
      .first<{ member_id: string; hash: string }>();
    const hash = owner?.hash ?? (await (dummyHash ??= hashPassword("not a password of anyone")));
    const ok = await verifyPassword(password, hash);
    if (!owner || !ok) throw new ApiError("unauthorized", "the email or password is wrong");
    const signed = await signInAs(env, owner.member_id, deviceName, { action: "password.sign_in", target: owner.member_id });
    await refundAttempt(env.DB, ipAttempt);
    await refundAttempt(env.DB, addressAttempt);
    return signed;
  },
});
