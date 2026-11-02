import type { ErrorCode } from "@chat/protocol";

/** Error codes of the protocol, plus the SDK's own: timeout, closed (the bot stopped), network. */
export type ChatErrorCode = ErrorCode | "timeout" | "closed" | "network";

/** Every failure the SDK reports. `status` is the HTTP status for HTTP calls. */
export class ChatError extends Error {
  readonly code: ChatErrorCode;
  readonly status?: number;

  constructor(code: ChatErrorCode, message: string, status?: number) {
    super(`${code}: ${message}`);
    this.name = "ChatError";
    this.code = code;
    this.status = status;
  }
}

/** Errors that a second try can fix. */
export function isRetryable(err: unknown): boolean {
  if (!(err instanceof ChatError)) return false;
  return err.code === "network" || err.code === "rate_limited" || err.code === "internal" || err.code === "timeout";
}
