// Machine metrics in MachineLink's SQLite: the latest sample (meta) and a 25-hour history, one
// row per heartbeat, read back as bucketed series for the owner's monitor. docs/protocol.md,
// "Machine monitor"; docs/machine-protocol.md, "Metrics".
import { type Health, MachineMetrics, type MonitorRange, type MonitorSeries } from "@chat/protocol";
import type { MigrationStep } from "../core/migrations";

/** History is kept this long (24 h sparklines plus a margin). */
export const HISTORY_MS = 25 * 60 * 60_000;

/** Named migration of MachineLink: the history table. */
export const METRICS_MIGRATION: MigrationStep = {
  name: "machine-2-metrics",
  up: `
    CREATE TABLE metrics (
      at INTEGER PRIMARY KEY,
      cpu REAL,
      mem_used INTEGER,
      mem_total INTEGER,
      disk_used INTEGER,
      disk_total INTEGER,
      rx REAL,
      tx REAL
    );
  `,
};

/** Buckets of each range: count and width. */
export const RANGES: Record<MonitorRange, { buckets: number; stepMs: number }> = {
  "15m": { buckets: 30, stepMs: 30_000 },
  "1h": { buckets: 60, stepMs: 60_000 },
  "24h": { buckets: 48, stepMs: 30 * 60_000 },
};

/**
 * Keeps a heartbeat's metrics: the latest sample in meta and one history row at `at` (server
 * clock). Metrics that do not parse are dropped (the heartbeat itself still counts), so a newer
 * daemon's shape never costs the machine its lease.
 */
export function recordMetrics(
  sql: SqlStorage,
  setMeta: (key: string, value: string) => void,
  raw: unknown,
  health: Health,
  at: number,
): MachineMetrics | null {
  if (raw === undefined) return null;
  const parsed = MachineMetrics.safeParse(raw);
  if (!parsed.success) return null;
  const m = parsed.data;
  setMeta("metrics", JSON.stringify(m));
  setMeta("metrics_at", String(at));
  const memUsed =
    m.memTotalBytes !== undefined && m.memAvailableBytes !== undefined ? Math.max(0, m.memTotalBytes - m.memAvailableBytes) : null;
  const diskUsed = health.diskTotalBytes > 0 ? Math.max(0, health.diskTotalBytes - health.diskFreeBytes) : null;
  sql.exec(
    "INSERT OR REPLACE INTO metrics (at, cpu, mem_used, mem_total, disk_used, disk_total, rx, tx) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    at,
    m.cpuPct ?? null,
    memUsed,
    m.memTotalBytes ?? null,
    diskUsed,
    health.diskTotalBytes > 0 ? health.diskTotalBytes : null,
    m.netRxBps ?? null,
    m.netTxBps ?? null,
  );
  sql.exec("DELETE FROM metrics WHERE at < ?", at - HISTORY_MS);
  return m;
}

/** The latest sample and when it arrived (server clock), or nulls before the first. */
export function latestMetrics(meta: (key: string) => string | null): { metrics: MachineMetrics | null; at: number | null } {
  const raw = meta("metrics");
  const at = meta("metrics_at");
  if (!raw || !at) return { metrics: null, at: null };
  const parsed = MachineMetrics.safeParse(JSON.parse(raw));
  return parsed.success ? { metrics: parsed.data, at: Number(at) } : { metrics: null, at: null };
}

const round1 = (x: number | null): number | null => (x === null || !Number.isFinite(x) ? null : Math.round(Math.max(0, x) * 10) / 10);

/** The history of `range` up to `now` as equal buckets, oldest first; a bucket without a sample is null. */
export function readSeries(sql: SqlStorage, range: MonitorRange, now: number): MonitorSeries {
  const { buckets, stepMs } = RANGES[range];
  const from = Math.floor(now / stepMs) * stepMs - (buckets - 1) * stepMs;
  const empty = (): (number | null)[] => Array.from({ length: buckets }, () => null);
  const out: MonitorSeries = { range, from, stepMs, cpuPct: empty(), memPct: empty(), diskPct: empty(), netRxBps: empty(), netTxBps: empty() };
  const rows = sql
    .exec<{ b: number; cpu: number | null; mem: number | null; disk: number | null; rx: number | null; tx: number | null }>(
      `SELECT (at - ?) / ? AS b,
              avg(cpu) AS cpu,
              avg(CASE WHEN mem_total > 0 THEN mem_used * 100.0 / mem_total END) AS mem,
              avg(CASE WHEN disk_total > 0 THEN disk_used * 100.0 / disk_total END) AS disk,
              avg(rx) AS rx,
              avg(tx) AS tx
       FROM metrics WHERE at >= ? AND at <= ? GROUP BY b ORDER BY b`,
      from,
      stepMs,
      from,
      now,
    )
    .toArray();
  for (const r of rows) {
    const i = Math.floor(r.b);
    if (i < 0 || i >= buckets) continue;
    out.cpuPct[i] = round1(r.cpu);
    out.memPct[i] = round1(r.mem);
    out.diskPct[i] = round1(r.disk);
    out.netRxBps[i] = r.rx === null ? null : Math.round(r.rx);
    out.netTxBps[i] = r.tx === null ? null : Math.round(r.tx);
  }
  return out;
}
