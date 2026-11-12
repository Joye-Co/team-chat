// Machine protocol: the daemon <-> MachineLink wire contract. See docs/machine-protocol.md.
// These frames never reach desktop clients, except the run log entries (`RunLogEntry`), which
// clients read with `GET /v1/runs/:id/events`.
import { z } from "zod";
import { CardRev } from "./card2.ts";
import { Id, Millis, Seq, utf8Bytes } from "./common.ts";
import { RunPlan } from "./plan.ts";
import { RunGithub } from "./github.ts";
import { RunCode } from "./relay.ts";
import { HandCommandId, MAX_HAND_COMMAND_CHARS, MAX_HAND_OUTPUT_BYTES, RunHandback } from "./takeover.ts";

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
export const MACHINE_FEATURES = ["pause", "answer", "aside", "acceptance", "interact", "output", "resume", "context", "effort", "gates", "accounts"] as const;
/** Usable features (docs/machine-protocol.md): `run.assign.github` and the GitHub ops. */
export const MACHINE_FEATURE_GITHUB = "github";
/** Usable: `run.pause.hold`, `run.command`, `run.command.cancel` and the `hand.*` run events. */
export const MACHINE_FEATURE_TAKEOVER = "takeover";
/** Usable: with `takeover`, a person's commands run as another user than the run (VM runs). */
export const MACHINE_FEATURE_TAKEOVER_ISOLATED = "takeover-isolated";
/** Usable: the `homes` metrics and `home.clear` (VM hosts: each AI member's home folder). */
export const MACHINE_FEATURE_HOMES = "homes";

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
  /** 1.1: a screen sharing address (e.g. `vnc://<host>`); not the usable take-over (feature "takeover"). */
  takeoverUrl: z.string().min(1).max(2048).optional(),
  /** Feature "accounts": the machine's X25519 public key (base64, 32 bytes); API keys are sealed to it. */
  publicKey: z.string().regex(/^[A-Za-z0-9+/]{43}=$/).optional(),
  /** Usable (feature "github" requires it): where runs execute, "host" | "vm" (`MACHINE_RUNS_IN`). */
  runsIn: z.string().min(1).max(16).optional(),
});
export type MachineInfo = z.infer<typeof MachineInfo>;

export const CliKind = z.enum(["codex", "claude", "acp", "pty"]);
/** A thinking effort name (思考强度), as the CLI spells it: "low", "high", "max"… */
export const Effort = z.string().regex(/^[a-z][a-z0-9_-]{0,31}$/);
export const MAX_CLI_MODELS = 50;
export const MAX_EFFORTS = 10;

/** A model a CLI can run (machine-protocol.md, "Models and effort"). */
export const CliModel = z.object({
  /** What `run.assign.model` and `SetBotRunnerRequest.model` carry. */
  id: z.string().min(1).max(200),
  /** The CLI's display name. */
  name: z.string().max(200).optional(),
  /** The efforts this model takes; absent = the CLI's `efforts`. */
  efforts: z.array(Effort).max(MAX_EFFORTS).optional(),
  defaultEffort: Effort.optional(),
  /** The CLI's default model (what runs without `model`). */
  isDefault: z.boolean().optional(),
});
export type CliModel = z.infer<typeof CliModel>;

