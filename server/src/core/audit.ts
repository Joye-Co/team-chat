import type { AuthContext } from "./auth";

export interface AuditEntry {
  action: string;
  target?: string | null;
  details?: Record<string, unknown>;
}

/** A prepared audit insert, so it can join the batch (transaction) of the write it records. */
export function auditStatement(
  db: D1Database,
  actor: Pick<AuthContext, "memberId" | "tokenId"> | null,
  entry: AuditEntry,
  at = Date.now(),
): D1PreparedStatement {
  return db
    .prepare("INSERT INTO audit (actor_id, credential_id, action, target, details, at) VALUES (?, ?, ?, ?, ?, ?)")
    .bind(
      actor?.memberId ?? null,
      actor?.tokenId ?? null,
      entry.action,
      entry.target ?? null,
      JSON.stringify(entry.details ?? {}),
      at,
    );
}

export async function audit(
  db: D1Database,
  actor: Pick<AuthContext, "memberId" | "tokenId"> | null,
  entry: AuditEntry,
): Promise<void> {
  await auditStatement(db, actor, entry).run();
}
