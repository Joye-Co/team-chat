import { sha256Hex } from "./crypto";
import { type MemberRow, toMember } from "./db";
import { ApiError } from "./errors";
import type { Member } from "@chat/protocol";

export interface AuthContext {
  memberId: string;
  tokenId: string;
  member: Member;
  row: MemberRow;
}

/** Updates `last_used_at` at most once per this many ms per token. */
const TOUCH_INTERVAL_MS = 60_000;

export function bearerToken(request: Request): string | null {
  const h = request.headers.get("authorization");
  if (!h) return null;
  const m = /^Bearer\s+(\S+)\s*$/i.exec(h);
  return m ? m[1] : null;
}

/**
 * Looks up a bearer token by its SHA-256 digest. Rejects missing, unknown and revoked
 * tokens and deactivated members with `unauthorized`.
 */
export async function authenticate(
  request: Request,
  db: D1Database,
  waitUntil?: (p: Promise<unknown>) => void,
): Promise<AuthContext> {
  const token = bearerToken(request);
  if (!token) throw new ApiError("unauthorized", "missing bearer token");
  return authenticateToken(token, db, waitUntil);
}

export async function authenticateToken(
  token: string,
  db: D1Database,
  waitUntil?: (p: Promise<unknown>) => void,
): Promise<AuthContext> {
  if (token.length > 200) throw new ApiError("unauthorized", "invalid token");
  const hash = await sha256Hex(token);
  const row = await db
    .prepare(
      `SELECT t.id AS token_id, t.revoked_at, t.last_used_at, m.*
       FROM tokens t JOIN members m ON m.id = t.member_id WHERE t.token_hash = ?`,
    )
    .bind(hash)
    .first<MemberRow & { token_id: string; revoked_at: number | null; last_used_at: number | null }>();
  if (!row || row.revoked_at !== null) throw new ApiError("unauthorized", "invalid token");
  if (row.deactivated) throw new ApiError("unauthorized", "member is deactivated");
  const now = Date.now();
  if (!row.last_used_at || now - row.last_used_at > TOUCH_INTERVAL_MS) {
    const p = db.prepare("UPDATE tokens SET last_used_at = ? WHERE id = ?").bind(now, row.token_id).run();
    if (waitUntil) waitUntil(p.catch(() => {}));
    else await p;
  }
  const { token_id, revoked_at: _r, last_used_at: _l, ...memberRow } = row;
  return { memberId: row.id, tokenId: token_id, member: toMember(memberRow), row: memberRow };
}

export function requireOwner(auth: AuthContext): void {
  if (auth.member.level !== "owner") throw new ApiError("forbidden", "owner only");
}
