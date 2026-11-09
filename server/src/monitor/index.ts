// The owner's machine monitor (docs/protocol.md, "Machine monitor"): one read route that joins a
// machine's MachineLink (status, metrics, history) with every runner bot's BotDesk (its runs on
// the machine, its queue and today's results). Metrics come with the daemon's heartbeat
// (docs/machine-protocol.md, "Metrics") and are stored by MachineLink through ./store.
import {
  type MachineMetrics,
  type MachineMonitor,
  MonitorQuery,
  type MonitorCli,
  type MonitorMember,
  type MonitorRun,
} from "@chat/protocol";
import { linkStub, deskStub } from "../bots/env";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import type { DeskRollup } from "./rollup";

const GIB = 1024 ** 3;
const DAY_MS = 86_400_000;

/** Memory kept free for spikes: a fifth of the host, at least 2 GiB, in whole GiB. */
export function reserveBytes(total: number): number {
  return Math.max(2, Math.round((total * 0.2) / GIB)) * GIB;
}

/** Runs the machine can still take: free slots, and memory above the reserve at the average VM size. */
export function canTake(slots: number, running: number, availableBytes?: number, totalBytes?: number, avgVmBytes?: number): number {
  let free = Math.max(0, slots - running);
  if (availableBytes !== undefined && totalBytes !== undefined && avgVmBytes && avgVmBytes > 0) {
    free = Math.min(free, Math.max(0, Math.floor((availableBytes - reserveBytes(totalBytes)) / avgVmBytes)));
  }
  return free;
}

type Rollup = DeskRollup & { lastFailureTitle?: string };

