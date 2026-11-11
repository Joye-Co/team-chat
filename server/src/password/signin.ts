// Shared by password and code sign-in: one more token for a member, audited in the same batch.
import type { RedeemResponse } from "@chat/protocol";
import { type AuditEntry, auditStatement } from "../core/audit";
import { newToken, sha256Hex } from "../core/crypto";
import { type MemberRow, getMember, toMember } from "../core/db";
import type { Env } from "../core/env";
import { ulid } from "../core/ids";

export async function ipKey(request: Request): Promise<string> {
  return sha256Hex(request.headers.get("cf-connecting-ip") ?? "unknown");
}

/** Makes a token named `deviceName` for `memberId`; the audit entry's actor is the new token. */
export async function signInAs(env: Env, memberId: string, deviceName: string, entry: AuditEntry): Promise<RedeemResponse> {
  const now = Date.now();
  const tokenId = ulid(now);
  const token = newToken();
  await env.DB.batch([
    env.DB.prepare("INSERT INTO tokens (id, token_hash, member_id, label, created_at) VALUES (?, ?, ?, ?, ?)").bind(
      tokenId,
      await sha256Hex(token),
      memberId,
      deviceName,
      now,
    ),
    auditStatement(env.DB, { memberId, tokenId }, entry, now),
  ]);
  const row = (await getMember(env.DB, memberId)) as MemberRow;
  return { token, member: toMember(row) };
}
