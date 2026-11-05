// Email sign-in (docs/protocol.md, "Email sign-in"): a member adds a sign-in email once (a code
// confirms it), then signs in on any Mac with a 6-digit code sent there. Registered from
// src/features.ts. Codes are stored only as SHA-256 digests and are never logged.
import {
  EmailCodeRequest,
  EmailSignInRequest,
  type EmailCodeResponse,
  type MyEmailResponse,
  type RedeemResponse,
  VerifyEmailRequest,
} from "@chat/protocol";
import product from "../../../product.json";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { sha256Hex, newToken, timingSafeEqualString } from "../core/crypto";
import { type MemberRow, getMember, toMember } from "../core/db";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { refundAttempt, takeAttempt } from "../core/ratelimit";
import { registerRoute } from "../core/registry";

/** Cloudflare Email Service binding (`send_email`), only the part used here. */
interface EmailBinding {
  send(message: { from: { email: string; name?: string } | string; to: string; subject: string; text: string; html?: string }): Promise<unknown>;
}
type EmailEnv = Env & { EMAIL?: EmailBinding; EMAIL_FROM?: string };

export const EMAIL_CODE_TTL_MS = 10 * 60 * 1000;
export const EMAIL_CODE_MAX_TRIES = 5;
const SEND_PER_ADDRESS = { windowMs: 3600_000, max: 5 };
const SEND_PER_IP = { windowMs: 3600_000, max: 20 };
const CHECK_PER_IP = { windowMs: 60_000, max: 10 };

type Purpose = "sign_in" | "add";

/** True where codes go to the test outbox instead of the mail. */
function usesOutbox(env: Env): boolean {
  return env.ENVIRONMENT === "local" || env.ENVIRONMENT === "e2e";
}

function canSend(env: EmailEnv): boolean {
  return usesOutbox(env) || (!!env.EMAIL && !!env.EMAIL_FROM);
}

function requireMail(env: EmailEnv): void {
  if (!canSend(env)) throw new ApiError("invalid", "email sign-in is not set up on this server", "email.not_configured");
}

/** 6 random digits, uniform. */
function newEmailCode(): string {
  const buf = new Uint32Array(1);
  const limit = Math.floor(0xffffffff / 1_000_000) * 1_000_000;
  for (;;) {
    crypto.getRandomValues(buf);
    if (buf[0] < limit) return String(buf[0] % 1_000_000).padStart(6, "0");
  }
}

async function ipKey(request: Request): Promise<string> {
  return sha256Hex(request.headers.get("cf-connecting-ip") ?? "unknown");
}

/** Saves a new code (replacing an older one for the same address and purpose) and sends it. */
async function issueCode(env: EmailEnv, email: string, purpose: Purpose, memberId: string, now: number): Promise<number> {
  const code = newEmailCode();
  const expiresAt = now + EMAIL_CODE_TTL_MS;
  await env.DB.prepare(
    `INSERT INTO email_codes (email, purpose, member_id, code_hash, expires_at, attempts, created_at)
     VALUES (?, ?, ?, ?, ?, 0, ?)
     ON CONFLICT (email, purpose, member_id) DO UPDATE SET code_hash = excluded.code_hash,
       expires_at = excluded.expires_at, attempts = 0, created_at = excluded.created_at`,
  )
    .bind(email, purpose, memberId, await sha256Hex(code), expiresAt, now)
    .run();
  if (usesOutbox(env)) {
    await env.DB.prepare("INSERT INTO email_outbox (email, purpose, code, at) VALUES (?, ?, ?, ?)")
      .bind(email, purpose, code, now)
      .run();
    return expiresAt;
  }
  const lead = purpose === "sign_in" ? `你正在登录 ${product.name}。` : `你正在把这个邮箱设为 ${product.name} 的登录邮箱。`;
  const text = `${lead}\n\n验证码：${code}\n\n10 分钟内有效，只能用一次。如果不是你本人操作，请忽略这封邮件。`;
  await env.EMAIL!.send({
    from: { email: env.EMAIL_FROM!, name: product.name },
    to: email,
    subject: `${product.name} 验证码 ${code}`,
    text,
  });
  return expiresAt;
}

