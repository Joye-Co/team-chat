// Machine protocol: the daemon <-> MachineLink wire contract. See docs/machine-protocol.md.
// These frames never reach desktop clients, except the run log entries (`RunLogEntry`), which
// clients read with `GET /v1/runs/:id/events`.
import { z } from "zod";
import { CardRev } from "./card2.ts";
import { Id, Millis, Seq } from "./common.ts";
import { RunPlan } from "./plan.ts";

export const MACHINE_PROTOCOL_VERSION = 1 as const;

/** Run ids become folder names on the machine: `[A-Za-z0-9_-]{1,64}`. */
export const RunId = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/);
/** Machine and bot runner labels: 1..64 of `[A-Za-z0-9_.:-]`. */
export const Label = z.string().regex(/^[A-Za-z0-9_.:-]{1,64}$/);
export const MAX_LABELS = 20;
export const Labels = z.array(Label).max(MAX_LABELS);

/** A feature a daemon reports in `hello.machine.features`; see docs/machine-protocol.md. */
export const MachineFeature = z.string().min(1).max(64);
export const MAX_MACHINE_FEATURES = 32;
/** Features of the 1.1 daemon. The server sends a 1.1 frame or field only to a daemon that has its feature. */
export const MACHINE_FEATURES = ["pause", "answer", "aside", "acceptance", "interact", "output", "resume"] as const;

export const MachineInfo = z.object({
  /** The id configured on the machine (optional); the token is authoritative. */
  id: z.string().optional(),
  name: z.string().min(1).max(64),
  labels: z.array(z.string()).max(MAX_LABELS),
  daemonVersion: z.string().max(64),
  os: z.string().max(32),
  arch: z.string().max(32),
  maxParallelRuns: z.int().nonnegative(),
  /** 1.1: what this daemon supports, e.g. "pause", "answer", "aside", "acceptance". */
  features: z.array(MachineFeature).max(MAX_MACHINE_FEATURES).optional(),
  /** 1.1: a screen sharing address (e.g. `vnc://mac-mini.tailnet`) people use for 接管. */
  takeoverUrl: z.string().min(1).max(2048).optional(),
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
// Acceptance criteria (验收标准)
// ---------------------------------------------------------------------------

export const MAX_ACCEPTANCE_ITEMS = 20;
export const AcceptanceState = z.enum(["pending", "pass", "fail"]);
export type AcceptanceState = z.infer<typeof AcceptanceState>;
/** Proof of one criterion: a note, a file, a link or a 产出 version. */
export const AcceptanceEvidence = z.object({
  text: z.string().max(1000).optional(),
  fileId: Id.optional(),
  url: z.string().max(2048).regex(/^https?:\/\/[^\s]+$/).optional(),
  artifact: z.object({ artifactId: Id, version: z.int().positive() }).optional(),
});
export type AcceptanceEvidence = z.infer<typeof AcceptanceEvidence>;
export const AcceptanceItem = z.object({
  text: z.string().min(1).max(500),
  state: AcceptanceState,
  evidence: AcceptanceEvidence.optional(),
});
export type AcceptanceItem = z.infer<typeof AcceptanceItem>;

// ---------------------------------------------------------------------------
// Run events (the item model)
// ---------------------------------------------------------------------------

const item = { itemId: z.string().min(1), turnId: z.string().optional() };
/** What a `chat-run` output asks for; each one has the rules of the matching WebSocket frame. */
export const RunOutputOp = z.enum(["card.post", "card.patch", "interaction.resolve", "artifact.data", "artifact.reply"]);
export type RunOutputOp = z.infer<typeof RunOutputOp>;

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
  /** 1.1: the run's acceptance criteria and their state; replaces `BotRun.acceptance`. */
  z.object({ type: z.literal("acceptance"), items: z.array(AcceptanceItem).max(MAX_ACCEPTANCE_ITEMS) }),
  /** 1.1: the CLI is stopped (`paused: true`, after run.pause) or goes on (`false`, after run.resume). */
  z.object({ type: z.literal("paused"), paused: z.boolean(), by: Id.optional() }),
  /** 1.1 (feature "output", `chat-run`): the CLI asks the server to post or patch a card, resolve an event or answer a 产出. */
  z.object({ type: z.literal("output"), outputId: z.string().min(1).max(64), op: RunOutputOp, body: z.json() }),
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
  /** 1.1: "aside" = a read-only side chat run. Sent only to a daemon with feature "aside". */
  kind: z.string().min(1).max(32).optional(),
  /** 1.1 (feature "resume"): the CLI session of the topic's last run on this machine, to continue. */
  resumeSessionId: z.string().min(1).max(128).optional(),
});
export type RunAssignFrame = z.infer<typeof RunAssignFrame>;

/** A Card v2 answer given to a run (`run.input.answer`). */
export const RunAnswer = z.object({
  messageId: Id,
  rev: CardRev,
  actionId: z.string().min(1).max(64),
  inputs: z.record(z.string(), z.json()).optional(),
  editedValues: z.record(z.string(), z.string()).optional(),
  actorId: Id,
  /** 1.1 (interaction loop): the stored answer. */
  answerId: Id.optional(),
  askId: z.string().min(1).max(64).optional(),
  eventId: Id.optional(),
  event: z.string().min(1).max(64).optional(),
  blockId: z.string().min(1).max(64).optional(),
  value: z.json().optional(),
});
export type RunAnswer = z.infer<typeof RunAnswer>;
/** An event from an interactive 产出 page given to a run (`run.input.artifactEvent`). */
export const RunArtifactEvent = z.object({
  artifactId: Id,
  version: z.int().positive(),
  name: z.string().min(1).max(128),
  payload: z.json().optional(),
  actorId: Id,
});
export type RunArtifactEvent = z.infer<typeof RunArtifactEvent>;

