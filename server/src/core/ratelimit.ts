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
