// Code sign-in, 扫码登录 (docs/protocol.md, "Code sign-in"): a signed-in device offers a one-time
// code a new device claims, or approves the code a new device shows. Codes and secrets are stored
// only as SHA-256 digests. Registered from src/features.ts.
import {
  QrClaimRequest,
  QrCodeRequest,
  type QrOfferResponse,
  type QrOfferStatus,
  type QrPeekResponse,
  QrPollRequest,
  type QrPollResponse,
  QrRequestRequest,
  type QrRequestResponse,
  type RedeemResponse,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { newInviteCode, newToken, normalizeInviteCode, sha256Hex, timingSafeEqualString } from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { refundAttempt, takeAttempt } from "../core/ratelimit";
import { registerRoute } from "../core/registry";
import { ipKey, signInAs } from "../password/signin";

export const QR_TTL_MS = 5 * 60_000;
const MAX_OPEN_OFFERS = 5;
const CODE_PER_IP = { windowMs: 60_000, max: 10 };
const REQUEST_PER_IP = { windowMs: 3600_000, max: 20 };
const POLL_PER_IP = { windowMs: 60_000, max: 120 };

type Row = {
  id: string;
  kind: "offer" | "request";
  member_id: string;
  device_name: string;
  secret_hash: string;
  state: "waiting" | "approved" | "used";
  expires_at: number;
};

function humanOnly(a: AuthContext): void {
  if (a.member.kind !== "human") throw new ApiError("forbidden", "only people sign devices in");
}

async function codeHash(code: string): Promise<string> {
  return sha256Hex(normalizeInviteCode(code));
}

/** The live row of `kind` for a typed code, or null. */
async function findByCode(env: Env, kind: Row["kind"], code: string, now: number): Promise<Row | null> {
  return env.DB.prepare(
    `SELECT id, kind, member_id, device_name, secret_hash, state, expires_at FROM qr_sign_ins
     WHERE code_hash = ? AND kind = ? AND state = 'waiting' AND expires_at > ?`,
  )
    .bind(await codeHash(code), kind, now)
    .first<Row>();
}

async function prune(env: Env, now: number): Promise<void> {
  if (Math.random() < 0.05) {
    await env.DB.prepare("DELETE FROM qr_sign_ins WHERE expires_at < ?").bind(now - 3600_000).run().catch(() => {});
  }
}

registerRoute({
  method: "POST",
  path: "/v1/me/qr/offer",
  auth: "member",
  async handler({ env, auth }): Promise<QrOfferResponse> {
    const a = auth as AuthContext;
    humanOnly(a);
    const now = Date.now();
    await prune(env, now);
    const id = ulid(now);
    const code = newInviteCode();
    const expiresAt = now + QR_TTL_MS;
    await env.DB.batch([
      // Keep at most MAX_OPEN_OFFERS open: the oldest beyond that end now.
      env.DB.prepare(
        `UPDATE qr_sign_ins SET expires_at = ? WHERE id IN (
           SELECT id FROM qr_sign_ins WHERE member_id = ? AND kind = 'offer' AND state = 'waiting' AND expires_at > ?
           ORDER BY id DESC LIMIT -1 OFFSET ?)`,
      ).bind(now, a.memberId, now, MAX_OPEN_OFFERS - 1),
      env.DB.prepare(
        `INSERT INTO qr_sign_ins (id, kind, code_hash, member_id, state, expires_at, created_at)
         VALUES (?, 'offer', ?, ?, 'waiting', ?, ?)`,
      ).bind(id, await codeHash(code), a.memberId, expiresAt, now),
      auditStatement(env.DB, a, { action: "qr.offer", target: a.memberId }, now),
    ]);
    return { id, code, expiresAt };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/me/qr/offers/:id",
  auth: "member",
  async handler({ env, auth, params }): Promise<QrOfferStatus> {
    const a = auth as AuthContext;
    const row = await env.DB.prepare(
      "SELECT state, expires_at, device_name FROM qr_sign_ins WHERE id = ? AND kind = 'offer' AND member_id = ?",
    )
      .bind(params.id, a.memberId)
      .first<{ state: string; expires_at: number; device_name: string }>();
    if (!row) throw new ApiError("not_found", "no such code");
    if (row.state === "used") return { state: "used", deviceName: row.device_name };
    return { state: row.expires_at > Date.now() ? "waiting" : "expired" };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/auth/qr/claim",
  auth: "none",
  async handler({ env, body, request }): Promise<RedeemResponse> {
    const attempt = await takeAttempt(env.DB, `qr-code-ip:${await ipKey(request)}`, CODE_PER_IP);
    const { code, deviceName } = await body(QrClaimRequest);
    const now = Date.now();
    const refused = new ApiError("unauthorized", "the code is wrong, used or expired");
    const row = await findByCode(env, "offer", code, now);
    if (!row) throw refused;
    const member = await env.DB.prepare("SELECT 1 FROM members WHERE id = ? AND deactivated = 0").bind(row.member_id).first();
    if (!member) throw refused;
    const used = await env.DB.prepare("UPDATE qr_sign_ins SET state = 'used', device_name = ? WHERE id = ? AND state = 'waiting'")
      .bind(deviceName, row.id)
      .run();
    // Two claims of the same code at once: only one wins.
    if (!used.meta.changes) throw refused;
    const signed = await signInAs(env, row.member_id, deviceName, {
      action: "qr.sign_in",
      target: row.member_id,
      details: { via: "offer" },
    });
    await refundAttempt(env.DB, attempt);
    return signed;
  },
});

registerRoute({
  method: "POST",
  path: "/v1/auth/qr/request",
  auth: "none",
  async handler({ env, body, request }): Promise<QrRequestResponse> {
    await takeAttempt(env.DB, `qr-request-ip:${await ipKey(request)}`, REQUEST_PER_IP);
    const { deviceName } = await body(QrRequestRequest);
    const now = Date.now();
    await prune(env, now);
    const id = ulid(now);
    const code = newInviteCode();
    const secret = newToken().slice(3);
    const expiresAt = now + QR_TTL_MS;
    await env.DB.prepare(
      `INSERT INTO qr_sign_ins (id, kind, code_hash, device_name, secret_hash, state, expires_at, created_at)
       VALUES (?, 'request', ?, ?, ?, 'waiting', ?, ?)`,
    )
      .bind(id, await codeHash(code), deviceName, await sha256Hex(secret), expiresAt, now)
      .run();
    return { id, code, secret, expiresAt };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/auth/qr/poll",
  auth: "none",
  async handler({ env, body, request }): Promise<QrPollResponse> {
    await takeAttempt(env.DB, `qr-poll-ip:${await ipKey(request)}`, POLL_PER_IP);
    const { id, secret } = await body(QrPollRequest);
    const now = Date.now();
    const row = await env.DB.prepare(
      `SELECT id, kind, member_id, device_name, secret_hash, state, expires_at FROM qr_sign_ins
       WHERE id = ? AND kind = 'request'`,
    )
      .bind(id)
      .first<Row>();
    if (!row || row.state === "used" || !(await timingSafeEqualString(await sha256Hex(secret), row.secret_hash))) {
      return { state: "expired" };
    }
    if (row.state === "waiting") return { state: row.expires_at > now ? "waiting" : "expired" };
    // Approved: make the token once.
    const taken = await env.DB.prepare("UPDATE qr_sign_ins SET state = 'used' WHERE id = ? AND state = 'approved'").bind(row.id).run();
    if (!taken.meta.changes) return { state: "expired" };
    const member = await env.DB.prepare("SELECT 1 FROM members WHERE id = ? AND deactivated = 0").bind(row.member_id).first();
    if (!member) return { state: "expired" };
    const signed = await signInAs(env, row.member_id, row.device_name, {
      action: "qr.sign_in",
      target: row.member_id,
      details: { via: "request" },
    });
    return { state: "approved", token: signed.token, member: signed.member };
  },
});

/** The request a signed-in person typed or scanned; counts a try against the IP. */
async function requestFor(env: Env, request: Request, code: string): Promise<{ row: Row; attempt: Awaited<ReturnType<typeof takeAttempt>> }> {
  const attempt = await takeAttempt(env.DB, `qr-code-ip:${await ipKey(request)}`, CODE_PER_IP);
  const row = await findByCode(env, "request", code, Date.now());
  if (!row) throw new ApiError("not_found", "the code is wrong, used or expired");
  return { row, attempt };
}

registerRoute({
  method: "POST",
  path: "/v1/me/qr/peek",
  auth: "member",
  async handler({ env, auth, body, request }): Promise<QrPeekResponse> {
    humanOnly(auth as AuthContext);
    const { code } = await body(QrCodeRequest);
    const { row, attempt } = await requestFor(env, request, code);
    await refundAttempt(env.DB, attempt);
    return { deviceName: row.device_name, expiresAt: row.expires_at };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/me/qr/approve",
  auth: "member",
  async handler({ env, auth, body, request }) {
    const a = auth as AuthContext;
    humanOnly(a);
    const { code } = await body(QrCodeRequest);
    const { row, attempt } = await requestFor(env, request, code);
    const now = Date.now();
    const done = await env.DB.prepare(
      "UPDATE qr_sign_ins SET state = 'approved', member_id = ? WHERE id = ? AND state = 'waiting' AND expires_at > ?",
    )
      .bind(a.memberId, row.id, now)
      .run();
    if (!done.meta.changes) throw new ApiError("not_found", "the code is wrong, used or expired");
    await auditStatement(env.DB, a, { action: "qr.approve", target: a.memberId, details: { deviceName: row.device_name } }, now).run();
    await refundAttempt(env.DB, attempt);
    return { ok: true as const };
  },
});
