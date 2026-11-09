// The owner's machine monitor (docs/protocol.md, "Machine monitor"):
// `GET /v1/machines/:id/monitor` answers one MachineMonitor.
import { z } from "zod";
import { Id, Millis } from "./common.ts";
import { BootStats, RunId } from "./machine.ts";

export const MonitorRange = z.enum(["15m", "1h", "24h"]);
export type MonitorRange = z.infer<typeof MonitorRange>;

/** `?range=15m|1h|24h&dayStart=<ms>`: dayStart is the viewer's local midnight (今天). */
export const MonitorQuery = z.object({
  range: MonitorRange.default("1h"),
  dayStart: z.coerce.number().int().nonnegative().optional(),
});
export type MonitorQuery = z.infer<typeof MonitorQuery>;

export const MAX_MONITOR_POINTS = 120;
/** One value per bucket, oldest first; null = no sample in the bucket. */
const Points = z.array(z.number().nonnegative().nullable()).max(MAX_MONITOR_POINTS);
export const MonitorSeries = z.object({
  range: MonitorRange,
  /** Start of the first bucket. */
  from: Millis,
  stepMs: z.int().positive(),
  cpuPct: Points,
  memPct: Points,
  diskPct: Points,
  netRxBps: Points,
  netTxBps: Points,
});
export type MonitorSeries = z.infer<typeof MonitorSeries>;

/** The latest sample of the host. */
export const MonitorHost = z.object({
  cpuPct: z.number().min(0).max(100).optional(),
  cpuThreads: z.int().nonnegative().optional(),
  load1: z.number().nonnegative(),
  memTotalBytes: z.int().nonnegative().optional(),
  memAvailableBytes: z.int().nonnegative().optional(),
  /** Memory the VMs use (sum of their resident memory). */
  memVmBytes: z.int().nonnegative(),
  diskTotalBytes: z.int().nonnegative(),
  diskFreeBytes: z.int().nonnegative(),
  netRxBps: z.number().nonnegative().optional(),
  netTxBps: z.number().nonnegative().optional(),
  uptimeSecs: z.int().nonnegative().optional(),
});
export type MonitorHost = z.infer<typeof MonitorHost>;

/** How many more runs the machine can take: free slots, and memory left above a reserve. */
export const MonitorCapacity = z.object({
  slots: z.int().nonnegative(),
  running: z.int().nonnegative(),
  canTake: z.int().nonnegative(),
  /** Average memory of a running VM, used for the estimate. */
  avgVmBytes: z.int().nonnegative().optional(),
  /** Memory kept free for spikes. */
  reserveBytes: z.int().nonnegative().optional(),
});
export type MonitorCapacity = z.infer<typeof MonitorCapacity>;

/** `MonitorRun.state` values; clients read unknown states as running. */
export const MONITOR_RUN_STATES = ["running", "waiting", "booting", "paused", "queued"] as const;
export const MonitorRun = z.object({
  runId: RunId,
  /** The AI member the run belongs to, when the server knows it. */
  memberId: Id.optional(),
  /** "running" (在跑) | "waiting" (等你) | "booting" (起机中) | "paused" (接管) | "queued" (排队) */
  state: z.string().min(1).max(32),
  startedAt: Millis.optional(),
  bootMs: z.int().nonnegative().optional(),
  cpuCores: z.number().nonnegative().optional(),
  memBytes: z.int().nonnegative().optional(),
  scratchBytes: z.int().nonnegative().optional(),
});
export type MonitorRun = z.infer<typeof MonitorRun>;

/** One AI member's use of the machine (按成员). Today = since `dayStart`. */
export const MonitorMember = z.object({
  memberId: Id,
  running: z.int().nonnegative(),
  waiting: z.int().nonnegative(),
  queued: z.int().nonnegative(),
  cpuCores: z.number().nonnegative(),
  memBytes: z.int().nonnegative(),
  tokensToday: z.int().nonnegative(),
  finishedToday: z.int().nonnegative(),
});
export type MonitorMember = z.infer<typeof MonitorMember>;

export const CliLoginState = z.enum(["logged_in", "logged_out", "unknown"]);
export const MonitorCli = z.object({
  runtime: z.string().min(1).max(64),
  kind: z.string().min(1).max(32),
  available: z.boolean(),
  version: z.string().max(200).optional(),
  login: CliLoginState,
  /** Runs of this runtime on the machine that failed today. */
  failedToday: z.int().nonnegative(),
});
export type MonitorCli = z.infer<typeof MonitorCli>;

export const MonitorFailure = z.object({
  runId: RunId,
  memberId: Id,
  at: Millis,
  /** The topic's title, when the server could read it. */
  title: z.string().max(200).optional(),
});
export type MonitorFailure = z.infer<typeof MonitorFailure>;

export const MachineMonitor = z.object({
  machine: z.object({
    id: Id,
    name: z.string().min(1).max(100),
    connected: z.boolean(),
    revoked: z.boolean().optional(),
    lastHeartbeatAt: Millis.optional(),
    /** When the current link came up. */
    connectedAt: Millis.optional(),
    daemonVersion: z.string().max(64).optional(),
    os: z.string().max(64).optional(),
    arch: z.string().max(64).optional(),
  }),
  /** When the daemon took the latest metrics sample (server clock); absent before the first. */
  sampledAt: Millis.optional(),
  host: MonitorHost.optional(),
  series: MonitorSeries,
  capacity: MonitorCapacity,
  boot: BootStats.optional(),
  queue: z.object({ count: z.int().nonnegative(), oldestAt: Millis.optional() }),
  today: z.object({
    finished: z.int().nonnegative(),
    succeeded: z.int().nonnegative(),
    failed: z.int().nonnegative(),
    finishedYesterday: z.int().nonnegative(),
    tokens: z.int().nonnegative(),
  }),
  clis: z.array(MonitorCli).max(64),
  runs: z.array(MonitorRun).max(500),
  members: z.array(MonitorMember).max(500),
  lastFailure: MonitorFailure.optional(),
});
export type MachineMonitor = z.infer<typeof MachineMonitor>;