export const CliInfo = z.object({
  /** The runtime name configured on the machine; `run.assign.runtime` names one. */
  runtime: z.string().min(1).max(64),
  kind: CliKind,
  command: z.string().max(1024),
  version: z.string().max(200).optional(),
  available: z.boolean(),
  /** Models the CLI can run (feature "effort"); absent = unknown, any id may be typed. */
  models: z.array(CliModel).max(MAX_CLI_MODELS).optional(),
  /** Efforts the CLI takes (feature "effort"); absent or empty = no effort setting. */
  efforts: z.array(Effort).max(MAX_EFFORTS).optional(),
  /** Feature "gates": runs of this CLI stop at `run.assign.gates` matches; absent = they cannot. */
  gates: z.boolean().optional(),
  /** Usable (feature "resume"): runs of this CLI continue `run.assign.resumeSessionId`; absent = only kinds "claude" and "codex" do. */
  resume: z.boolean().optional(),
  /** Usable (feature "aside"): this CLI has a read-only mode for side chats; absent = only kinds "claude" and "codex" do. */
  aside: z.boolean().optional(),
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

// ---------------------------------------------------------------------------
// Machine metrics (heartbeat.metrics, docs/machine-protocol.md "Metrics")
// ---------------------------------------------------------------------------

export const MAX_METRIC_RUNS = 64;
/** `VmRunMetrics.phase` values; read as any short string so a newer daemon's phase never fails a heartbeat. */
export const VM_PHASES = ["booting", "running", "paused"] as const;
/** One run's VM (or local process tree) as the daemon measured it. */
export const VmRunMetrics = z.object({
  runId: RunId,
  /** "booting" | "running" | "paused" */
  phase: z.string().min(1).max(32),
  startedAt: Millis,
  /** Start of the VM to a connected guest agent, once it booted. */
  bootMs: z.int().nonnegative().optional(),
  /** CPU since the previous sample, in cores (1.0 = one thread busy). */
  cpuCores: z.number().nonnegative().optional(),
  /** Resident memory of the VM (the guest pages it touched). */
  memBytes: z.int().nonnegative().optional(),
  /** Blocks the run's scratch disk uses. */
  scratchBytes: z.int().nonnegative().optional(),
});
export type VmRunMetrics = z.infer<typeof VmRunMetrics>;
/** `CliLogin.state` values. */
export const CLI_LOGIN_STATES = ["logged_in", "logged_out", "unknown"] as const;
/** Whether a runtime's CLI has a credential on the machine; never the credential itself. */
export const CliLogin = z.object({
  runtime: z.string().min(1).max(64),
  /** "logged_in" | "logged_out" | "unknown" */
  state: z.string().min(1).max(32),
  /** "subscription" | "api_key" when known. */
  method: z.string().min(1).max(32).optional(),
  /** Feature "accounts": the signed-in account and plan, when the CLI says. */
  email: z.string().max(320).optional(),
  plan: z.string().max(64).optional(),
  /**
   * Usable: `true` when a `logged_out` state is certain (the CLI's own status check, run with the
   * env its runs get, said so, or a run just failed because it is not signed in). Absent = from
   * the credential files only, or an older daemon. Only a confirmed `logged_out` keeps runs away.
   */
  confirmed: z.boolean().optional(),
});
export type CliLogin = z.infer<typeof CliLogin>;
/** VM boot times of the last (at most 50) runs. */
export const BootStats = z.object({
  avgMs: z.int().nonnegative(),
  maxMs: z.int().nonnegative(),
  samples: z.int().nonnegative(),
});
export type BootStats = z.infer<typeof BootStats>;
export const MAX_HOMES = 500;
/** Usable, feature "homes": one AI member's kept home folder on a VM host. */
export const HomeUse = z.object({
  memberId: Id,
  /** Bytes the home takes on the host disk. */
  usedBytes: z.int().nonnegative(),
  /** Its fixed size; absent = no limit. */
  sizeBytes: z.int().nonnegative().optional(),
});
export type HomeUse = z.infer<typeof HomeUse>;
/** Host and VM numbers the daemon sends with each heartbeat; every number is optional (a platform may not have it). */
// ---------------------------------------------------------------------------
// Accounts and API keys (feature "accounts", docs/machine-protocol.md "Accounts and keys")
// ---------------------------------------------------------------------------

/** An API key's name: an environment variable name. */
export const SecretName = z.string().regex(/^[A-Z_][A-Z0-9_]{0,63}$/);
/** A key the machine holds, as it reports it: never the value, only its last 4 characters. */
export const SecretInfo = z.object({
  name: SecretName,
  last4: z.string().max(4),
  /** AI members whose runs get it; empty = every run. */
  members: z.array(Id).max(100),
  updatedAt: Millis,
});
export type SecretInfo = z.infer<typeof SecretInfo>;
/** One usage window of a subscription (Claude 5 h / week, Codex primary / secondary). */
export const UsageWindow = z.object({
  /** "session" | "week" | "week_model" | "primary" | "secondary" | another short word. */
  kind: z.string().min(1).max(32),
  /** What the CLI calls it, e.g. "Current week (Opus)". */
  label: z.string().max(80).optional(),
  usedPct: z.number().min(0).max(100),
  resetsAt: Millis.optional(),
});
export type UsageWindow = z.infer<typeof UsageWindow>;
export const CliUsage = z.object({
  runtime: z.string().min(1).max(64),
  /** When the daemon read it (machine clock). */
  at: Millis,
  windows: z.array(UsageWindow).max(8),
});
export type CliUsage = z.infer<typeof CliUsage>;
/** `account.login.status.state` values. */
export const LOGIN_STATES = ["starting", "open_url", "device_code", "checking", "done", "failed", "cancelled"] as const;
export const LoginStatusFrame = z.object({
  t: z.literal("account.login.status"),
  loginId: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/),
  runtime: z.string().min(1).max(64),
  /** "starting" | "open_url" (open url, paste the code) | "device_code" (open url, enter userCode) | "checking" | "done" | "failed" | "cancelled" */
  state: z.string().min(1).max(32),
  url: z.string().max(4096).optional(),
  userCode: z.string().max(64).optional(),
  account: z.object({ email: z.string().max(320).optional(), plan: z.string().max(64).optional() }).optional(),
  error: z.string().max(1000).optional(),
});
export type LoginStatusFrame = z.infer<typeof LoginStatusFrame>;

export const MachineMetrics = z.object({
  /** When the daemon took the sample (machine clock). */
  at: Millis,
  uptimeSecs: z.int().nonnegative().optional(),
  /** Whole-host CPU busy since the previous sample, 0-100. */
  cpuPct: z.number().min(0).max(100).optional(),
  cpuThreads: z.int().nonnegative().optional(),
  memTotalBytes: z.int().nonnegative().optional(),
  memAvailableBytes: z.int().nonnegative().optional(),
  /** Bytes per second over the non-loopback, non-VM interfaces since the previous sample. */
  netRxBps: z.number().nonnegative().optional(),
  netTxBps: z.number().nonnegative().optional(),
  /** VM slots of the machine (`[vm]`), absent without VMs. */
  vmSlots: z.int().nonnegative().optional(),
  boot: BootStats.optional(),
  runs: z.array(VmRunMetrics).max(MAX_METRIC_RUNS),
  logins: z.array(CliLogin).max(64).optional(),
  /** Feature "accounts": the API keys the machine holds (names and last 4 only). */
  secrets: z.array(SecretInfo).max(100).optional(),
  /** Feature "accounts": subscription usage windows, read every 10 minutes. */
  usage: z.array(CliUsage).max(16).optional(),
  /** Usable, feature "homes": each AI member's home folder on a VM host. */
  homes: z.array(HomeUse).max(MAX_HOMES).optional(),
});
export type MachineMetrics = z.infer<typeof MachineMetrics>;

export const RunCursor = z.object({
  runId: RunId,
  lastSeq: Seq,
  done: z.boolean(),
  /**
   * Usable (盯 1.4): the processes the CLI started (a build, a test run) used CPU since the last
   * heartbeat, so a long step is working, not stuck. Absent = not known (older daemons).
   */
  active: z.boolean().optional(),
});
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

/**
 * What one model of a run used. `inputTokens` leaves out cached input: cache reads and writes are
 * their own counts, priced apart. `costUsd` is the CLI's own figure for this model (Claude Code
 * reports one); absent, the server prices the tokens from its price list.
 */
export const ModelUsage = z.object({
  model: z.string().min(1).max(200),
  inputTokens: z.int().nonnegative(),
  outputTokens: z.int().nonnegative(),
  cacheReadTokens: z.int().nonnegative().optional(),
  cacheWriteTokens: z.int().nonnegative().optional(),
  costUsd: z.number().nonnegative().optional(),
});
export type ModelUsage = z.infer<typeof ModelUsage>;

export const Usage = z.object({
  inputTokens: z.int().nonnegative(),
  outputTokens: z.int().nonnegative(),
  costUsd: z.number().nonnegative().optional(),
  /** Per model the run used (daemons from 1.3 on); older daemons send the totals only. */
  models: z.array(ModelUsage).max(32).optional(),
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
/** `desk.note` (1.1): the bot's desk note in the run's group, body `{ text }` (as `PUT /v1/desk-note`). */
/** `run.report` (usable): how the run ended in the CLI's own words, body `RunReport`. */
export const RunOutputOp = z.enum(["card.post", "card.patch", "interaction.resolve", "artifact.data", "artifact.reply", "desk.note", "run.report"]);
export type RunOutputOp = z.infer<typeof RunOutputOp>;

/** Known `RunReport.state` values: done, blocked (needs something to go on), waiting (for a person's answer). */
export const RUN_REPORT_STATES = ["done", "blocked", "waiting"] as const;
export const MAX_RUN_REPORT_CHARS = 2000;
/**
 * The body of the `run.report` output (`chat-run report`, usable): the CLI says how its work
 * ended. The latest report counts when the run ends (docs/machine-protocol.md, "Run report").
 */
export const RunReport = z.object({
  state: z.enum(RUN_REPORT_STATES),
  text: z.string().trim().min(1).max(MAX_RUN_REPORT_CHARS),
});
export type RunReport = z.infer<typeof RunReport>;

export const RunEvent = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("session"),
    sessionId: z.string(),
    runtime: z.string(),
    model: z.string().optional(),
    /** Usable: the 思考强度 the CLI runs with; absent = its default or not known. */
    effort: z.string().optional(),
  }),
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
    /** Feature "gates": the gate kind the command matched (`RunGate.kind`). */
    kind: z.string().min(1).max(32).optional(),
    /** Feature "gates": the command the CLI was about to run (redacted, <= 4000). */
    command: z.string().max(4000).optional(),
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
  // Usable (feature "github"): git or gh used the person's GitHub, or could not.
  z.object({ type: z.literal("github"), state: z.string().min(1).max(32), login: z.string().min(1).max(100).optional() }),
  // Usable (feature "takeover"): a person's command while holding the run. Not the AI member's progress.
  z.object({ type: z.literal("hand.command"), commandId: HandCommandId, command: z.string().min(1).max(MAX_HAND_COMMAND_CHARS), by: Id }),
  z.object({ type: z.literal("hand.output"), commandId: HandCommandId, text: z.string().refine((t) => utf8Bytes(t) <= MAX_HAND_OUTPUT_BYTES, { message: "hand.output text is at most 16 KiB" }) }),
  z.object({
    type: z.literal("hand.result"),
    commandId: HandCommandId,
    ok: z.boolean(),
    exitCode: z.int().optional(),
    ms: z.int().nonnegative(),
    timedOut: z.boolean().optional(),
    /** "not_held" | "busy" | "unsupported" (`HAND_ERRORS`). */
    error: z.string().min(1).max(32).optional(),
  }),
  z.object({ type: z.literal("hand.summary"), ...RunHandback.shape }),
]);
export type RunEvent = z.infer<typeof RunEvent>;
export type RunEventType = RunEvent["type"];

