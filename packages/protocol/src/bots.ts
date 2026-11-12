// Bot runs for clients: runner-backed bot configuration, machines and their health, run
// status events in the group log and the run log. See docs/protocol.md, "Bot runs".
import { z } from "zod";
import { eventBase, Id, Millis, Name, Seq } from "./common.ts";
import { AcceptanceItem, CliInfo, Effort, Health, Labels, MAX_ACCEPTANCE_ITEMS, MachineFeature, MachineGithub, RunFailure, RunLogEntry, Usage } from "./machine.ts";
import { RunPlan } from "./plan.ts";
import { Artifact, ArtifactVersion } from "./files.ts";
import { GroupDecision, RecordedDecision } from "./card2.ts";
import { RunCode } from "./relay.ts";
import { runPhaseFields } from "./takeover.ts";
import { runSuperviseFields } from "./supervise.ts";

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
  // Usable fields. Absent on older runs and servers.
  /** The person the run works for (交代人): a hand-over, relay step, follow-up of a wait or retry
   * keeps the first one; absent = no person (a hook), a private card answer (the answerer stays
   * anonymous), or a run from before (read `requestedBy`). */
  requestedFor: Id.optional(),
  /** The piece of work (chain) the run belongs to: its root run and every hand-over and relay step. */
  chainId: Id.optional(),
  /** Where the run left its code (`run.done.code`). */
  code: RunCode.optional(),
  /** The model the CLI really ran with (the `session` run event). */
  model: z.string().min(1).max(200).optional(),
  /** The 思考强度 the CLI really ran with (the `session` run event); absent = its default. */
  effort: z.string().min(1).max(32).optional(),
  /** Why a failed run failed (`run.done.failure`); absent = not known. */
  failure: RunFailure.optional(),
  ...runPhaseFields,
  ...runSuperviseFields,
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

// ---------------------------------------------------------------------------
// Run context (docs/protocol.md, "Run context"; machine-protocol.md, "Topic context")
// ---------------------------------------------------------------------------

const Count = z.int().nonnegative();

/** What a run's prompt carried from its topic. */
export const RunContextManifest = z.object({
  /** The prompt continued the CLI session of the topic's last run: only what was new. */
  resumed: z.boolean(),
  /** Characters of topic lines. */
  chars: Count,
  firstSeq: Seq.optional(),
  lastSeq: Seq.optional(),
  /** Messages carried. */
  messages: Count,
  /** Messages left out (readable with chat-run). */
  omitted: Count,
  files: Count,
  cards: Count,
  refs: Count,
});
export type RunContextManifest = z.infer<typeof RunContextManifest>;

/** `GET /v1/runs/:id/context` (owner). */
export const RunContext = z.object({ prompt: z.string(), manifest: RunContextManifest });
export type RunContext = z.infer<typeof RunContext>;

/** One message as a run reads it: `text` is its compact line (Run context). */
export const ContextMessage = z.object({
  id: Id,
  seq: Seq,
  authorId: Id,
  authorName: z.string(),
  at: Millis,
  text: z.string(),
});
export type ContextMessage = z.infer<typeof ContextMessage>;

/** `GET /v1/machine/runs/:id/topic/messages` */
export const TopicHistoryResponse = z.object({ messages: z.array(ContextMessage).max(100), hasMore: z.boolean() });
export type TopicHistoryResponse = z.infer<typeof TopicHistoryResponse>;

/** `GET /v1/machine/runs/:id/topic/messages/:messageId` */
export const TopicMessageResponse = z.object({ message: ContextMessage });
export type TopicMessageResponse = z.infer<typeof TopicMessageResponse>;

export const ContextHit = z.object({
  messageId: Id,
  topicId: Id,
  topicTitle: z.string().optional(),
  seq: Seq,
  authorName: z.string(),
  at: Millis,
  snippet: z.string(),
});
export type ContextHit = z.infer<typeof ContextHit>;

/** The front desk's work order (交代单) for the executor. */
export const WorkBrief = z.object({
  goal: z.string().min(1).max(1000),
  doneWhen: z.array(z.string().min(1).max(300)).max(10),
  notes: z.string().max(2000).optional(),
});
export type WorkBrief = z.infer<typeof WorkBrief>;

