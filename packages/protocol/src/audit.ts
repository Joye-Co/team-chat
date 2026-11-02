// V1 audit log. See docs/protocol.md, "Audit".
import { z } from "zod";
import { Id, Millis } from "./common.ts";

export const MAX_AUDIT_LIMIT = 200;

export const CredentialKind = z.enum(["token", "admin", "hook", "invite"]);
export type CredentialKind = z.infer<typeof CredentialKind>;

/** One audited write. Never holds a token, secret or invite code. */
export const AuditEntry = z.object({
  id: Id,
  at: Millis,
  /** The member whose credential made the write (null for the admin secret). */
  actorId: Id.nullable(),
  /** The member who asked for it, when a bot acted for someone. */
  requestedBy: Id.nullable().optional(),
  credential: z.object({
    kind: CredentialKind,
    /** Token, hook or invite record id. Never the secret or its digest. */
    id: Id.optional(),
  }),
  /** Dotted action name, such as `member.update` or `hook.create`. */
  action: z.string().min(1).max(64),
  target: z.object({ kind: z.string().min(1).max(32), id: Id }).optional(),
  groupId: Id.optional(),
  /** What changed, for example `{ level: ["member", "owner"] }`. Secrets are removed. */
  details: z.record(z.string(), z.json()).optional(),
});
export type AuditEntry = z.infer<typeof AuditEntry>;

/** `GET /v1/audit?limit=&before=`: newest first; `before` is an entry id. */
export const AuditResponse = z.object({
  entries: z.array(AuditEntry),
  hasMore: z.boolean(),
});
export type AuditResponse = z.infer<typeof AuditResponse>;
