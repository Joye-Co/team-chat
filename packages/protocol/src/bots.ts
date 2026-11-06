// Bot runs for clients: runner-backed bot configuration, machines and their health, run
// status events in the group log and the run log. See docs/protocol.md, "Bot runs".
import { z } from "zod";
import { eventBase, Id, Millis, Name, Seq } from "./common.ts";
import { AcceptanceItem, CliInfo, Health, Labels, MAX_ACCEPTANCE_ITEMS, MachineFeature, RunLogEntry, Usage } from "./machine.ts";
import { RunPlan } from "./plan.ts";

/** `BotRun.kind` of a side chat run. */
export const RUN_KIND_ASIDE = "aside";

export const RunStatus = z.enum(["created", "running", "waiting", "blocked", "done", "failed", "cancelled"]);
export type RunStatus = z.infer<typeof RunStatus>;


/** One run of a runner-backed bot in a topic. */
export const BotRun = z.object({
  id: Id,
  botId: Id,
  groupId: Id,
  topicId: Id,
  /** The machine the run was given to; absent while none was picked. */
  machineId: Id.optional(),
  status: RunStatus,
  /** The member whose message started the run. */
  requestedBy: Id.optional(),
  createdAt: Millis,
  startedAt: Millis.optional(),
  endedAt: Millis.optional(),
  updatedAt: Millis,
  /** Short plain-text note: the result's first line, or why the run stopped. At most 280. */
  summary: z.string().max(280).optional(),
  /** The latest plan the bot or its runner set; absent when there is none. */
  plan: RunPlan.optional(),
  // 1.1 fields. Absent on older runs.
  /** Tokens and cost the CLI reported in `run.done`. */
  usage: Usage.optional(),
  /** Run time the daemon reported in `run.done`. */
  durationMs: z.int().nonnegative().optional(),
  /** The message (usually a Card v2 with intent "done") the bot posted as the result. */
  resultMessageId: Id.optional(),
  /** 验收标准 the run reports (run event `acceptance`); replaced as a whole. */
  acceptance: z.array(AcceptanceItem).max(MAX_ACCEPTANCE_ITEMS).optional(),
  /** Present while a person has paused the run (接管): who. Absent = not paused. */
  pausedBy: Id.optional(),
  /** "aside": a read-only side chat run (单独问). Absent = a normal run. Unknown values read as normal. */
  kind: z.string().min(1).max(32).optional(),
  /** 1.1: the run this one retries (`POST /v1/runs/:id/retry`). */
  retryOf: Id.optional(),
  /** 1.1 (排队): set while a `created` run with no `machineId` waits for a free machine. */
  queuedAt: Millis.optional(),
  /** 1.1 (排队): 1-based place among the bot's queued runs, oldest first; with `queuedAt`. */
  queuePosition: z.int().positive().optional(),
});
export type BotRun = z.infer<typeof BotRun>;

/** Stored group event; `actorId` = the bot. Sent when a run is created and when its status changes. */
export const RunUpdatedEvent = z.object({
  ...eventBase,
  type: z.literal("run.updated"),
  data: z.object({ run: BotRun }),
});

/** `GET /v1/runs/:id/events?afterSeq=&limit=` (member of the run's group). */
export const RunEventsResponse = z.object({
  run: BotRun,
  entries: z.array(RunLogEntry),
  hasMore: z.boolean(),
  /** Highest seq stored for this run. */
  lastSeq: Seq,
});
export type RunEventsResponse = z.infer<typeof RunEventsResponse>;

/** `GET /v1/runs/:id` */
export const RunResponse = BotRun;

/**
 * `POST /v1/runs` (a bot that is not runner-backed, member of the group): starts a run of the
 * calling bot in the topic, or returns its active run there. `plan`, when given, replaces the plan.
 */
export const StartRunRequest = z.object({ groupId: Id, topicId: Id, plan: RunPlan.optional() });
export type StartRunRequest = z.infer<typeof StartRunRequest>;

/** `PUT /v1/runs/:id/plan` (the run's bot): replaces the whole plan of an active run. */
export const SetRunPlanRequest = z.object({ plan: RunPlan });
export type SetRunPlanRequest = z.infer<typeof SetRunPlanRequest>;

/** `PUT /v1/runs/:id/acceptance` (the run's bot, 1.1): replaces the 验收标准 of an active run; [] clears them. */
export const SetRunAcceptanceRequest = z.object({ items: z.array(AcceptanceItem).max(MAX_ACCEPTANCE_ITEMS) });
export type SetRunAcceptanceRequest = z.infer<typeof SetRunAcceptanceRequest>;