/** Who made an answer or an event (a person). */
export const RunActor = z.object({ id: Id, name: z.string().min(1).max(64) });
/** `run.answer` (feature "interact"): a stored Card v2 answer for the run's CLI. */
export const RunAnswerFrame = z.object({
  t: z.literal("run.answer"),
  runId: RunId,
  inputId: z.string().min(1),
  source: z.object({ kind: z.literal("card"), messageId: Id, askId: z.string().min(1).max(64), answerId: Id, rev: CardRev }),
  actionId: z.string().min(1).max(64).optional(),
  blockId: z.string().min(1).max(64).optional(),
  event: z.string().min(1).max(64),
  value: z.json().optional(),
  inputs: z.record(z.string(), z.json()).optional(),
  editedValues: z.record(z.string(), z.string()).optional(),
  actor: RunActor,
  /** The interaction's event id, which `card.patch.cause` / `send.causeEventId` name. */
  eventId: Id.optional(),
  /** The block the answer names, as it was at `source.rev` (the CLI has no copy of the card). */
  block: z.json().optional(),
  /** A poll that shows counts only: the votes per option now; `value`/`inputs` are then absent. */
  tally: z.record(z.string(), z.int().nonnegative()).optional(),
});
export type RunAnswerFrame = z.infer<typeof RunAnswerFrame>;
export const RunInteractionSource = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("card"), messageId: Id, blockId: z.string().min(1).max(64).optional(), actionId: z.string().min(1).max(64).optional(), rev: CardRev }),
  z.object({ kind: z.literal("artifact"), artifactId: Id, version: z.int().positive(), eventId: Id, ask: z.boolean() }),
]);
/** `run.interaction` (feature "interact"): a live card event or a page event for the run's CLI. */
export const RunInteractionFrame = z.object({
  t: z.literal("run.interaction"),
  runId: RunId,
  inputId: z.string().min(1),
  source: RunInteractionSource,
  /** A card event name or the page event name. */
  event: z.string().min(1).max(128),
  value: z.json().optional(),
  actor: RunActor,
  /** The interaction's event id, which `card.patch.cause` / `send.causeEventId` name. */
  eventId: Id.optional(),
  /** Card events: the block (or the action) as it was at `source.rev`. */
  block: z.json().optional(),
});
export type RunInteractionFrame = z.infer<typeof RunInteractionFrame>;
/** `run.output.result` (feature "output"): the answer to a RunEvent `output`. */
export const RunOutputResultFrame = z.object({
  t: z.literal("run.output.result"),
  runId: RunId,
  outputId: z.string().min(1).max(64),
  ok: z.boolean(),
  result: z.json().optional(),
  error: z.object({ code: z.string().min(1), reason: z.string().min(1).max(64).optional(), message: z.string() }).optional(),
});
export type RunOutputResultFrame = z.infer<typeof RunOutputResultFrame>;

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
  z.object({
    t: z.literal("run.input"),
    runId: RunId,
    inputId: z.string().min(1),
    /** Always set: the text form of the input, for daemons without the feature of `answer`/`artifactEvent`. */
    text: z.string(),
    authorId: Id.optional(),
    /** 1.1 (feature "answer"): a Card v2 answer, as typed data. */
    answer: RunAnswer.optional(),
    /** 1.1 (feature "answer"): an event from an interactive 产出 page. */
    artifactEvent: RunArtifactEvent.optional(),
  }),
  z.object({ t: z.literal("run.approval"), runId: RunId, approvalId: z.string().min(1), decision: z.enum(["approve", "deny"]) }),
  z.object({ t: z.literal("run.cancel"), runId: RunId, reason: z.string().optional() }),
  /** 1.1 (feature "pause"): stop the CLI (SIGSTOP of its process group) for 接管; deadlines stop too. */
  z.object({ t: z.literal("run.pause"), runId: RunId, by: Id.optional() }),
  /** 1.1 (feature "pause"): go on after a pause; `note` becomes the CLI's next input when set. */
  z.object({ t: z.literal("run.resume"), runId: RunId, note: z.string().max(2000).optional() }),
  z.object({ t: z.literal("ack"), runId: RunId, seq: Seq }),
  z.object({ t: z.literal("error"), error: z.object({ code: z.string(), message: z.string() }) }),
  // 1.1 interaction loop
  RunAnswerFrame,
  RunInteractionFrame,
  RunOutputResultFrame,
]);
export type MachineDownFrame = z.infer<typeof MachineDownFrame>;

/** One stored frame of a run, as `GET /v1/runs/:id/events` returns it. */
export const RunLogEntry = z.discriminatedUnion("t", [RunEventFrame, RunDoneFrame, RunSessionFrame]);
export type RunLogEntry = z.infer<typeof RunLogEntry>;
