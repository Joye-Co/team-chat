import { ApiError } from "./errors";

/** In-memory token bucket. `rate` tokens per second, at most `burst` stored. */
export class TokenBucket {
  private tokens: number;
  private last: number;
  constructor(
    private readonly rate: number,
    private readonly burst: number,
    private readonly now: () => number = Date.now,
  ) {
    this.tokens = burst;
    this.last = now();
  }
  take(cost = 1): boolean {
    const t = this.now();
    this.tokens = Math.min(this.burst, this.tokens + ((t - this.last) / 1000) * this.rate);
    this.last = t;
    if (this.tokens < cost) return false;
    this.tokens -= cost;
    return true;
  }
}

/** Limits per member, enforced in that member's gateway (shared by all its sockets). */
export const LIMITS = {
  /** send, edit, delete, topic.update and feature write frames: about 20 per second. */
  write: { rate: 20, burst: 20 },
  /** delta frames of streaming replies. */
  delta: { rate: 100, burst: 200 },
  /** fetch, history and feature read frames. */
  read: { rate: 50, burst: 100 },
  /** Typing: at most one per 3 s per member and topic (enforced in the room). */
  typingIntervalMs: 3000,
  /** Default HTTP request body limit; feature routes set their own. */
  httpBodyBytes: 32 * 1024,
} as const;

/** Invite code attempts per client IP: 10 per minute. */
export const REDEEM_RATE = { windowMs: 60_000, max: 10 };

/** One counted attempt: the window it was counted in. */
export interface Attempt {
  key: string;
  window: number;
}

/**
 * Counts one attempt for `key` in the current fixed window (D1, so every location shares the
 * count). Throws `rate_limited` once the window holds more than `max` attempts.
 */
export async function takeAttempt(db: D1Database, key: string, rate: { windowMs: number; max: number }): Promise<Attempt> {
  const now = Date.now();
  const window = Math.floor(now / rate.windowMs) * rate.windowMs;
  const row = await db
    .prepare(
      `INSERT INTO auth_rate_hits (key, window, count) VALUES (?, ?, 1)
       ON CONFLICT (key, window) DO UPDATE SET count = count + 1 RETURNING count`,
    )
    .bind(key, window)
    .first<{ count: number }>();
  if (Math.random() < 0.02) {
    await db
      .prepare("DELETE FROM auth_rate_hits WHERE window < ?")
      .bind(now - 24 * 3600_000)
      .run()
      .catch(() => {});
  }
  if ((row?.count ?? 1) > rate.max) throw new ApiError("rate_limited", "too many invite code attempts");
  return { key, window };
}

/** Gives back an attempt that succeeded: only wrong codes use up the limit. */
export async function refundAttempt(db: D1Database, attempt: Attempt): Promise<void> {
  await db
    .prepare("UPDATE auth_rate_hits SET count = count - 1 WHERE key = ? AND window = ? AND count > 0")
    .bind(attempt.key, attempt.window)
    .run();
}
