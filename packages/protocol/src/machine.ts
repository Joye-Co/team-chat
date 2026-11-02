// Machine protocol: the daemon <-> MachineLink wire contract. See docs/machine-protocol.md.
// These frames never reach desktop clients, except the run log entries (`RunLogEntry`), which
// clients read with `GET /v1/runs/:id/events`.
import { z } from "zod";
import { Id, Millis, Seq } from "./common.ts";
import { RunPlan } from "./plan.ts";

export const MACHINE_PROTOCOL_VERSION = 1 as const;

/** Run ids become folder names on the machine: `[A-Za-z0-9_-]{1,64}`. */
export const RunId = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/);
/** Machine and bot runner labels: 1..64 of `[A-Za-z0-9_.:-]`. */
export const Label = z.string().regex(/^[A-Za-z0-9_.:-]{1,64}$/);
export const MAX_LABELS = 20;
export const Labels = z.array(Label).max(MAX_LABELS);

export const MachineInfo = z.object({
  /** The id configured on the machine (optional); the token is authoritative. */
  id: z.string().optional(),
  name: z.string().min(1).max(64),
  labels: z.array(z.string()).max(MAX_LABELS),
  daemonVersion: z.string().max(64),
  os: z.string().max(32),
  arch: z.string().max(32),
  maxParallelRuns: z.int().nonnegative(),
});
export type MachineInfo = z.infer<typeof MachineInfo>;

export const CliKind = z.enum(["codex", "claude", "acp", "pty"]);
export const CliInfo = z.object({
  /** The runtime name configured on the machine; `run.assign.runtime` names one. */
  runtime: z.string().min(1).max(64),
  kind: CliKind,
  command: z.string().max(1024),
  version: z.string().max(200).optional(),
  available: z.boolean(),
});
export type CliInfo = z.infer<typeof CliInfo>;

export const Health = z.object({
  diskFreeBytes: z.int().nonnegative(),
  diskTotalBytes: z.int().nonnegative(),
  load1: z.number().nonnegative(),
  load5: z.number().nonnegative(),
  load15: z.number().nonnegative(),
  cpuCount: z.int().nonnegative(),
  activeRuns: z.int().nonnegative(),
});
export type Health = z.infer<typeof Health>;

export const RunCursor = z.object({ runId: RunId, lastSeq: Seq, done: z.boolean() });
export type RunCursor = z.infer<typeof RunCursor>;
export const KnownRun = z.object({ runId: RunId, ackedSeq: Seq });
export type KnownRun = z.infer<typeof KnownRun>;

export const RepoSpec = z.object({
  url: z.string().min(1).max(2048).optional(),
  path: z.string().min(1).max(1024).optional(),
  baseBranch: z.string().min(1).max(200).optional(),
});
export type RepoSpec = z.infer<typeof RepoSpec>;

export const SessionUpload = z.object({
  url: z.string().min(1),
  headers: z.record(z.string(), z.string()).optional(),
});

export const Usage = z.object({
  inputTokens: z.int().nonnegative(),
  outputTokens: z.int().nonnegative(),
  costUsd: z.number().nonnegative().optional(),
});
export type Usage = z.infer<typeof Usage>;

// ---------------------------------------------------------------------------
// Run events (the item model)
// ---------------------------------------------------------------------------

const item = { itemId: z.string().min(1), turnId: z.string().optional() };
export const RunEvent = z.discriminatedUnion("type", [
  z.object({ type: z.literal("session"), sessionId: z.string(), runtime: z.string(), model: z.string().optional() }),
  z.object({ type: z.literal("turn.started"), turnId: z.string() }),
  z.object({
    type: z.literal("turn.completed"),
    turnId: z.string(),
    status: z.enum(["completed", "failed", "interrupted"]),
    error: z.string().optional(),
  }),
  z.object({ type: z.literal("message.delta"), ...item, text: z.string() }),
  z.object({ type: z.literal("message"), ...item, role: z.enum(["assistant", "user"]), text: z.string() }),
  z.object({ type: z.literal("reasoning"), ...item, text: z.string() }),
  z.object({ type: z.literal("tool.call"), ...item, tool: z.string(), title: z.string(), input: z.unknown().optional() }),
  z.object({
    type: z.literal("tool.result"),
    ...item,
    ok: z.boolean(),
    output: z.string().optional(),
    exitCode: z.int().optional(),
  }),
  z.object({
    type: z.literal("file.change"),
    ...item,
    changes: z.array(
      z.object({ path: z.string(), kind: z.enum(["add", "modify", "delete"]), diff: z.string().optional() }),
    ),
  }),
  z.object({
    type: z.literal("approval.request"),
    approvalId: z.string().min(1),
    itemId: z.string().optional(),
    title: z.string(),
    detail: z.string().optional(),
  }),
  z.object({ type: z.literal("approval.resolved"), approvalId: z.string().min(1), decision: z.enum(["approve", "deny"]) }),
  z.object({ type: z.literal("input.accepted"), inputId: z.string().min(1) }),
  /** The CLI's to-do list (Claude Code TodoWrite, Codex plan updates); replaces the run plan. */
  z.object({ type: z.literal("plan"), items: RunPlan }),
  z.object({
    type: z.literal("status"),
    state: z.enum(["preparing", "running", "retrying", "stalled", "warning", "error"]),
    message: z.string().optional(),
  }),
]);
export type RunEvent = z.infer<typeof RunEvent>;
export type RunEventType = RunEvent["type"];

