// MonitorState: one Durable Object (idFromName("monitor")) with SQLite. Holds each target's
// alert state, recent check results, alerts not yet posted, the topics it posts into and the
// day of the last summary. All alert decisions happen here, in one place, in order.
import { DurableObject } from "cloudflare:workers";
import { type Alert, type CheckResult, INITIAL, type Status, type TargetState, stateKey, step } from "./alerts";

const RETAIN_MS = 8 * 24 * 60 * 60 * 1000;
const PENDING_MAX_AGE_MS = 3 * 24 * 60 * 60 * 1000;

export interface PendingAlert {
  id: string;
  alert: Alert;
  createdAt: number;
  attempts: number;
}

export interface TargetSummary {
  target: string;
  status: Status;
  since: number;
  checks: number;
  failures: number;
  uptime: number | null;
  p50: number | null;
  p95: number | null;
}

function percentile(sorted: number[], p: number): number | null {
  if (sorted.length === 0) return null;
  const i = Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length) - 1));
  return sorted[i];
}

export class MonitorState extends DurableObject<Record<string, unknown>> {
  private readonly sql: SqlStorage;

  constructor(ctx: DurableObjectState, env: Record<string, unknown>) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS targets (
        name TEXT PRIMARY KEY, status TEXT NOT NULL, fails INTEGER NOT NULL, since INTEGER NOT NULL,
        alerted INTEGER NOT NULL, last_error TEXT
      );
      CREATE TABLE IF NOT EXISTS checks (
        at INTEGER NOT NULL, target TEXT NOT NULL, ok INTEGER NOT NULL,
        health_ms INTEGER, rtt_ms INTEGER, error TEXT
      );
      CREATE INDEX IF NOT EXISTS checks_at ON checks(at);
      CREATE TABLE IF NOT EXISTS pending (
        id TEXT PRIMARY KEY, alert TEXT NOT NULL, created_at INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0
      );
      CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    `);
  }

  private state(name: string): TargetState {
    const r = this.sql
      .exec<{ status: string; fails: number; since: number; alerted: number; last_error: string | null }>(
        "SELECT status, fails, since, alerted, last_error FROM targets WHERE name = ?",
        name,
      )
      .toArray()[0];
    if (!r) return { ...INITIAL };
    return { status: r.status as Status, fails: r.fails, since: r.since, alerted: r.alerted === 1, lastError: r.last_error };
  }

  /** Records check results and returns the new alerts (also queued as pending until posted). */
  async record(results: CheckResult[], threshold: number): Promise<PendingAlert[]> {
    const raised: PendingAlert[] = [];
    this.ctx.storage.transactionSync(() => {
      for (const r of results) {
        const key = stateKey(r);
        const { next, alert } = step(this.state(key), r, threshold);
        this.sql.exec(
          `INSERT INTO targets (name, status, fails, since, alerted, last_error) VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT (name) DO UPDATE SET status = excluded.status, fails = excluded.fails, since = excluded.since,
             alerted = excluded.alerted, last_error = excluded.last_error`,
          key,
          next.status,
          next.fails,
          next.since,
          next.alerted ? 1 : 0,
          next.lastError,
        );
        this.sql.exec(
          "INSERT INTO checks (at, target, ok, health_ms, rtt_ms, error) VALUES (?, ?, ?, ?, ?, ?)",
          r.at,
          key,
          r.ok ? 1 : 0,
          r.healthMs,
          r.rttMs,
          r.error,
        );
        if (alert) {
          const p: PendingAlert = { id: crypto.randomUUID(), alert, createdAt: r.at, attempts: 0 };
          this.sql.exec("INSERT INTO pending (id, alert, created_at) VALUES (?, ?, ?)", p.id, JSON.stringify(alert), p.createdAt);
          raised.push(p);
        }
      }
      const now = results[0]?.at ?? Date.now();
      this.sql.exec("DELETE FROM checks WHERE at < ?", now - RETAIN_MS);
      this.sql.exec("DELETE FROM pending WHERE created_at < ?", now - PENDING_MAX_AGE_MS);
    });
    return raised;
  }

  async targetState(name: string): Promise<TargetState> {
    return this.state(name);
  }

  /** Alerts not yet posted, oldest first. */
  async pending(): Promise<PendingAlert[]> {
    return this.sql
      .exec<{ id: string; alert: string; created_at: number; attempts: number }>(
        "SELECT id, alert, created_at, attempts FROM pending ORDER BY created_at, rowid",
      )
      .toArray()
      .map((r) => ({ id: r.id, alert: JSON.parse(r.alert) as Alert, createdAt: r.created_at, attempts: r.attempts }));
  }

  async markSent(id: string): Promise<void> {
    this.sql.exec("DELETE FROM pending WHERE id = ?", id);
  }

  async markFailed(id: string): Promise<void> {
    this.sql.exec("UPDATE pending SET attempts = attempts + 1 WHERE id = ?", id);
  }

  async get(key: string): Promise<string | null> {
    return this.sql.exec<{ value: string }>("SELECT value FROM kv WHERE key = ?", key).toArray()[0]?.value ?? null;
  }

  async set(key: string, value: string | null): Promise<void> {
    if (value === null) this.sql.exec("DELETE FROM kv WHERE key = ?", key);
    else this.sql.exec("INSERT OR REPLACE INTO kv (key, value) VALUES (?, ?)", key, value);
  }

  /** Sets `key` to `now` and returns true unless it was set less than `gapMs` ago (atomic). */
  async claim(key: string, now: number, gapMs: number): Promise<boolean> {
    const last = Number((await this.get(key)) ?? Number.NEGATIVE_INFINITY);
    if (now - last < gapMs) return false;
    await this.set(key, String(now));
    return true;
  }

  /** Per-target stats of the checks in [from, to). Round-trip latency percentiles in ms. */
  async summary(targets: string[], from: number, to: number): Promise<TargetSummary[]> {
    return targets.map((target) => {
      const rows = this.sql
        .exec<{ ok: number; rtt_ms: number | null }>(
          "SELECT ok, rtt_ms FROM checks WHERE target = ? AND at >= ? AND at < ?",
          target,
          from,
          to,
        )
        .toArray();
      const failures = rows.filter((r) => r.ok === 0).length;
      const rtts = rows
        .filter((r) => r.ok === 1 && r.rtt_ms !== null)
        .map((r) => r.rtt_ms as number)
        .sort((a, b) => a - b);
      const s = this.state(target);
      return {
        target,
        status: s.status,
        since: s.since,
        checks: rows.length,
        failures,
        uptime: rows.length ? (rows.length - failures) / rows.length : null,
        p50: percentile(rtts, 0.5),
        p95: percentile(rtts, 0.95),
      };
    });
  }
}