/** One front desk decision (docs/protocol.md, "Front desk"). */
export const FrontDeskDecision = z.object({
  messageId: Id,
  topicId: Id,
  authorId: Id,
  action: z.enum(["reply", "dispatch", "ask", "skip"]),
  reason: z.string(),
  confidence: z.number().min(0).max(1),
  reply: z.string().optional(),
  brief: WorkBrief.optional(),
  /** Why the desk did not take the model's answer as it was (it then dispatched). */
  fallback: z.string().optional(),
  model: z.string().optional(),
  ms: Count,
  inputTokens: Count,
  outputTokens: Count,
  replyMessageId: Id.optional(),
  at: Millis,
});
export type FrontDeskDecision = z.infer<typeof FrontDeskDecision>;

/** `GET /v1/bots/:id/decisions?groupId=&topicId=` (owner), newest first. */
export const FrontDeskDecisionsResponse = z.object({ decisions: z.array(FrontDeskDecision).max(100) });
export type FrontDeskDecisionsResponse = z.infer<typeof FrontDeskDecisionsResponse>;

/** `GET /v1/bots/:id/metrics?days=` (owner): what the front desk and the runs did lately. */
export const BotMetrics = z.object({
  since: Millis,
  frontDesk: z.object({
    decisions: Count,
    byAction: z.object({ reply: Count, dispatch: Count, ask: Count, skip: Count }),
    fallbacks: Count,
    p50Ms: Count,
    p95Ms: Count,
    inputTokens: Count,
    outputTokens: Count,
  }),
  runs: z.object({
    total: Count,
    done: Count,
    failed: Count,
    blocked: Count,
    cancelled: Count,
    /** Restarted by supervision (盯活). */
    restarted: Count,
    /** Continued by supervision after a promise without work. */
    continued: Count,
    interventions: Count,
    p50Ms: Count,
    p95Ms: Count,
  }),
});
export type BotMetrics = z.infer<typeof BotMetrics>;

/** `GET /v1/machine/runs/:id/search` */
export const ContextSearchResponse = z.object({ hits: z.array(ContextHit).max(50) });
export type ContextSearchResponse = z.infer<typeof ContextSearchResponse>;

/** `GET /v1/machine/runs/:id/artifacts?scope=topic|group`: the 产出 of the run's topic or group, newest first. */
export const RunArtifactsResponse = z.object({ artifacts: z.array(Artifact) });
export type RunArtifactsResponse = z.infer<typeof RunArtifactsResponse>;

/** Characters of a 产出's entry text that `artifact get` carries. */
export const MAX_RUN_ARTIFACT_TEXT = 200_000;

/**
 * `GET /v1/machine/runs/:id/artifacts/:artifactId?version=`: one 产出 of the run's group with its
 * versions, newest first; `version` is the one read (default the current one) and `text` its entry
 * file when that is text (cut to `MAX_RUN_ARTIFACT_TEXT`, then `truncated`).
 */
export const RunArtifactResponse = z.object({
  artifact: Artifact,
  versions: z.array(ArtifactVersion),
  version: z.int().positive(),
  text: z.string().optional(),
  truncated: z.boolean().optional(),
});
export type RunArtifactResponse = z.infer<typeof RunArtifactResponse>;

/** One answer of a card as a run reads it. */
export const RunCardAnswer = z.object({
  actorId: Id,
  actorName: z.string(),
  at: Millis.optional(),
  value: z.json().optional(),
  inputs: z.json().optional(),
});
export type RunCardAnswer = z.infer<typeof RunCardAnswer>;

/**
 * `GET /v1/machine/runs/:id/cards/:messageId`: a card of the run's group, whole: the message as a
 * compact line, the card document (v2, or the 1.0 card) and its live answers.
 */
export const RunCardResponse = z.object({
  topicId: Id,
  message: ContextMessage,
  card: z.json(),
  /** The open ask: who is waited on. */
  ask: z.object({ status: z.string(), from: z.array(z.string()) }).optional(),
  answers: z.array(RunCardAnswer),
  answerCount: Count.optional(),
});
export type RunCardResponse = z.infer<typeof RunCardResponse>;