export const RunDoneStatus = z.enum(["succeeded", "failed", "cancelled", "timeout"]);
export type RunDoneStatus = z.infer<typeof RunDoneStatus>;

// ---------------------------------------------------------------------------
// Daemon to server
// ---------------------------------------------------------------------------

export const RunEventFrame = z.object({
  t: z.literal("run.event"),
  runId: RunId,
  seq: z.int().positive(),
  at: Millis,
  event: RunEvent,
});
export const RunDoneFrame = z.object({
  t: z.literal("run.done"),
  runId: RunId,
  seq: z.int().positive(),
  at: Millis,
  status: RunDoneStatus,
  exitCode: z.int().optional(),
  resultText: z.string().max(65_536 + 64).optional(),
  summary: z.string().max(400).optional(),
  error: z.string().max(4096 + 64).optional(),
  sessionId: z.string().optional(),
  usage: Usage.optional(),
  durationMs: z.int().nonnegative(),
});
export const RunSessionFrame = z.object({
  t: z.literal("run.session"),
  runId: RunId,
  seq: z.int().positive(),
  at: Millis,
  ok: z.boolean(),
  source: z.enum(["cli", "stream"]).optional(),
  bytes: z.int().nonnegative().optional(),
  gzipBytes: z.int().nonnegative().optional(),
  sha256: z.string().optional(),
  error: z.string().optional(),
});

export const MachineUpFrame = z.discriminatedUnion("t", [
  z.object({
    t: z.literal("hello"),
    protocol: z.literal(MACHINE_PROTOCOL_VERSION),
    machine: MachineInfo,
    clis: z.array(CliInfo).max(64),
    health: Health,
    runs: z.array(RunCursor),
  }),
  z.object({ t: z.literal("heartbeat"), at: Millis, health: Health, runs: z.array(RunCursor) }),
  z.object({ t: z.literal("run.accept"), runId: RunId }),
  z.object({
    t: z.literal("run.reject"),
    runId: RunId,
    reason: z.enum(["busy", "runtime_unavailable", "disk_low", "invalid", "draining"]),
    message: z.string().max(4096),
  }),
  RunEventFrame,
  RunDoneFrame,
  RunSessionFrame,
]);
export type MachineUpFrame = z.infer<typeof MachineUpFrame>;

// ---------------------------------------------------------------------------
// Server to daemon
// ---------------------------------------------------------------------------

export const RunAssignFrame = z.object({
  t: z.literal("run.assign"),
  runId: RunId,
  botMemberId: Id,
  groupId: Id,
  topicId: Id,
  runtime: z.string().min(1).max(64),
  prompt: z.string(),
  model: z.string().optional(),
  repo: RepoSpec.optional(),
  envAllowlist: z.array(z.string()),
  leaseMs: z.int().positive(),
  maxDurationMs: z.int().positive().optional(),
  sessionUpload: SessionUpload.optional(),
});
export type RunAssignFrame = z.infer<typeof RunAssignFrame>;

export const MachineDownFrame = z.discriminatedUnion("t", [
  z.object({
    t: z.literal("welcome"),
    protocol: z.literal(MACHINE_PROTOCOL_VERSION),
    machineId: Id,
    serverTime: Millis,
    runs: z.array(KnownRun),
  }),
  z.object({ t: z.literal("heartbeat.ack"), at: Millis }),
  RunAssignFrame,
  z.object({ t: z.literal("run.input"), runId: RunId, inputId: z.string().min(1), text: z.string(), authorId: Id.optional() }),
  z.object({ t: z.literal("run.approval"), runId: RunId, approvalId: z.string().min(1), decision: z.enum(["approve", "deny"]) }),
  z.object({ t: z.literal("run.cancel"), runId: RunId, reason: z.string().optional() }),
  z.object({ t: z.literal("ack"), runId: RunId, seq: Seq }),
  z.object({ t: z.literal("error"), error: z.object({ code: z.string(), message: z.string() }) }),
]);
export type MachineDownFrame = z.infer<typeof MachineDownFrame>;

/** One stored frame of a run, as `GET /v1/runs/:id/events` returns it. */
export const RunLogEntry = z.discriminatedUnion("t", [RunEventFrame, RunDoneFrame, RunSessionFrame]);
export type RunLogEntry = z.infer<typeof RunLogEntry>;