export const RunDoneStatus = z.enum(["succeeded", "failed", "cancelled", "timeout"]);
export type RunDoneStatus = z.infer<typeof RunDoneStatus>;

/**
 * Usable: why a run failed, as the daemon sorted it (machine-protocol.md, "Failure kinds"). The
 * kind is an open string: an unknown one reads as `other`.
 */
export const RUN_FAILURE_KINDS = ["signed_out", "limit", "model", "network", "not_installed", "other"] as const;
export type RunFailureKind = (typeof RUN_FAILURE_KINDS)[number];
export const RunFailure = z.object({
  kind: z.string().min(1).max(32),
  /** `limit`: when the limit resets (ms since the epoch), when the CLI said. */
  resetsAt: Millis.optional(),
});
export type RunFailure = z.infer<typeof RunFailure>;

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
  /** Usable: the run's branch, its head and whether it is pushed; absent = not known. */
  code: RunCode.optional(),
  /** Usable: with `status: "failed"`, why it failed; absent = not sorted (older daemons). */
  failure: RunFailure.optional(),
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
  z.object({
    t: z.literal("heartbeat"),
    at: Millis,
    health: Health,
    runs: z.array(RunCursor),
    /** Host and VM metrics (daemons since 1.2.7); old servers ignore it. */
    metrics: MachineMetrics.optional(),
  }),
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
  LoginStatusFrame,
]);
export type MachineUpFrame = z.infer<typeof MachineUpFrame>;

