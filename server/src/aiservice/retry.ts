// One retry policy for every call to the team's provider (usable 模 1.8), Anthropic and
// `/chat/completions` alike: a rate limit (429), a server error (5xx, Anthropic's 529 included) or
// a timeout or network error is tried again, twice, after a growing pause (or the provider's
// `retry-after`, up to a cap). Anything else (a bad key, an unknown model, a refusal) is final.

/** Tries after the first. */
export const RETRIES = 2;
/** Pause before the 1st and the 2nd retry. */
export const BACKOFF_MS = [1_000, 3_000] as const;
/** A provider's `retry-after` longer than this is cut to it. */
export const MAX_RETRY_AFTER_MS = 10_000;

/** Whether a failed attempt may be tried again: `status` undefined = no answer (timeout, network). */
export function retryable(status: number | undefined): boolean {
  return status === undefined || status === 408 || status === 429 || status >= 500;
}

/** Milliseconds a `retry-after` header asks for (seconds or an HTTP date), or undefined. */
export function retryAfterMs(value: string | null | undefined, now = Date.now()): number | undefined {
  if (!value) return undefined;
  const secs = Number(value);
  if (Number.isFinite(secs) && secs >= 0) return Math.min(MAX_RETRY_AFTER_MS, secs * 1000);
  const at = Date.parse(value);
  return Number.isNaN(at) ? undefined : Math.min(MAX_RETRY_AFTER_MS, Math.max(0, at - now));
}

/** One attempt's outcome: its result, and whether (and after how long) to try again. */
export interface Attempt<T> {
  result: T;
  retry: boolean;
  afterMs?: number;
}

export type Sleep = (ms: number) => Promise<void>;
const realSleep: Sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Runs `attempt` until it needs no retry or `retries` run out; the last result counts. */
export async function withRetry<T>(attempt: (n: number) => Promise<Attempt<T>>, sleep: Sleep = realSleep, retries: number = RETRIES): Promise<T> {
  for (let n = 0; ; n++) {
    const a = await attempt(n);
    if (!a.retry || n >= retries) return a.result;
    await sleep(a.afterMs ?? BACKOFF_MS[Math.min(n, BACKOFF_MS.length - 1)]);
  }
}