/** `GET /v1/machine/runs/:id/decisions?limit=`: the group's decisions, newest first. */
export const RunDecisionsResponse = z.object({
  decisions: z.array(GroupDecision.extend({ answeredByName: z.string() })),
  hasMore: z.boolean(),
  /** Decisions recorded by hand (记一条), newest first. */
  recorded: z.array(RecordedDecision.extend({ recordedByName: z.string() })).optional(),
});
export type RunDecisionsResponse = z.infer<typeof RunDecisionsResponse>;

/** One member of the run's group as a run reads it: name, 岗位 and whether it is an AI member. */
export const RunMember = z.object({ id: Id, name: z.string(), role: z.string().optional(), ai: z.boolean() });
export type RunMember = z.infer<typeof RunMember>;

/** `GET /v1/machine/runs/:id/members` */
export const RunMembersResponse = z.object({ members: z.array(RunMember) });
export type RunMembersResponse = z.infer<typeof RunMembersResponse>;

/** `GET /v1/machine/runs/:id/topic/state?topic=`: one compact state card of a topic (at most 2,000 characters). */
export const TopicStateResponse = z.object({ topicId: Id, title: z.string(), text: z.string().max(2000) });
export type TopicStateResponse = z.infer<typeof TopicStateResponse>;

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
  /** Thinking effort (思考强度) as the CLI spells it. Absent keeps the stored one (clients before
   * it send none); null clears it (the CLI's default). */
  effort: Effort.nullable().optional(),
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
  effort: z.string().optional(),
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

/** Usable: at most this many AI members on one personal machine. */
export const MAX_PERSONAL_BOTS = 200;
/** Usable: an AI member whose work for the machine's owner runs on that machine. */
export const PersonalBot = z.object({
  botId: Id,
  /** The owner's code folder on this machine for that work. */
  repoPath: z.string().min(1).max(1024).optional(),
});
export type PersonalBot = z.infer<typeof PersonalBot>;

/** `Machine.unhealthy` values (机 5.6); readers treat an unknown one as a failed check without a name. */
export const MACHINE_UNHEALTHY = ["heartbeat", "disk"] as const;

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
  /**
   * Usable (机 5.6): the check that failed when connected and not healthy: "heartbeat" (no
   * heartbeat for 90 s) or "disk" (free disk below the limit). Open string; absent otherwise.
   */
  unhealthy: z.string().min(1).max(32).optional(),
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
  /** Usable: the person whose own machine this is (个人的电脑); absent = a team machine. */
  ownerId: Id.optional(),
  /** Usable: the AI members whose work for the owner runs here. Sent to the owner only. */
  personalBots: z.array(PersonalBot).max(MAX_PERSONAL_BOTS).optional(),
  /** Usable: the computer's own GitHub sign-in (`metrics.github`). Admins and the computer's owner only. */
  github: MachineGithub.optional(),
});
export type Machine = z.infer<typeof Machine>;

/** `POST /v1/machines` (owner; usable: any member, see `personal`) */
export const CreateMachineRequest = z.object({
  name: Name,
  labels: Labels.optional(),
  /** Usable: true = my own machine; absent = a team machine for an admin, my own for a member. */
  personal: z.boolean().optional(),
});
export type CreateMachineRequest = z.infer<typeof CreateMachineRequest>;
/** `PATCH /v1/machines/:id` (usable): rename it, or move it between the team and me. At least one field. */
export const UpdateMachineRequest = z
  .object({
    name: Name.optional(),
    /** "me" = make it my own machine; null = give it to the team. */
    owner: z.literal("me").nullable().optional(),
  })
  .refine((r) => r.name !== undefined || r.owner !== undefined, { message: "name or owner is required" });
export type UpdateMachineRequest = z.infer<typeof UpdateMachineRequest>;
/** `PUT /v1/machines/:id/bots/:botId` (the machine's owner, usable). Response: `Machine`. */
export const SetMachineBotRequest = z.object({
  /** My code folder on this machine for the bot's work for me; null clears it, absent keeps it. */
  repoPath: z.string().min(1).max(1024).nullable().optional(),
});
export type SetMachineBotRequest = z.infer<typeof SetMachineBotRequest>;
/** The token (`tm_` + 43 base64url) is shown only in this response. */
export const CreateMachineResponse = z.object({ machine: Machine, token: z.string().min(1) });
export type CreateMachineResponse = z.infer<typeof CreateMachineResponse>;
/** `GET /v1/machines` (owner) */
export const MachinesResponse = z.array(Machine);