// ---------------------------------------------------------------------------
// Server to daemon
// ---------------------------------------------------------------------------

/**
 * One gated action kind of a run (feature "gates"): before a tool runs a shell command that a
 * pattern matches (case-insensitive regex, the subset JavaScript and Rust share), the daemon asks
 * for approval (`approval.request` with this `kind`) and waits.
 */
export const RunGate = z.object({
  kind: z.string().regex(/^[A-Za-z][A-Za-z0-9_-]{0,31}$/),
  patterns: z.array(z.string().min(1).max(200)).min(1).max(64),
});
export type RunGate = z.infer<typeof RunGate>;
export const MAX_RUN_GATES = 16;

export const RunAssignFrame = z.object({
  t: z.literal("run.assign"),
  runId: RunId,
  botMemberId: Id,
  groupId: Id,
  topicId: Id,
  runtime: z.string().min(1).max(64),
  prompt: z.string(),
  model: z.string().optional(),
  /** Thinking effort (思考强度); sent only to a daemon with feature "effort". */
  effort: Effort.optional(),
  repo: RepoSpec.optional(),
  envAllowlist: z.array(z.string()),
  leaseMs: z.int().positive(),
  maxDurationMs: z.int().positive().optional(),
  sessionUpload: SessionUpload.optional(),
  /** 1.1: "aside" = a read-only side chat run. Sent only to a daemon with feature "aside". */
  kind: z.string().min(1).max(32).optional(),
  /** 1.1 (feature "resume"): the CLI session of the topic's last run on this machine, to continue. */
  resumeSessionId: z.string().min(1).max(128).optional(),
  /** Feature "gates": the action kinds the run stops at for a person's approval. */
  gates: z.array(RunGate).max(MAX_RUN_GATES).optional(),
  /** Usable, feature "github": whose GitHub the run uses and how. */
  github: RunGithub.optional(),
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
  z.object({
    t: z.literal("run.approval"),
    runId: RunId,
    approvalId: z.string().min(1),
    decision: z.enum(["approve", "deny"]),
    /** Feature "gates": "run" = 这件事里都同意, later matches of the same kind in this run pass. */
    scope: z.enum(["run"]).optional(),
  }),
  z.object({ t: z.literal("run.cancel"), runId: RunId, reason: z.string().optional() }),
  /** 1.1 (feature "pause"): stop the CLI (SIGSTOP of its process group) for 接管; deadlines stop too. */
  z.object({
    t: z.literal("run.pause"),
    runId: RunId,
    by: Id.optional(),
    /** Usable, feature "takeover": "takeover" = a person takes over (`RUN_HOLD_KINDS`); absent = a pause. */
    hold: z.string().min(1).max(32).optional(),
  }),
  /** 1.1 (feature "pause"): go on after a pause; `note` becomes the CLI's next input when set. */
  z.object({ t: z.literal("run.resume"), runId: RunId, note: z.string().max(2000).optional() }),
  z.object({ t: z.literal("ack"), runId: RunId, seq: Seq }),
  z.object({ t: z.literal("error"), error: z.object({ code: z.string(), message: z.string() }) }),
  // 1.1 interaction loop
  RunAnswerFrame,
  RunInteractionFrame,
  RunOutputResultFrame,
  // feature "accounts"
  z.object({
    t: z.literal("account.login"),
    loginId: z.string().regex(/^[A-Za-z0-9_-]{1,64}$/),
    runtime: z.string().min(1).max(64),
    op: z.enum(["start", "code", "cancel"]),
    /** op "code": what the person pasted (Claude's code#state). */
    code: z.string().min(1).max(2048).optional(),
  }),
  z.object({
    t: z.literal("secret.set"),
    name: SecretName,
    /** base64 of a libsodium sealed box (X25519 + XSalsa20-Poly1305) to `machine.publicKey`. */
    sealed: z.string().min(1).max(16384),
    members: z.array(Id).max(100),
  }),
  z.object({ t: z.literal("secret.delete"), name: SecretName }),
  // Usable, feature "homes": empty one AI member's home folder; sent only while connected.
  z.object({ t: z.literal("home.clear"), memberId: Id }),
  // Usable, feature "takeover": a command of the person holding the run; sent only while connected.
  z.object({
    t: z.literal("run.command"),
    runId: RunId,
    commandId: HandCommandId,
    command: z.string().min(1).max(MAX_HAND_COMMAND_CHARS),
    by: Id,
  }),
  z.object({ t: z.literal("run.command.cancel"), runId: RunId, commandId: HandCommandId }),
]);
export type MachineDownFrame = z.infer<typeof MachineDownFrame>;

/** One stored frame of a run, as `GET /v1/runs/:id/events` returns it. */
export const RunLogEntry = z.discriminatedUnion("t", [RunEventFrame, RunDoneFrame, RunSessionFrame]);
export type RunLogEntry = z.infer<typeof RunLogEntry>;