export function buildMonitor(input: {
  machine: { id: string; name: string; revoked: boolean };
  link: Awaited<ReturnType<ReturnType<typeof linkStub>["monitor"]>> | null;
  desks: { botId: string; rollup: Rollup }[];
  now: number;
}): MachineMonitor {
  const { link, desks, now } = input;
  const status = link?.status;
  const metrics: MachineMetrics | null = link?.metrics ?? null;
  const health = status?.health;
  const info = status?.info;
  const botOf = new Map<string, string>((link?.runBots ?? []).map((r) => [r.runId, r.botId]));
  const deskRun = new Map<string, { status: string; paused: boolean; startedAt: number | null; botId: string }>();
  for (const d of desks) for (const a of d.rollup.active) deskRun.set(a.runId, { ...a, botId: d.botId });

  // Runs on the machine: the daemon's VMs when it reports them, else the server's open runs.
  const runs: MonitorRun[] = [];
  const seen = new Set<string>();
  const stateOf = (runId: string, phase?: string): string => {
    const d = deskRun.get(runId);
    if (phase === "booting") return "booting";
    if (phase === "paused" || d?.paused) return "paused";
    if (d?.status === "waiting") return "waiting";
    return "running";
  };
  for (const v of metrics?.runs ?? []) {
    seen.add(v.runId);
    const memberId = botOf.get(v.runId) ?? deskRun.get(v.runId)?.botId;
    runs.push({
      runId: v.runId,
      ...(memberId ? { memberId } : {}),
      state: stateOf(v.runId, v.phase),
      startedAt: v.startedAt,
      ...(v.bootMs !== undefined ? { bootMs: v.bootMs } : {}),
      ...(v.cpuCores !== undefined ? { cpuCores: v.cpuCores } : {}),
      ...(v.memBytes !== undefined ? { memBytes: v.memBytes } : {}),
      ...(v.scratchBytes !== undefined ? { scratchBytes: v.scratchBytes } : {}),
    });
  }
  // Runs the server gave the machine that its last report did not list yet are still booting;
  // without metrics (an older daemon) the server's runs are all there is.
  for (const [runId, d] of deskRun) {
    if (seen.has(runId)) continue;
    seen.add(runId);
    runs.push({ runId, memberId: d.botId, state: stateOf(runId, metrics ? "booting" : undefined), ...(d.startedAt ? { startedAt: d.startedAt } : {}) });
  }
  let queueOldest: number | undefined;
  let queueCount = 0;
  for (const d of desks) {
    for (const q of d.rollup.queued) {
      queueCount++;
      queueOldest = queueOldest === undefined ? q.queuedAt : Math.min(queueOldest, q.queuedAt);
      if (runs.length < 500) runs.push({ runId: q.runId, memberId: d.botId, state: "queued" });
    }
  }

  const onMachine = runs.filter((r) => r.state !== "queued");
  const memVmBytes = onMachine.reduce((s, r) => s + (r.memBytes ?? 0), 0);
  const withMem = onMachine.filter((r) => r.memBytes !== undefined && r.memBytes > 0);
  const avgVmBytes = withMem.length ? Math.round(memVmBytes / withMem.length) : undefined;
  const slots = metrics?.vmSlots ?? info?.maxParallelRuns ?? 0;
  const running = metrics ? onMachine.length : Math.max(onMachine.length, health?.activeRuns ?? 0);

  const today = { finished: 0, succeeded: 0, failed: 0, finishedYesterday: 0, tokens: 0 };
  const failedByRuntime: Record<string, number> = {};
  let lastFailure: MachineMonitor["lastFailure"];
  for (const d of desks) {
    const r = d.rollup;
    today.finished += r.finished;
    today.succeeded += r.succeeded;
    today.failed += r.failed;
    today.finishedYesterday += r.finishedYesterday;
    today.tokens += r.tokens;
    for (const [k, n] of Object.entries(r.failedByRuntime)) failedByRuntime[k] = (failedByRuntime[k] ?? 0) + n;
    if (r.lastFailure && (!lastFailure || r.lastFailure.at > lastFailure.at)) {
      lastFailure = { runId: r.lastFailure.runId, memberId: d.botId, at: r.lastFailure.at, ...(r.lastFailureTitle ? { title: r.lastFailureTitle.slice(0, 200) } : {}) };
    }
  }

  const logins = new Map((metrics?.logins ?? []).map((l) => [l.runtime, l.state]));
  const clis: MonitorCli[] = (status?.clis ?? []).map((c) => {
    const l = logins.get(c.runtime);
    return {
      runtime: c.runtime,
      kind: c.kind,
      available: c.available,
      ...(c.version ? { version: c.version } : {}),
      login: l === "logged_in" || l === "logged_out" ? l : "unknown",
      failedToday: failedByRuntime[c.runtime] ?? 0,
    };
  });

  const members: MonitorMember[] = desks
    .map(({ botId, rollup }) => {
      const mine = onMachine.filter((r) => r.memberId === botId);
      return {
        memberId: botId,
        running: mine.filter((r) => r.state !== "waiting").length,
        waiting: mine.filter((r) => r.state === "waiting").length,
        queued: rollup.queued.length,
        cpuCores: Math.round(mine.reduce((s, r) => s + (r.cpuCores ?? 0), 0) * 100) / 100,
        memBytes: mine.reduce((s, r) => s + (r.memBytes ?? 0), 0),
        tokensToday: rollup.tokens,
        finishedToday: rollup.finished,
      };
    })
    .filter((m) => m.running + m.waiting + m.queued + m.finishedToday > 0 || m.tokensToday > 0);

  const sampledAt = link?.metricsAt ?? undefined;
  const out: MachineMonitor = {
    machine: {
      id: input.machine.id,
      name: input.machine.name,
      connected: status?.connected ?? false,
      ...(input.machine.revoked ? { revoked: true } : {}),
      ...(status?.lastHeartbeatAt !== undefined ? { lastHeartbeatAt: status.lastHeartbeatAt } : {}),
      ...(link?.connectedAt && status?.connected ? { connectedAt: link.connectedAt } : {}),
      ...(info ? { daemonVersion: info.daemonVersion.slice(0, 64), os: info.os.slice(0, 64), arch: info.arch.slice(0, 64) } : {}),
    },
    series: link?.series ?? { range: "1h", from: Math.max(0, now - 3_600_000), stepMs: 60_000, cpuPct: [], memPct: [], diskPct: [], netRxBps: [], netTxBps: [] },
    capacity: {
      slots,
      running,
      canTake: canTake(slots, running, metrics?.memAvailableBytes, metrics?.memTotalBytes, avgVmBytes),
      ...(avgVmBytes !== undefined ? { avgVmBytes } : {}),
      ...(metrics?.memTotalBytes !== undefined ? { reserveBytes: reserveBytes(metrics.memTotalBytes) } : {}),
    },
    queue: { count: queueCount, ...(queueOldest !== undefined ? { oldestAt: queueOldest } : {}) },
    today,
    clis,
    runs,
    members,
  };
  if (sampledAt) out.sampledAt = sampledAt;
  if (metrics?.boot) out.boot = metrics.boot;
  if (lastFailure) out.lastFailure = lastFailure;
  if (health) {
    out.host = {
      load1: health.load1,
      memVmBytes,
      diskTotalBytes: health.diskTotalBytes,
      diskFreeBytes: health.diskFreeBytes,
      ...(metrics?.cpuPct !== undefined ? { cpuPct: metrics.cpuPct } : {}),
      cpuThreads: metrics?.cpuThreads ?? health.cpuCount,
      ...(metrics?.memTotalBytes !== undefined ? { memTotalBytes: metrics.memTotalBytes } : {}),
      ...(metrics?.memAvailableBytes !== undefined ? { memAvailableBytes: metrics.memAvailableBytes } : {}),
      ...(metrics?.netRxBps !== undefined ? { netRxBps: metrics.netRxBps } : {}),
      ...(metrics?.netTxBps !== undefined ? { netTxBps: metrics.netTxBps } : {}),
      ...(metrics?.uptimeSecs !== undefined ? { uptimeSecs: metrics.uptimeSecs } : {}),
    };
  }
  return out;
}

