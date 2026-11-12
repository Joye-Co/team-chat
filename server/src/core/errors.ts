import { type ErrorBody, type ErrorCode, issueReason } from "@chat/protocol";

/** An expected failure with a protocol error code. Safe to show to the client. */
export class ApiError extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
    /** 1.1: the specific rule (`ErrorBody.reason`), e.g. `card.already_answered`. */
    readonly reason?: string,
    /** 1.1, `rate_limited`: `ErrorBody.retryAfterMs`. */
    readonly retryAfterMs?: number,
    /** Usable: `ErrorBody.memberIds`, the members the reason is about (e.g. `relay.member_unavailable`). */
    readonly memberIds?: string[],
  ) {
    super(message);
  }
  toBody(): ErrorBody {
    const body: ErrorBody = this.reason ? { code: this.code, message: this.message, reason: this.reason } : { code: this.code, message: this.message };
    if (this.retryAfterMs !== undefined) body.retryAfterMs = Math.max(0, Math.ceil(this.retryAfterMs));
    if (this.memberIds && this.memberIds.length > 0) body.memberIds = this.memberIds.slice(0, 20);
    return body;
  }
}

export const STATUS: Record<ErrorCode, number> = {
  unauthorized: 401,
  forbidden: 403,
  not_found: 404,
  invalid: 400,
  conflict: 409,
  rate_limited: 429,
  too_large: 413,
  unsupported_protocol: 400,
  internal: 500,
};

/** Result type used across Durable Object RPC (thrown errors lose their class over RPC). */
export type Result<T> = { ok: true; value: T } | { ok: false; error: ErrorBody };

export function toErrorBody(err: unknown): ErrorBody {
  if (err instanceof ApiError) return err.toBody();
  return { code: "internal", message: "internal error" };
}

export function unwrap<T>(r: Result<T>): T {
  if (r.ok) return r.value;
  throw new ApiError(r.error.code, r.error.message, r.error.reason, undefined, r.error.memberIds);
}

/** The `reason` of a frame that failed its schema: a named card rule, or a deferred secret input. */
export function schemaReason(raw: Record<string, unknown>, message: string | undefined, path?: readonly PropertyKey[]): string | undefined {
  const named = issueReason(message);
  if (named) return named;
  const blocks: unknown[] = [];
  const cardV2 = raw.cardV2 as { blocks?: unknown } | undefined;
  if (cardV2 && Array.isArray(cardV2.blocks)) blocks.push(...cardV2.blocks);
  if (Array.isArray(raw.ops)) for (const op of raw.ops) if (op && typeof op === "object") blocks.push((op as { block?: unknown }).block);
  const secret = (b: unknown): boolean => {
    if (!b || typeof b !== "object") return false;
    const x = b as { type?: unknown; kind?: unknown; blocks?: unknown };
    if (x.type === "input" && x.kind === "secret") return true;
    return Array.isArray(x.blocks) && x.blocks.some(secret);
  };
  if (blocks.some(secret)) return "card.secret_deferred";
  // Any other shape problem of a card or of patch ops is `card.limits`, as the Rust pre-check
  // (`CardV2::problem`) names it.
  if (path && path.length > 0 && (path[0] === "cardV2" || (path[0] === "ops" && Array.isArray(raw.ops)))) return "card.limits";
  return undefined;
}