/** `POST /v1/runs/:id/resume` (1.1): `note` becomes the CLI's next input when set. */
export const ResumeRunRequest = z.object({ note: z.string().max(2000).optional() });
export type ResumeRunRequest = z.infer<typeof ResumeRunRequest>;

/** `POST /v1/runs/:id/end` (the run's bot, runs started with `POST /v1/runs` only). */
export const EndRunRequest = z.object({
  status: z.enum(["done", "failed", "cancelled"]),
  summary: z.string().max(280).optional(),
});
export type EndRunRequest = z.infer<typeof EndRunRequest>;

// ---------------------------------------------------------------------------
// Runner-backed bots
// ---------------------------------------------------------------------------

export const RunnerRepo = z
  .object({
    url: z.string().min(1).max(2048).optional(),
    path: z.string().min(1).max(1024).optional(),
  })
  .refine((r) => (r.url === undefined) !== (r.path === undefined), { message: "repo needs url or path, not both" });

const EnvName = z.string().regex(/^[A-Za-z_][A-Za-z0-9_]{0,127}$/);

/** `PUT /v1/bots/:id/runner` (owner). Makes the bot runner-backed. */
export const SetBotRunnerRequest = z.object({
  /** A runtime name that machines report in `hello.clis[].runtime`, e.g. "codex" or "claude". */
  runtime: z.string().min(1).max(64),
  /** A machine qualifies when it has every one of these labels. */
  labels: Labels.optional(),
  repo: RunnerRepo.optional(),
  baseBranch: z.string().min(1).max(200).optional(),
  model: z.string().min(1).max(200).optional(),
  envAllowlist: z.array(EnvName).max(50).optional(),
  /** Absent = true. */
  enabled: z.boolean().optional(),
  /** 1.2.2: the one machine that runs this bot (labels then do not count). Absent keeps the
   * stored machine; null lets any machine with the labels run it. */
  machineId: Id.nullable().optional(),
});
export type SetBotRunnerRequest = z.infer<typeof SetBotRunnerRequest>;

/** `GET` and `PUT /v1/bots/:id/runner` */
export const BotRunner = z.object({
  botId: Id,
  runtime: z.string().min(1).max(64),
  labels: Labels,
  repo: RunnerRepo.optional(),
  baseBranch: z.string().optional(),
  model: z.string().optional(),
  envAllowlist: z.array(z.string()),
  enabled: z.boolean(),
  updatedAt: Millis,
  /** 1.2.2: the one machine that runs this bot; absent = any machine with the labels. */
  machineId: Id.optional(),
});
export type BotRunner = z.infer<typeof BotRunner>;

// ---------------------------------------------------------------------------
// Machines
// ---------------------------------------------------------------------------

export const Machine = z.object({
  id: Id,
  name: Name,
  /** Owner labels and the labels the daemon reports, merged. */
  labels: z.array(z.string()),
  createdBy: Id,
  createdAt: Millis,
  revoked: z.boolean().optional(),
  /** A daemon link is open now. */
  connected: z.boolean(),
  /** Accepts new runs: connected, recent heartbeat, enough free disk. */
  healthy: z.boolean(),
  lastHeartbeatAt: Millis.optional(),
  daemonVersion: z.string().optional(),
  os: z.string().optional(),
  arch: z.string().optional(),
  maxParallelRuns: z.int().nonnegative().optional(),
  health: Health.optional(),
  clis: z.array(CliInfo).optional(),
  /** Ids of the runs this machine holds now. */
  runningRuns: z.array(Id),
  /** What the daemon supports (`hello.machine.features`, 1.1). Owners only. */
  features: z.array(MachineFeature).optional(),
  /** Screen sharing address for 接管 (`hello.machine.takeoverUrl`). Owners and run requesters only. */
  takeoverUrl: z.string().min(1).max(2048).optional(),
});
export type Machine = z.infer<typeof Machine>;

/** `POST /v1/machines` (owner) */
export const CreateMachineRequest = z.object({ name: Name, labels: Labels.optional() });
export type CreateMachineRequest = z.infer<typeof CreateMachineRequest>;
/** The token (`tm_` + 43 base64url) is shown only in this response. */
export const CreateMachineResponse = z.object({ machine: Machine, token: z.string().min(1) });
export type CreateMachineResponse = z.infer<typeof CreateMachineResponse>;
/** `GET /v1/machines` (owner) */
export const MachinesResponse = z.array(Machine);