/**
 * Checks a code and uses it up. A wrong code counts a try; after EMAIL_CODE_MAX_TRIES the code
 * stops working. Throws `unauthorized` for a wrong, used or expired code.
 */
async function takeCode(env: Env, email: string, purpose: Purpose, memberId: string, code: string, now: number): Promise<void> {
  const row = await env.DB.prepare(
    "SELECT code_hash, expires_at, attempts FROM email_codes WHERE email = ? AND purpose = ? AND member_id = ?",
  )
    .bind(email, purpose, memberId)
    .first<{ code_hash: string; expires_at: number; attempts: number }>();
  const refused = new ApiError("unauthorized", "the code is wrong, used or expired");
  if (!row || row.expires_at <= now || row.attempts >= EMAIL_CODE_MAX_TRIES) throw refused;
  if (!(await timingSafeEqualString(await sha256Hex(code), row.code_hash))) {
    await env.DB.prepare("UPDATE email_codes SET attempts = attempts + 1 WHERE email = ? AND purpose = ? AND member_id = ?")
      .bind(email, purpose, memberId)
      .run();
    throw refused;
  }
  const used = await env.DB.prepare(
    "DELETE FROM email_codes WHERE email = ? AND purpose = ? AND member_id = ? AND code_hash = ?",
  )
    .bind(email, purpose, memberId, row.code_hash)
    .run();
  // Two tries with the same code at once: only one wins.
  if (!used.meta.changes) throw refused;
}

function humanOnly(a: AuthContext): void {
  if (a.member.kind !== "human") throw new ApiError("forbidden", "only people sign in with email");
}

async function myEmail(env: Env, memberId: string): Promise<string | null> {
  const row = await env.DB.prepare("SELECT email FROM member_emails WHERE member_id = ?")
    .bind(memberId)
    .first<{ email: string }>();
  return row?.email ?? null;
}

