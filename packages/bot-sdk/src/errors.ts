import type { ErrorCode } from "@chat/protocol";

/** Error codes of the protocol, plus the SDK's own: timeout, closed (the bot stopped), network. */
export type ChatErrorCode = ErrorCode | "timeout" | "closed" | "network";

/** The optional fields of the server's error body that a `ChatError` keeps. */
export interface ChatErrorDetails {
  retryAfterMs?: number;
  memberIds?: string[];
}

/** Every failure the SDK reports. `status` is the HTTP status for HTTP calls. */
export class ChatError extends Error {
  readonly code: ChatErrorCode;
  readonly status?: number;
  /** 1.1: the specific reason from `ack.error.reason`, for example `card.stale_rev`. */
  readonly reason?: string;
  /** With `rate_limited`, when the server says it: how long to wait before the next try. */
  readonly retryAfterMs?: number;
  /** With reason `relay.member_unavailable`: the members that cannot take the message now. */
  readonly memberIds?: string[];

  constructor(code: ChatErrorCode, message: string, status?: number, reason?: string, details: ChatErrorDetails = {}) {
    super(`${code}: ${message}`);
    this.name = "ChatError";
    this.code = code;
    this.status = status;
    this.reason = reason;
    if (details.retryAfterMs !== undefined) this.retryAfterMs = details.retryAfterMs;
    if (details.memberIds !== undefined) this.memberIds = details.memberIds;
  }
}

/** `retryAfterMs` and `memberIds` of a server error body (`ack.error` or the HTTP `error`), when well formed. */
export function errorDetails(e: unknown): ChatErrorDetails {
  const o = (e && typeof e === "object" ? e : {}) as { retryAfterMs?: unknown; memberIds?: unknown };
  const out: ChatErrorDetails = {};
  if (typeof o.retryAfterMs === "number" && Number.isFinite(o.retryAfterMs) && o.retryAfterMs >= 0) out.retryAfterMs = o.retryAfterMs;
  if (Array.isArray(o.memberIds)) out.memberIds = o.memberIds.filter((x): x is string => typeof x === "string");
  return out;
}

/** Errors that a second try can fix. After `rate_limited`, wait `err.retryAfterMs` first when it is set. */
export function isRetryable(err: unknown): boolean {
  if (!(err instanceof ChatError)) return false;
  return err.code === "network" || err.code === "rate_limited" || err.code === "internal" || err.code === "timeout";
}