registerRoute({
  method: "GET",
  path: "/v1/machines/:id/monitor",
  auth: "owner",
  async handler({ env, params, url }) {
    const q = MonitorQuery.safeParse({
      range: url.searchParams.get("range") ?? undefined,
      dayStart: url.searchParams.get("dayStart") ?? undefined,
    });
    if (!q.success) throw new ApiError("invalid", "range must be 15m, 1h or 24h; dayStart a time in ms");
    const now = Date.now();
    // Without the viewer's midnight, 今天 is the last 24 hours.
    const dayStart = Math.min(q.data.dayStart ?? now - DAY_MS, now);
    const row = await env.DB.prepare("SELECT id, name, revoked_at FROM machines WHERE id = ?")
      .bind(params.id)
      .first<{ id: string; name: string; revoked_at: number | null }>();
    if (!row) throw new ApiError("not_found", "machine not found");
    const bots = await env.DB.prepare(
      "SELECT r.member_id AS id FROM bot_runners r JOIN members m ON m.id = r.member_id WHERE m.deactivated = 0 ORDER BY r.member_id",
    ).all<{ id: string }>();
    const [link, desks] = await Promise.all([
      row.revoked_at === null ? linkStub(env, row.id).monitor(q.data.range, now).catch(() => null) : Promise.resolve(null),
      Promise.all(
        bots.results.map(async (b) => {
          try {
            return { botId: b.id, rollup: await deskStub(env, b.id).monitorRollup(b.id, row.id, dayStart) };
          } catch {
            return null;
          }
        }),
      ),
    ]);
    return buildMonitor({
      machine: { id: row.id, name: row.name, revoked: row.revoked_at !== null },
      link,
      desks: desks.filter((d): d is { botId: string; rollup: Rollup } => d !== null),
      now,
    });
  },
});