registerRoute({
  method: "POST",
  path: "/v1/auth/email/code",
  auth: "none",
  async handler({ env, body, request }): Promise<EmailCodeResponse> {
    const e = env as EmailEnv;
    const { email } = await body(EmailCodeRequest);
    requireMail(e);
    await takeAttempt(env.DB, `email-send-ip:${await ipKey(request)}`, SEND_PER_IP);
    await takeAttempt(env.DB, `email-send:${await sha256Hex(email)}`, SEND_PER_ADDRESS);
    const now = Date.now();
    const owner = await env.DB.prepare(
      `SELECT e.member_id FROM member_emails e JOIN members m ON m.id = e.member_id
       WHERE e.email = ? AND m.deactivated = 0 AND m.kind = 'human'`,
    )
      .bind(email)
      .first<{ member_id: string }>();
    // Same answer either way: the route never tells whether an address has an account.
    if (!owner) return { sent: true, expiresAt: now + EMAIL_CODE_TTL_MS };
    return { sent: true, expiresAt: await issueCode(e, email, "sign_in", "", now) };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/auth/email/sign-in",
  auth: "none",
  async handler({ env, body, request }): Promise<RedeemResponse> {
    const attempt = await takeAttempt(env.DB, `email-check-ip:${await ipKey(request)}`, CHECK_PER_IP);
    const { email, code, deviceName } = await body(EmailSignInRequest);
    const now = Date.now();
    await takeCode(env, email, "sign_in", "", code, now);
    const owner = await env.DB.prepare(
      `SELECT e.member_id FROM member_emails e JOIN members m ON m.id = e.member_id
       WHERE e.email = ? AND m.deactivated = 0 AND m.kind = 'human'`,
    )
      .bind(email)
      .first<{ member_id: string }>();
    if (!owner) throw new ApiError("unauthorized", "the code is wrong, used or expired");
    const tokenId = ulid(now);
    const token = newToken();
    await env.DB.batch([
      env.DB.prepare("INSERT INTO tokens (id, token_hash, member_id, label, created_at) VALUES (?, ?, ?, ?, ?)").bind(
        tokenId,
        await sha256Hex(token),
        owner.member_id,
        deviceName,
        now,
      ),
      auditStatement(env.DB, { memberId: owner.member_id, tokenId }, { action: "email.sign_in", target: owner.member_id }, now),
    ]);
    await refundAttempt(env.DB, attempt);
    const row = (await getMember(env.DB, owner.member_id)) as MemberRow;
    return { token, member: toMember(row) };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/me/email",
  auth: "member",
  async handler({ env, auth }): Promise<MyEmailResponse> {
    return { email: await myEmail(env, (auth as AuthContext).memberId) };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/me/email/code",
  auth: "member",
  async handler({ env, auth, body }): Promise<EmailCodeResponse> {
    const a = auth as AuthContext;
    humanOnly(a);
    const e = env as EmailEnv;
    const { email } = await body(EmailCodeRequest);
    requireMail(e);
    await takeAttempt(env.DB, `email-send:${await sha256Hex(email)}`, SEND_PER_ADDRESS);
    const taken = await env.DB.prepare("SELECT member_id FROM member_emails WHERE email = ? AND member_id != ?")
      .bind(email, a.memberId)
      .first();
    if (taken) throw new ApiError("conflict", "another member signs in with this email");
    return { sent: true, expiresAt: await issueCode(e, email, "add", a.memberId, Date.now()) };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/me/email",
  auth: "member",
  async handler({ env, auth, body, request }): Promise<MyEmailResponse> {
    const a = auth as AuthContext;
    humanOnly(a);
    const attempt = await takeAttempt(env.DB, `email-check-ip:${await ipKey(request)}`, CHECK_PER_IP);
    const { email, code } = await body(VerifyEmailRequest);
    const now = Date.now();
    await takeCode(env, email, "add", a.memberId, code, now);
    try {
      await env.DB.batch([
        env.DB.prepare("DELETE FROM member_emails WHERE member_id = ?").bind(a.memberId),
        env.DB.prepare("INSERT INTO member_emails (member_id, email, verified_at) VALUES (?, ?, ?)").bind(a.memberId, email, now),
        auditStatement(env.DB, a, { action: "email.set", target: a.memberId }, now),
      ]);
    } catch (err) {
      if (String(err).includes("UNIQUE")) throw new ApiError("conflict", "another member signs in with this email");
      throw err;
    }
    await refundAttempt(env.DB, attempt);
    return { email };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/me/email",
  auth: "member",
  async handler({ env, auth }): Promise<MyEmailResponse> {
    const a = auth as AuthContext;
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare("DELETE FROM member_emails WHERE member_id = ?").bind(a.memberId),
      auditStatement(env.DB, a, { action: "email.remove", target: a.memberId }, now),
    ]);
    return { email: null };
  },
});

// Tests read the codes of local and e2e from here; elsewhere the outbox stays empty.
registerRoute({
  method: "GET",
  path: "/v1/admin/email-outbox",
  auth: "none",
  async handler({ env, request }) {
    const given = request.headers.get("x-admin-secret") ?? "";
    const expected = env.ADMIN_SECRET ?? "";
    if (!usesOutbox(env) || !expected || !(await timingSafeEqualString(given, expected))) {
      throw new ApiError("unauthorized", "invalid admin secret");
    }
    const email = (new URL(request.url).searchParams.get("email") ?? "").trim().toLowerCase();
    const r = await env.DB.prepare("SELECT purpose, code, at FROM email_outbox WHERE email = ? ORDER BY at DESC LIMIT 20")
      .bind(email)
      .all<{ purpose: string; code: string; at: number }>();
    return { codes: r.results };
  },
});
