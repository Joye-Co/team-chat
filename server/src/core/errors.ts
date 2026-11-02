import type { ErrorBody, ErrorCode } from "@chat/protocol";

/** An expected failure with a protocol error code. Safe to show to the client. */
export class ApiError extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
  }
  toBody(): ErrorBody {
    return { code: this.code, message: this.message };
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
  throw new ApiError(r.error.code, r.error.message);
}
