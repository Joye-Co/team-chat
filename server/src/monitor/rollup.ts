// One AI member's share of a machine, read from its BotDesk's runs (docs/protocol.md, "Machine
// monitor"): runs active on the machine, its queue (排队, not tied to a machine yet), and what
// ended today on the machine.
import type { RunStatus } from "@chat/protocol";
import { sanitizeUsage } from "../bots/desk/core";

export interface DeskRollup {
  active: { runId: string; status: RunStatus; startedAt: number | null; runtime: string | null; paused: boolean }[];
  queued: { runId: string; queuedAt: number }[];
  finished: number;
  succeeded: number;
  failed: number;
  finishedYesterday: number;
  tokens: number;
  failedByRuntime: Record<string, number>;
  lastFailure: { runId: string; at: number; groupId: string; topicId: string } | null;
}

const DAY_MS = 86_400_000;
/** Ended without a result: a CLI failure, or lost / stuck (blocked). Cancelled runs are neither. */
const FAILED = "('failed','blocked')";

export function deskRollup(sql: SqlStorage, machineId: string, dayStart: number): DeskRollup {
  const active = sql
    .exec<{ id: string; status: RunStatus; started_at: number | null; created_at: number; runtime: string | null; paused_by: string | null }>(
      "SELECT id, status, started_at, created_at, runtime, paused_by FROM runs WHERE ended_at IS NULL AND machine_id = ? ORDER BY created_at",
      machineId,
    )
    .toArray()
    .map((r) => ({ runId: r.id, status: r.status, startedAt: r.started_at ?? r.created_at, runtime: r.runtime, paused: r.paused_by !== null }));
  const queued = sql
    .exec<{ id: string; queued_at: number }>(
      "SELECT id, queued_at FROM runs WHERE ended_at IS NULL AND status = 'created' AND machine_id IS NULL AND queued_at IS NOT NULL ORDER BY queued_at, id",
    )
    .toArray()
    .map((r) => ({ runId: r.id, queuedAt: r.queued_at }));
  const ended = sql
    .exec<{ id: string; status: string; ended_at: number; usage: string | null; runtime: string | null; group_id: string; topic_id: string }>(
      "SELECT id, status, ended_at, usage, runtime, group_id, topic_id FROM runs WHERE machine_id = ? AND ended_at >= ? ORDER BY ended_at",
      machineId,
      dayStart - DAY_MS,
    )
    .toArray();
  const out: DeskRollup = {
    active,
    queued,
    finished: 0,
    succeeded: 0,
    failed: 0,
    finishedYesterday: 0,
    tokens: 0,
    failedByRuntime: {},
    lastFailure: null,
  };
  for (const r of ended) {
    if (r.ended_at < dayStart) {
      out.finishedYesterday++;
      continue;
    }
    out.finished++;
    if (r.status === "done") out.succeeded++;
    if (FAILED.includes(`'${r.status}'`)) {
      out.failed++;
      if (r.runtime) out.failedByRuntime[r.runtime] = (out.failedByRuntime[r.runtime] ?? 0) + 1;
      out.lastFailure = { runId: r.id, at: r.ended_at, groupId: r.group_id, topicId: r.topic_id };
    }
    const usage = r.usage ? sanitizeUsage(safeJson(r.usage)) : null;
    if (usage) out.tokens += usage.inputTokens + usage.outputTokens;
  }
  return out;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
