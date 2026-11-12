// BotDesk core: the Durable Object's state, its SQLite migrations, run rows and the writes
// it makes in rooms as the bot. The feature modules extend it in this order: runs (start and
// machine answers), events (run log and results), stall (stall detection), input (topic
// messages, approvals, stop), takeover (pause, resume, acceptance) and the BotDesk class itself
// (external runs and reads).
import { DurableObject } from "cloudflare:workers";
import type { Approver } from "@chat/protocol";
import { effectivePolicy } from "../../settings/approvals";
import { approverOf } from "../../settings/rules";
import {
  AcceptanceItem,
  type BotRun,
  type Card,
  type CardV2,
  isMainTopicId,
  MAX_ACCEPTANCE_ITEMS,
  MAX_PLAN_ITEMS,
  MAX_PLAN_TEXT_CHARS,
  type RunPlan,
  RunFailure,
  RunPlanItem,
  RUN_KIND_ASIDE,
  RunCode,
  RunHandback,
  type RunStatus,
  SuperviseAct,
  type TopicStatus,
  Usage,
} from "@chat/protocol";
import { roomStub } from "../../core/env";
import { type Result, toErrorBody, unwrap } from "../../core/errors";
import { logger as baseLogger, type Logger } from "../../core/logger";
import { runMigrations } from "../../core/migrations";
import type { TokenBucket } from "../../core/ratelimit";
import type { Actor } from "../../core/registry";
import { ANONYMOUS_ID } from "../../cards2/privacy";
import { ROOM_REACT_RPC } from "../../reactions";
import { type BotsEnv, linkStub } from "../env";
import { ACK_EMOJI } from "../proof";
import type { TopicMessage } from "../room";
import type { WorkState } from "../participants";
import type { Brief } from "../../agent/frontdesk";
import { needsReply, reportOf } from "./report";
import { holdKindOf, isHeld, phaseOf } from "./phase";

export type RunRow = {
  id: string;
  group_id: string;
  topic_id: string;
  machine_id: string | null;
  status: RunStatus;
  requested_by: string;
  trigger_message_id: string;
  created_at: number;
  started_at: number | null;
  ended_at: number | null;
  updated_at: number;
  last_event_at: number;
  summary: string | null;
  progress_message_id: string | null;
  progress_text: string;
  progress_full: number;
  items: string;
  approval_id: string | null;
  stalled: number;
  notes: number;
  excluded: string;
  last_seq: number;
  follow_up_of: string | null;
  /** JSON of the latest plan, or null. */
  plan: string | null;
  /** 1 = started by an external bot with `POST /v1/runs` (no machine). */
  external: number;
  /** Highest run_log seq whose effects were applied (frames are stored before they apply). */
  applied_seq: number;
  // desk-4 (1.1)
  /** JSON of `Usage` from run.done, or null. */
  usage: string | null;
  duration_ms: number | null;
  result_message_id: string | null;
  /** JSON of the latest acceptance criteria, or null. */
  acceptance: string | null;
  /** Who paused the run (接管); null while it is not paused. */
  paused_by: string | null;
  paused_at: number | null;
  /** "aside" = a read-only side chat run; null = a normal run. */
  kind: string | null;
  // desk-5 (1.1 interaction loop)
  /** 1 while the CLI is inside a turn (turn.started without turn.completed). */
  in_turn: number;
  // desk-6 (1.1 retry)
  /** The run this one retries (`POST /v1/runs/:id/retry`), or null. */
  retry_of: string | null;
  // desk-6-queue (1.1, GAPS SF-3)
  /** Set while the run waits for a free machine (排队); null otherwise. */
  queued_at: number | null;
  // desk-6 (1.1 resume)
  /** The CLI session the run used (`run.done.sessionId` or the `session` event), or null. */
  session_id: string | null;
  /** The runtime the run was assigned to, or null. */
  runtime: string | null;
  // desk-7-pause-order (GAPS SB-16)
  /** run.pause frames the server sent for this run. */
  pause_gen: number;
  /** `paused: true` events the daemon reported for this run (at most `pause_gen`). */
  pause_seen: number;
  // desk-8-context (Run context)
  /** The highest topic seq the run's prompt covered, or null (runs before it). */
  context_seq: number | null;
  /** 1: the run must start a new CLI session (a new request in the 群聊). */
  fresh: number;
  // desk-10-supervise (盯活, agent/supervise.ts)
  /** Times the desk fixed this piece of work by itself (carried over to a restarted run). */
  interventions: number;
  /** Tool calls the run made. */
  tool_calls: number;
  /** 1 after the silence nudge; progress clears it. */
  nudged: number;
  loop_key: string | null;
  loop_count: number;
  /** Tool calls without a result yet: while it is above 0 the CLI is waiting for a step, not silent. */
  open_tools: number;
  /** Runs in a row the desk continued because the CLI only promised work. */
  auto_continues: number;
  // desk-machine-away (盯 1.1, 盯 1.3)
  /** Since when the run waits for its machine to come back; null while the machine is there. */
  away_since: number | null;
  /** How far the desk got with a silent piece of work: 1 = restarted in the same session, 2 = moved to another machine. Carried over. */
  stall_step: number;
  /** A nudge was really sent for this piece of work (盯 1.12: the note names only real tries). Carried over. */
  nudge_sent: number;
  // desk-report (接 4.2)
  /** JSON of the run's latest `chat-run report` (report.ts), or null. */
  report: string | null;
  // desk-run-code (design-relay R5)
  /** JSON of `RunCode` from run.done (branch, head, pushed), or null when not known. */
  code: string | null;
  // desk-requested-for (usable, protocol "Who a run works for")
  /** The person the run works for (`BotRun.requestedFor`); null for a hook's run and runs from before. */
  requested_for: string | null;
  // reach-1 (design-visibility 3.3)
  /** JSON array of the other groups the run read (`noteRead`), or null when none. */
  read_groups: string | null;
  // desk-failure-model (模 2.2, 2.3, 2.10)
  /** JSON of `RunFailure` of a failed run, or null. */
  failure: string | null;
  /** The model and 思考强度 the CLI really ran with (the session event), or null. */
  model: string | null;
  effort: string | null;
  // desk-hold-1 (design-takeover 3.7)
  /** NULL (a pause, or not held) | "pause" | "takeover"; set with `paused_by`. */
  hold_kind: string | null;
  /** Hold reminders sent: 1 after 2 hours, 2 after 24 hours. */
  hold_reminded: number;
  /** The person's command running now (its commandId), or null. */
  hand_command: string | null;
  /** When that command was sent: a command with no result after HAND_COMMAND_MAX_MS no longer blocks. */
  hand_command_at: number | null;
  /** When a person last handed the run back (the two-minute rule). */
  handed_back_at: number | null;
  /** JSON of the latest `hand.summary` (`RunHandback`), or null. */
  last_handback: string | null;
  /** How the run's machine can be taken over (`RUN_TAKEOVER`), from its features at assignment. */
  takeover: string | null;
  // desk-limit-wait (模 3.4, 盯 1.5)
  /** While a queued run waits for its computer's subscription limit: when it resets (server clock); else null. */
  limit_until: number | null;
  /** Verify 2, problem 9: why a queued run waits (`offline`); NULL: every machine is busy. */
  wait_for?: string | null;
  // desk-time-limit (盯 1.4, 1.6, 1.8, 1.11, 5.5)
  /** The team's time limit (ms of counted work) when the run was assigned, or null (the default). */
  limit_ms: number | null;
  /** Work time that counts toward the limit: not waiting for an approval, a pause or the machine. */
  worked_ms: number;
  /** When `worked_ms` was last brought up to date, or null before the first check. */
  worked_at: number | null;
  /** When the run was told to save its state at the time limit; null before. */
  limit_at: number | null;
  /** Turns in a row that reached the time limit without progress. Carried over. */
  limit_rounds: number;
  /** Signs of progress in this run: new successful steps, file changes, plan and acceptance updates, outputs. */
  progress: number;
  /** The daemon's latest heartbeat that said a step of the run is working (盯 1.4), or null. */
  alive_at: number | null;
  /** JSON of the open tool calls (item id -> title) for loops that look at the output (盯 1.11). */
  loop_calls: string | null;
  /** JSON of the desk's acts on this run (`SuperviseAct[]`, 盯 5.5), or null. */
  acts: string | null;
};

/**
 * A run's place in its chain (design-relay R2, meta `chain:<runId>`): the room's step, its depth
 * and path, and what every later step gets: the root request as text and the root topic.
 */
export interface ChainMeta {
  chainId: string;
  seq: number;
  depth: number;
  path: string[];
  rootRequest?: string;
  rootTopicId?: string;
}

/** A stored or reported code location (`run.done.code`), or null when it is not a valid one. */
export function codeOf(v: unknown): RunCode | null {
  if (v === null || v === undefined) return null;
  let raw = v;
  if (typeof v === "string") {
    try {
      raw = JSON.parse(v);
    } catch {
      return null;
    }
  }
  const parsed = RunCode.safeParse(raw);
  return parsed.success ? parsed.data : null;
}

/** JSON text parsed, or undefined when it does not parse. */
export function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

/** The desk's acts on a run (盯 5.5), oldest first; [] when none or the row does not parse. */
export function actsOf(r: Pick<RunRow, "acts">): SuperviseAct[] {
  if (!r.acts) return [];
  try {
    const v = JSON.parse(r.acts) as unknown;
    return Array.isArray(v) ? v.filter((x): x is SuperviseAct => SuperviseAct.safeParse(x).success) : [];
  } catch {
    return [];
  }
}

/** The other groups a run read (`runs.read_groups`, design-visibility 3.3); [] when none. */
export function readGroupsOf(r: Pick<RunRow, "read_groups">): string[] {
  if (!r.read_groups) return [];
  try {
    const v = JSON.parse(r.read_groups) as unknown;
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** A CLI session id BotDesk keeps and sends back (`run.assign.resumeSessionId`). */
export function sessionIdOf(v: unknown): string | null {
  return typeof v === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(v) ? v : null;
}

export const ACTIVE = "ended_at IS NULL";
/** Runs waiting for a free machine (排队): created, no machine yet, queued. */
export const QUEUED = "ended_at IS NULL AND status = 'created' AND machine_id IS NULL AND queued_at IS NOT NULL";
/** A queued run that gets no machine this long after it was queued fails (机 2.2: a computer may be closed overnight). */
export const QUEUE_MAX_MS = 24 * 60 * 60_000;
/** How often the desk tries its queued runs again (machines free up in other desks too). */
export const QUEUE_RETRY_MS = 15_000;
/** A run still `created` with no machine this long after it was made failed to start. */
export const START_STUCK_MS = 2 * 60_000;
/** Follow-up runs in a row (leftover input after a run ended) before the bot stops chaining. */
export const MAX_FOLLOW_UPS = 5;
/** A run report the room does not take is sent again by the alarm for at most this long. */
export const PENDING_STATE_MAX_MS = 24 * 60 * 60_000;
/** Plan updates over HTTP, per run: 2 per second, bursts of 10. */
export const PLAN_RATE = { rate: 2, burst: 10 };

/** Runs `fn` and turns its error into a Result (thrown errors lose their class over RPC). */
export async function result<T>(fn: () => Promise<T>): Promise<Result<T>> {
  try {
    return { ok: true, value: await fn() };
  } catch (err) {
    return { ok: false, error: toErrorBody(err) };
  }
}

/** A daemon's plan items cut to the protocol limits; null when `items` is not a list. */
export function sanitizePlan(items: unknown): RunPlan | null {
  if (!Array.isArray(items)) return null;
  const out: RunPlan = [];
  for (const it of items) {
    const parsed = RunPlanItem.safeParse({ ...(it as object), text: typeof it?.text === "string" ? [...it.text.trim()].slice(0, MAX_PLAN_TEXT_CHARS).join("") : it?.text });
    if (parsed.success) {
      const { text, state, startedAt, endedAt, estimateMs } = parsed.data;
      out.push({ text, state, ...(startedAt !== undefined ? { startedAt } : {}), ...(endedAt !== undefined ? { endedAt } : {}), ...(estimateMs !== undefined ? { estimateMs } : {}) });
    }
    if (out.length === MAX_PLAN_ITEMS) break;
  }
  return out;
}

/** A daemon's acceptance items cut to the protocol limits; null when `items` is not a list. */
export function sanitizeAcceptance(items: unknown): AcceptanceItem[] | null {
  if (!Array.isArray(items)) return null;
  const out: AcceptanceItem[] = [];
  for (const it of items) {
    const parsed = AcceptanceItem.safeParse(it);
    if (parsed.success) out.push(parsed.data);
    if (out.length === MAX_ACCEPTANCE_ITEMS) break;
  }
  return out;
}

/** `run.done.usage` when it fits the protocol, else null. */
export function sanitizeUsage(usage: unknown): Usage | null {
  const parsed = Usage.safeParse(usage);
  return parsed.success ? parsed.data : null;
}

/** A daemon status event from its own stall detector (machine-protocol.md). */
export function isStallStatus(event: unknown): boolean {
  const e = event as { type?: unknown; state?: unknown } | null;
  return e?.type === "status" && (e.state === "stalled" || e.state === "retrying");
}

export function samePlan(a: RunPlan | null, b: RunPlan): boolean {
  return (a ?? []).length === b.length && (a ?? []).every((x, i) => x.text === b[i].text && x.state === b[i].state);
}
export const MAX_PAGE = 500;
export const DEFAULT_PAGE = 200;

/**
 * Desk migration `github-off-1` (design-github-identity 3.4): no run waits for a person's GitHub
 * any more. The runs that did (meta `github_wait:` and `github_resume:`) are kept in
 * `github_off_pending`; their topics are set right after the next bind (`clearGithubWaits`).
 */
export function githubOff1(sql: SqlStorage): void {
  const keys = "key GLOB 'github_wait:*' OR key GLOB 'github_resume:*'";
  const ids = sql
    .exec<{ key: string }>(`SELECT key FROM meta WHERE ${keys}`)
    .toArray()
    .map((x) => x.key.slice(x.key.indexOf(":") + 1));
  const unique = [...new Set(ids)];
  if (unique.length > 0) sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('github_off_pending', ?)", JSON.stringify(unique));
  sql.exec(`DELETE FROM meta WHERE ${keys}`);
}

export abstract class DeskCore extends DurableObject<BotsEnv> {
  protected readonly sql: SqlStorage;
  protected botId: string | null = null;
  protected log: Logger = baseLogger.child({ obj: "BotDesk" });
  /** One chain per run so frames, accepts and timeouts of one run apply in order. */
  protected readonly chains = new Map<string, Promise<unknown>>();
  protected readonly planBuckets = new Map<string, TokenBucket>();

  constructor(ctx: DurableObjectState, env: BotsEnv) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      runMigrations(ctx.storage, [
        {
          name: "desk-1",
          up: `
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE runs (
              id TEXT PRIMARY KEY,
              group_id TEXT NOT NULL,
              topic_id TEXT NOT NULL,
              machine_id TEXT,
              status TEXT NOT NULL,
              requested_by TEXT NOT NULL,
              trigger_message_id TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              started_at INTEGER,
              ended_at INTEGER,
              updated_at INTEGER NOT NULL,
              last_event_at INTEGER NOT NULL,
              summary TEXT,
              progress_message_id TEXT,
              progress_text TEXT NOT NULL DEFAULT '',
              progress_full INTEGER NOT NULL DEFAULT 0,
              items TEXT NOT NULL DEFAULT '{}',
              approval_id TEXT,
              stalled INTEGER NOT NULL DEFAULT 0,
              notes INTEGER NOT NULL DEFAULT 0,
              excluded TEXT NOT NULL DEFAULT '[]',
              last_seq INTEGER NOT NULL DEFAULT 0,
              follow_up_of TEXT
            );
            CREATE INDEX runs_topic ON runs(topic_id, created_at);
            CREATE INDEX runs_active ON runs(ended_at);
            CREATE TABLE run_log (
              run_id TEXT NOT NULL,
              seq INTEGER NOT NULL,
              at INTEGER NOT NULL,
              frame TEXT NOT NULL,
              PRIMARY KEY (run_id, seq)
            );
            CREATE TABLE inputs (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              message_id TEXT NOT NULL,
              author_id TEXT NOT NULL,
              body TEXT NOT NULL,
              at INTEGER NOT NULL,
              accepted INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX inputs_run ON inputs(run_id);
          `,
        },
        {
          name: "desk-2-plan",
          up: `
            ALTER TABLE runs ADD COLUMN plan TEXT;
            ALTER TABLE runs ADD COLUMN external INTEGER NOT NULL DEFAULT 0;
          `,
        },
        {
          // Runs made before this step applied every stored frame.
          name: "desk-3-applied",
          up: `
            ALTER TABLE runs ADD COLUMN applied_seq INTEGER NOT NULL DEFAULT 0;
            UPDATE runs SET applied_seq = last_seq;
            CREATE INDEX inputs_message ON inputs(message_id);
            CREATE INDEX runs_trigger ON runs(trigger_message_id);
          `,
        },
        {
          // 1.1: run.done usage and time, the result message, acceptance criteria, pause (接管)
          // and the run kind (side chats). Old rows read as a normal run without them.
          name: "desk-4-v11",
          up: `
            ALTER TABLE runs ADD COLUMN usage TEXT;
            ALTER TABLE runs ADD COLUMN duration_ms INTEGER;
            ALTER TABLE runs ADD COLUMN result_message_id TEXT;
            ALTER TABLE runs ADD COLUMN acceptance TEXT;
            ALTER TABLE runs ADD COLUMN paused_by TEXT;
            ALTER TABLE runs ADD COLUMN paused_at INTEGER;
            ALTER TABLE runs ADD COLUMN kind TEXT;
          `,
        },
        {
          // 1.1: the card a Card v2 answer input answers, for the acknowledgement of a follow-up
          // run. Old rows read as NULL and acknowledge on message_id.
          name: "desk-5-input-ack",
          up: `ALTER TABLE inputs ADD COLUMN ack_message_id TEXT;`,
        },
        {
          // 1.1 interaction loop: coalescing windows of live events per card, events waiting for
          // a window or a machine, short follow-up runs started by live events (limits), refs a
          // run already saw, and whether the CLI is inside a turn (expect).
          name: "desk-5-interact",
          up: `
            ALTER TABLE runs ADD COLUMN in_turn INTEGER NOT NULL DEFAULT 0;
            CREATE TABLE interact_windows (card_key TEXT PRIMARY KEY, run_id TEXT NOT NULL, until INTEGER NOT NULL);
            CREATE TABLE interact_pending (
              card_key TEXT NOT NULL,
              block_key TEXT NOT NULL,
              run_id TEXT NOT NULL,
              frame TEXT NOT NULL,
              at INTEGER NOT NULL,
              PRIMARY KEY (card_key, block_key)
            );
            CREATE TABLE interact_runs (run_id TEXT PRIMARY KEY, card_key TEXT NOT NULL, at INTEGER NOT NULL);
            CREATE INDEX interact_runs_at ON interact_runs(at);
            CREATE TABLE run_refs_seen (run_id TEXT NOT NULL, message_id TEXT NOT NULL, PRIMARY KEY (run_id, message_id));
          `,
        },
        {
          // 1.1 retry: the run a run retries. Old rows read as NULL (not a retry); their request
          // is read from the room when they are retried (meta `request:{runId}` holds it for new runs).
          name: "desk-6-retry",
          up: `ALTER TABLE runs ADD COLUMN retry_of TEXT;`,
        },
        {
          // 1.1 queue (GAPS SF-3): a run whose machines are all busy waits instead of failing.
          // Old rows read as NULL (not queued).
          name: "desk-6-queue",
          up: `ALTER TABLE runs ADD COLUMN queued_at INTEGER;`,
        },
        {
          // 1.1 resume: the CLI session a run used and the runtime it ran on, so the topic's next
          // run on the same machine continues the session. Old rows read as NULL: no resume.
          name: "desk-6-resume",
          up: `
            ALTER TABLE runs ADD COLUMN session_id TEXT;
            ALTER TABLE runs ADD COLUMN runtime TEXT;
          `,
        },
        {
          // GAPS SB-16: pause order. A `paused: false` event clears pausedBy only after the
          // daemon reported every pause the server sent; a late one from an earlier resume does
          // not clear a newer pause. Old rows read as 0/0: the event clears as before.
          name: "desk-7-pause-order",
          up: `
            ALTER TABLE runs ADD COLUMN pause_gen INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN pause_seen INTEGER NOT NULL DEFAULT 0;
          `,
        },
        {
          // Run context: the cursor a resumed run continues from, and the 群聊 rule that a new
          // request does not continue another request's session. Old rows: NULL / 0.
          name: "desk-8-context",
          up: `
            ALTER TABLE runs ADD COLUMN context_seq INTEGER;
            ALTER TABLE runs ADD COLUMN fresh INTEGER NOT NULL DEFAULT 0;
          `,
        },
        {
          // The front desk (agent/frontdesk.ts): each decision is stored before it is acted on.
          name: "desk-9-front",
          up: `
            CREATE TABLE decisions (
              message_id TEXT PRIMARY KEY,
              group_id TEXT NOT NULL,
              topic_id TEXT NOT NULL,
              author_id TEXT NOT NULL,
              action TEXT NOT NULL,
              decision TEXT NOT NULL,
              fallback TEXT,
              model TEXT,
              ms INTEGER NOT NULL DEFAULT 0,
              input_tokens INTEGER NOT NULL DEFAULT 0,
              output_tokens INTEGER NOT NULL DEFAULT 0,
              reply_message_id TEXT,
              answered INTEGER NOT NULL DEFAULT 0,
              at INTEGER NOT NULL
            );
            CREATE INDEX decisions_topic ON decisions(topic_id, at);
          `,
        },
        {
          // 盯活: what the desk watches in a run and how often it stepped in. Old rows: 0 / NULL.
          name: "desk-10-supervise",
          up: `
            ALTER TABLE runs ADD COLUMN interventions INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN tool_calls INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN nudged INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN loop_key TEXT;
            ALTER TABLE runs ADD COLUMN loop_count INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN auto_continues INTEGER NOT NULL DEFAULT 0;
          `,
        },
        {
          // 盯活: a run that waits for a step it started is not silent. Old rows: 0.
          name: "desk-12-open-tools",
          up: `ALTER TABLE runs ADD COLUMN open_tools INTEGER NOT NULL DEFAULT 0;`,
        },
        {
          // M6 replay: what the front desk read, so a decision can be replayed on another model.
          name: "desk-11-replay",
          up: `
            ALTER TABLE decisions ADD COLUMN system TEXT;
            ALTER TABLE decisions ADD COLUMN input TEXT;
          `,
        },
        {
          // 盯 1.1, 盯 1.3: a run waits for a machine that went away, and a silent run is
          // restarted in its own session before it moves. Old rows: NULL / 0.
          name: "desk-machine-away",
          up: `
            ALTER TABLE runs ADD COLUMN away_since INTEGER;
            ALTER TABLE runs ADD COLUMN stall_step INTEGER NOT NULL DEFAULT 0;
          `,
        },
        {
          name: "desk-nudge-sent",
          up: `ALTER TABLE runs ADD COLUMN nudge_sent INTEGER NOT NULL DEFAULT 0;`,
        },
        {
          // 接 4.2: the run's latest `chat-run report`. Old rows: NULL (no report: done as before).
          name: "desk-report",
          up: `ALTER TABLE runs ADD COLUMN report TEXT;`,
        },
        {
          // Usable (design-relay R5): where the run left its code (`run.done.code`, JSON).
          name: "desk-run-code",
          up: `ALTER TABLE runs ADD COLUMN code TEXT;`,
        },
        {
          // Usable (protocol "Who a run works for"): the person a run works for. Old rows: NULL,
          // read as their requester where a person is needed.
          name: "desk-requested-for",
          up: `ALTER TABLE runs ADD COLUMN requested_for TEXT;`,
        },
        {
          // 模 2.2, 2.3, 2.10: why a run failed (run.done.failure, JSON) and the model and
          // 思考强度 it really ran with (the session event). Old rows: NULL.
          name: "desk-failure-model",
          up: `
            ALTER TABLE runs ADD COLUMN failure TEXT;
            ALTER TABLE runs ADD COLUMN model TEXT;
            ALTER TABLE runs ADD COLUMN effort TEXT;
          `,
        },
        {
          // 模 3.4, 盯 1.5: a queued run that waits for a subscription limit to reset. Old rows: NULL.
          name: "desk-limit-wait",
          up: `ALTER TABLE runs ADD COLUMN limit_until INTEGER;`,
        },
        {
          // 盯 1.4, 1.6, 1.8, 1.11, 5.5: the time limit counts only work (not waits), a long step
          // whose processes work is not silent, loops look at the output, and what the desk did is
          // kept with the run. Old rows: NULL / 0 (the default limit, nothing recorded).
          name: "desk-time-limit",
          up: `
            ALTER TABLE runs ADD COLUMN limit_ms INTEGER;
            ALTER TABLE runs ADD COLUMN worked_ms INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN worked_at INTEGER;
            ALTER TABLE runs ADD COLUMN limit_at INTEGER;
            ALTER TABLE runs ADD COLUMN limit_rounds INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN progress INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN alive_at INTEGER;
            ALTER TABLE runs ADD COLUMN loop_calls TEXT;
            ALTER TABLE runs ADD COLUMN acts TEXT;
          `,
        },
        {
          // Usable (design-takeover 3.7): pause versus take-over, the person's command, the
          // hand-back. Old rows: not held, or a pause (hold_kind NULL).
          name: "desk-hold-1",
          up: `
            ALTER TABLE runs ADD COLUMN hold_kind TEXT;
            ALTER TABLE runs ADD COLUMN hold_reminded INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN hand_command TEXT;
            ALTER TABLE runs ADD COLUMN hand_command_at INTEGER;
            ALTER TABLE runs ADD COLUMN handed_back_at INTEGER;
            ALTER TABLE runs ADD COLUMN last_handback TEXT;
            ALTER TABLE runs ADD COLUMN takeover TEXT;
          `,
        },
        {
          // Usable 模 4.1: how the run's CLI was signed in when the run was sent ("subscription" |
          // "api_key"); the weekly budget counts by it. Old rows: NULL (the machine's sign-in now).
          name: "desk-login-method",
          up: `ALTER TABLE runs ADD COLUMN login_method TEXT;`,
        },
        {
          // Verify 2, problem 9: a queued run says whether it waits for a computer to come
          // online (`offline`) or for a free one. Old rows: NULL (waiting for a free one).
          name: "desk-wait-for",
          up: `ALTER TABLE runs ADD COLUMN wait_for TEXT;`,
        },
        {
          // Usable (design-visibility 3.3, 3.6): the other groups a run read; a session that read
          // them resumes only for a person who may read them all. Old rows: NULL (none).
          name: "reach-1",
          up: `ALTER TABLE runs ADD COLUMN read_groups TEXT;`,
        },
        { name: "github-off-1", up: githubOff1 },
      ]);
      const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'bot_id'").toArray()[0];
      this.botId = row?.value ?? null;
      if (this.botId) this.log = baseLogger.child({ obj: "BotDesk", botId: this.botId });
    });
  }

  protected bind(botId: string): void {
    if (this.botId !== botId) {
      if (this.botId !== null) throw new Error("desk bound to another bot");
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('bot_id', ?)", botId);
      this.botId = botId;
      this.log = baseLogger.child({ obj: "BotDesk", botId });
    }
    if (!this.githubOffStarted && this.meta("github_off_pending") !== null) {
      this.githubOffStarted = true;
      this.ctx.waitUntil(this.clearGithubWaits().catch((err) => this.log.warn("github waits cleanup failed", { err })));
    }
  }

  private githubOffStarted = false;

  /**
   * design-github-identity 3.4 (migration `github-off-1`): the topics of runs that waited for a
   * person to connect GitHub stop waiting on them; each gets the status its run ended with.
   */
  protected async clearGithubWaits(): Promise<void> {
    let ids: string[] = [];
    try {
      ids = JSON.parse(this.meta("github_off_pending") ?? "[]") as string[];
    } catch {
      ids = [];
    }
    for (const id of ids) {
      await this.serial(id, async () => {
        const r = this.run(id);
        // A run still going ends as usual, without a GitHub wait.
        if (!r || r.ended_at === null) return;
        // A topic has no failed status: an unsuccessful run leaves it blocked on nobody, as `end` does.
        await this.topic(r, r.status === "done" ? "done" : "blocked", [], { outcome: r.status, result: r.summary ?? "" });
      });
    }
    this.sql.exec("DELETE FROM meta WHERE key = 'github_off_pending'");
  }

  /** Runs `fn` after every earlier operation on the same run. */
  protected serial<T>(runId: string, fn: () => Promise<T>): Promise<T> {
    const prev = this.chains.get(runId) ?? Promise.resolve();
    const next = prev.then(fn, fn);
    const tail = next.catch(() => {});
    this.chains.set(runId, tail);
    tail.then(() => {
      if (this.chains.get(runId) === tail) this.chains.delete(runId);
    });
    return next;
  }

  protected actor(): Actor {
    return { id: this.botId as string, level: "member", kind: "bot" };
  }

  protected run(id: string): RunRow | undefined {
    return this.sql.exec<RunRow>("SELECT * FROM runs WHERE id = ?", id).toArray()[0] as RunRow | undefined;
  }

  protected activeRun(topicId: string): RunRow | undefined {
    return this.sql
      .exec<RunRow>(`SELECT * FROM runs WHERE topic_id = ? AND ${ACTIVE} ORDER BY created_at DESC LIMIT 1`, topicId)
      .toArray()[0] as RunRow | undefined;
  }

  /** The topic's active runs, newest first (接 1.5: a 群聊 or DM chat area can have several). */
  protected activeRuns(topicId: string): RunRow[] {
    return this.sql.exec<RunRow>(`SELECT * FROM runs WHERE topic_id = ? AND ${ACTIVE} ORDER BY created_at DESC`, topicId).toArray() as RunRow[];
  }

  /**
   * True when a new run may not start in the topic now: a topic (话题) is one piece of work with
   * one run at a time; in a chat area (main topic) each request gets its own run (接 1.5).
   */
  protected startBlocked(topicId: string): boolean {
    return !isMainTopicId(topicId) && this.activeRun(topicId) !== undefined;
  }

  /**
   * The active run a card of this bot belongs to (its answers and live events go there). A topic
   * (话题) has one run at a time. In a chat area (接 1.5) it is the run that posted the card, while
   * it is active; a card no run is known to have posted goes to the only active run, if there is
   * just one. Undefined: no run takes it (an answer may start one).
   */
  protected runOfCard(topicId: string, messageId: string): RunRow | undefined {
    if (!isMainTopicId(topicId)) return this.activeRun(topicId);
    const owner = this.meta(`runCard:${messageId}`);
    if (owner !== null) {
      const r = this.run(owner);
      return r && r.ended_at === null ? r : undefined;
    }
    const runs = this.activeRuns(topicId);
    return runs.length === 1 ? runs[0] : undefined;
  }

  protected toRun(r: RunRow): BotRun {
    const run: BotRun = {
      id: r.id,
      botId: this.botId as string,
      groupId: r.group_id,
      topicId: r.topic_id,
      status: r.status,
      createdAt: r.created_at,
      updatedAt: r.updated_at,
    };
    if (r.requested_by) run.requestedBy = r.requested_by;
    // A private card's answerer stays on the server: the wire field is absent then.
    if (r.requested_for && r.requested_by !== ANONYMOUS_ID) run.requestedFor = r.requested_for;
    if (r.machine_id) run.machineId = r.machine_id;
    if (r.started_at !== null) run.startedAt = r.started_at;
    if (r.ended_at !== null) run.endedAt = r.ended_at;
    if (r.summary) run.summary = r.summary;
    const plan = this.planOf(r);
    if (plan && plan.length > 0) run.plan = plan;
    const usage = r.usage ? sanitizeUsage(JSON.parse(r.usage)) : null;
    if (usage) run.usage = usage;
    if (r.duration_ms !== null && r.duration_ms !== undefined) run.durationMs = r.duration_ms;
    if (r.result_message_id) run.resultMessageId = r.result_message_id;
    const acceptance = this.acceptanceOf(r);
    if (acceptance && acceptance.length > 0) run.acceptance = acceptance;
    if (r.paused_by) run.pausedBy = r.paused_by;
    if (r.kind) run.kind = r.kind;
    if (r.retry_of) run.retryOf = r.retry_of;
    const code = codeOf(r.code);
    if (code) run.code = code;
    if (r.model) run.model = r.model;
    if (r.effort) run.effort = r.effort;
    const failure = r.failure ? RunFailure.safeParse(JSON.parse(r.failure)) : null;
    if (failure?.success) run.failure = failure.data;
    const acts = actsOf(r);
    if (acts.length > 0) run.supervision = acts;
    if (r.queued_at && r.ended_at === null && r.status === "created" && !r.machine_id) {
      run.queuedAt = r.queued_at;
      run.queuePosition = this.queuePosition(r);
    }
    // 模 3.4: waiting for the subscription limit is waiting for its computer (phaseOf, rule 2.2).
    // design-takeover 3.1: one phase per run (an ended one follows its report, 接 4.2).
    if (r.ended_at === null || !r.external) {
      const report = r.ended_at !== null ? reportOf(r) : null;
      const { phase, waitingFor } = phaseOf(r, { report, superseded: needsReply(report) && this.superseded(r) });
      run.phase = phase;
      if (waitingFor) run.waitingFor = waitingFor;
      // design-roles 3.3: a queued run that waits for its person's own computers to come online.
      if (waitingFor === "queue" && this.meta(`waitPersonal:${r.id}`) !== null) run.waitingFor = "offline";
    }
    if (isHeld(r)) {
      run.holdKind = holdKindOf(r);
      if (r.paused_at !== null) run.heldAt = r.paused_at;
    }
    if (r.takeover) run.takeover = r.takeover;
    const handback = r.last_handback ? RunHandback.safeParse(safeJson(r.last_handback)) : null;
    if (handback?.success) run.lastHandback = handback.data;
    return run;
  }

  /**
   * design-takeover 3.7: the run is no longer held (handed back, ended, restarted). Clears the
   * hold and the person's running command, and gives MachineLink the run's lease back. Never throws.
   */
  protected async clearHold(r: RunRow): Promise<void> {
    const held = r.paused_by !== null || r.hold_kind !== null;
    this.sql.exec("UPDATE runs SET paused_by = NULL, paused_at = NULL, hold_kind = NULL, hand_command = NULL, hand_command_at = NULL WHERE id = ?", r.id);
    r.paused_by = null;
    r.paused_at = null;
    r.hold_kind = null;
    r.hand_command = null;
    r.hand_command_at = null;
    if (!held || !r.machine_id) return;
    try {
      await linkStub(this.env, r.machine_id).setHeld(r.id, false);
    } catch (err) {
      this.log.warn("hold release on the machine failed", { runId: r.id, err });
    }
  }

  /** A later run of this AI member started in the run's topic. */
  protected superseded(r: Pick<RunRow, "id" | "topic_id" | "created_at">): boolean {
    const later = this.sql.exec("SELECT 1 FROM runs WHERE topic_id = ? AND id != ? AND created_at >= ? LIMIT 1", r.topic_id, r.id, r.created_at);
    return later.toArray().length > 0;
  }

  /**
   * 1-based place of a queued run among this bot's queued runs, oldest first. design-roles 3.3:
   * only earlier runs that wait for one of the same machines count (meta `places:{runId}`); a
   * run without that record counts as waiting for every machine.
   */
  protected queuePosition(r: Pick<RunRow, "id" | "queued_at">): number {
    const earlier = this.sql
      .exec<{ id: string }>(`SELECT id FROM runs WHERE ${QUEUED} AND (queued_at < ? OR (queued_at = ? AND id < ?))`, r.queued_at, r.queued_at, r.id)
      .toArray();
    const placesOf = (id: string): Set<string> | null => {
      const raw = this.meta(`places:${id}`);
      if (raw === null) return null;
      try {
        return new Set(JSON.parse(raw) as string[]);
      } catch {
        return null;
      }
    };
    const mine = placesOf(r.id);
    let n = 0;
    for (const { id } of earlier) {
      const theirs = mine ? placesOf(id) : null;
      if (!mine || !theirs || [...theirs].some((m) => mine.has(m))) n += 1;
    }
    return n + 1;
  }

  protected acceptanceOf(r: Pick<RunRow, "acceptance">): AcceptanceItem[] | null {
    if (!r.acceptance) return null;
    try {
      return JSON.parse(r.acceptance) as AcceptanceItem[];
    } catch {
      return null;
    }
  }

  /** Keeps `items` as the run's acceptance criteria. Returns false when they did not change. */
  protected storeAcceptance(r: RunRow, items: AcceptanceItem[]): boolean {
    const next = items.length > 0 ? JSON.stringify(items) : null;
    if ((r.acceptance ?? null) === next) return false;
    r.acceptance = next;
    this.sql.exec("UPDATE runs SET acceptance = ? WHERE id = ?", next, r.id);
    return true;
  }

  protected planOf(r: Pick<RunRow, "plan">): RunPlan | null {
    if (!r.plan) return null;
    try {
      return JSON.parse(r.plan) as RunPlan;
    } catch {
      return null;
    }
  }

  /** Keeps `plan` as the run's latest plan. Returns false when it equals the current one. */
  protected storePlan(r: RunRow, plan: RunPlan): boolean {
    if (samePlan(this.planOf(r), plan)) return false;
    r.plan = plan.length > 0 ? JSON.stringify(plan) : null;
    this.sql.exec("UPDATE runs SET plan = ? WHERE id = ?", r.plan, r.id);
    return true;
  }

  // -------------------------------------------------------------------------
  // Room writes (as the bot)
  // -------------------------------------------------------------------------

  protected room(r: Pick<RunRow, "group_id">) {
    return roomStub(this.env, r.group_id);
  }

  protected async post(
    r: Pick<RunRow, "group_id" | "topic_id">,
    clientMsgId: string,
    body: string,
    extra: { streaming?: boolean; card?: Card; cardV2?: CardV2; mentions?: string[] } = {},
  ): Promise<string> {
    const res = unwrap(
      await this.room(r).send(r.group_id, this.actor(), {
        t: "send",
        rid: "bot",
        groupId: r.group_id,
        topicId: r.topic_id,
        clientMsgId,
        body,
        ...extra,
      } as never),
    );
    return res.message.id;
  }

  protected async note(r: RunRow, body: string): Promise<void> {
    const n = r.notes + 1;
    this.sql.exec("UPDATE runs SET notes = ? WHERE id = ?", n, r.id);
    r.notes = n;
    await this.post(r, `run:${r.id}:note:${n}`, body);
  }

  /**
   * The person to mention and wait on for the run's approvals: its requester, or nobody when
   * an answer to a private card started the run (cards2/privacy; then owners approve).
   */
  /** A private card's answer started this run: who it works for stays on the server. */
  protected anonymous(r: Pick<RunRow, "requested_by">): boolean {
    return r.requested_by === ANONYMOUS_ID;
  }

  protected requester(r: Pick<RunRow, "requested_by">): string[] {
    return r.requested_by === ANONYMOUS_ID ? [] : [r.requested_by];
  }

  /**
   * settings: who answers this bot's approvals (谁可以批准): the approver of the gated kind
   * (每类的批准人) when the approval has one, else the team policy's approver.
   */
  protected async approver(kind?: string | null): Promise<Approver> {
    return approverOf(await effectivePolicy(this.env.DB), kind);
  }

  /** The gated kind of the run's open approval (feature "gates"), or null. */
  protected approvalKind(r: Pick<RunRow, "id" | "approval_id">): string | null {
    const raw = this.meta(`approval:${r.id}`);
    if (!raw || !r.approval_id) return null;
    try {
      const v = JSON.parse(raw) as { approvalId?: string; kind?: string };
      return v.approvalId === r.approval_id && typeof v.kind === "string" ? v.kind : null;
    } catch {
      return null;
    }
  }

  /**
   * The gated kinds a person approved for the whole piece of work (这件事里都同意), kept in the
   * run's request (meta `request:{runId}`) and carried to its follow-ups and retries.
   */
  protected approvedGates(runId: string | null | undefined): string[] {
    const raw = runId ? this.meta(`request:${runId}`) : null;
    if (!raw) return [];
    try {
      const v = (JSON.parse(raw) as { approvedGates?: unknown }).approvedGates;
      return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
    } catch {
      return [];
    }
  }

  /** Adds `kind` to the run's request's approved gates. */
  protected approveGateForWork(runId: string, kind: string): void {
    const raw = this.meta(`request:${runId}`);
    if (!raw) return;
    try {
      const req = JSON.parse(raw) as { approvedGates?: string[] };
      const kinds = new Set(this.approvedGates(runId));
      kinds.add(kind);
      req.approvedGates = [...kinds];
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `request:${runId}`, JSON.stringify(req));
    } catch {
      // A request that does not parse keeps no approvals.
    }
  }

  /**
   * settings: the people to mention and wait on for an approval: the requester, or the active
   * human owners in the run's group when the policy (or the kind's approver) says owners approve
   * (mentions must be group members; owners outside the group can still answer).
   */
  protected async approvers(r: Pick<RunRow, "requested_by" | "group_id">, kind?: string | null): Promise<string[]> {
    return (await this.approversOrRequester(r, kind)).ids;
  }

  /**
   * {@link approvers}, and whether they are the requester only because the group has no owner
   * to ask (`noOwner`): then the requester is asked and may answer, and the card says so.
   */
  protected async approversOrRequester(
    r: Pick<RunRow, "requested_by" | "group_id">,
    kind?: string | null,
  ): Promise<{ ids: string[]; noOwner: boolean }> {
    if ((await this.approver(kind)) !== "owners") return { ids: this.requester(r), noOwner: false };
    const owners = await this.env.DB.prepare(
      `SELECT m.id FROM members m JOIN group_members g ON g.member_id = m.id AND g.group_id = ?
       WHERE m.level = 'owner' AND m.kind = 'human' AND m.deactivated = 0 ORDER BY m.id LIMIT 20`,
    )
      .bind(r.group_id)
      .all<{ id: string }>()
      .catch(() => ({ results: [] as { id: string }[] }));
    const ids = owners.results.map((x) => x.id);
    if (ids.length > 0) return { ids, noOwner: false };
    const requester = this.requester(r);
    return { ids: requester, noOwner: requester.length > 0 };
  }

  /**
   * The open approval's card (meta `approvalCard:{runId}`): its message id, once posted, and
   * whether the requester answers it because the group has no owner. Null for another approval.
   */
  protected approvalCard(r: Pick<RunRow, "id" | "approval_id">): { messageId?: string; noOwner: boolean } | null {
    const raw = this.meta(`approvalCard:${r.id}`);
    if (!raw || !r.approval_id) return null;
    try {
      const v = JSON.parse(raw) as { approvalId?: string; messageId?: string; noOwner?: boolean };
      if (v.approvalId !== r.approval_id) return null;
      return { ...(typeof v.messageId === "string" ? { messageId: v.messageId } : {}), noOwner: v.noOwner === true };
    } catch {
      return null;
    }
  }

  /** Who answers the run's open approval: the requester when its card says the group has no owner. */
  protected async approverOfOpen(r: Pick<RunRow, "id" | "approval_id">, kind?: string | null): Promise<Approver> {
    return this.approvalCard(r)?.noOwner ? "requester" : this.approver(kind);
  }

  protected async topic(
    r: RunRow,
    status: TopicStatus,
    waitingOn: string[] = [],
    end?: Pick<WorkState, "outcome" | "result">,
  ): Promise<void> {
    // A side chat answers in the asker's DM; its topic keeps no work status.
    if (r.kind === RUN_KIND_ASIDE) return;
    // Usable (design-relay R1): the room keeps this run's report and rolls the topic's status up
    // from every AI member working there; the routing owner is set when a run claims the topic.
    // R2: with the run's step of its chain, how it ended, who stopped it and its result.
    const s: WorkState = { topicId: r.topic_id, runId: r.id, status, waitingOn, ended: r.ended_at !== null };
    const chain = this.chainOf(r);
    if (chain) s.chain = { chainId: chain.chainId, seq: chain.seq };
    if (s.ended) {
      if (end?.outcome) s.outcome = end.outcome;
      if (end?.result !== undefined) s.result = end.result.slice(0, 4000);
      const stoppedBy = this.meta(`stoppedBy:${r.id}`);
      if (stoppedBy) s.stoppedBy = stoppedBy;
      const handed = this.handedOverOf(r.id);
      if (handed) s.handedOver = handed.to;
      // design-relay R3: a relay does not go on to another member past code that is not pushed.
      const code = codeOf(r.code);
      if (code) s.code = code;
    }
    await this.sendWorkState(r.group_id, s);
  }

  /**
   * Sends a run's report to its room. A report that fails is kept (meta `pendingState:<runId>`)
   * and sent again by the alarm, so the room's rows and chains do not miss a run's end.
   */
  protected async sendWorkState(groupId: string, s: WorkState): Promise<boolean> {
    const key = `pendingState:${s.runId}`;
    try {
      unwrap(await roomStub(this.env, groupId).rpc(groupId, "bots.workState", this.actor(), s));
      this.sql.exec("DELETE FROM meta WHERE key = ?", key);
      return true;
    } catch (err) {
      this.log.warn("topic update failed", { runId: s.runId, status: s.status, err });
      // The room refused it for good (the topic or the bot is gone), or it failed for a day: dropped.
      const code = (err as { code?: string } | null)?.code;
      const prev = this.meta(key);
      const since = prev ? ((JSON.parse(prev) as { at?: number }).at ?? Date.now()) : Date.now();
      if (code === "forbidden" || code === "not_found" || code === "invalid" || Date.now() - since > PENDING_STATE_MAX_MS) {
        this.sql.exec("DELETE FROM meta WHERE key = ?", key);
        return false;
      }
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", key, JSON.stringify({ groupId, s, at: since }));
      await this.scheduleStall().catch(() => {});
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // Chains (design-relay R2)
  // -------------------------------------------------------------------------

  /**
   * The run's place in its chain (meta `chain:<runId>`, or `chainFor:<trigger>` written before a
   * handed-over run started). A retry, a restart or a follow-up of a run is the same step.
   */
  protected chainOf(r: Pick<RunRow, "id" | "trigger_message_id" | "retry_of" | "follow_up_of">): ChainMeta | null {
    let cur: Pick<RunRow, "id" | "trigger_message_id" | "retry_of" | "follow_up_of"> | undefined = r;
    for (let i = 0; cur && i < 20; i++) {
      const raw = this.meta(`chain:${cur.id}`) ?? this.meta(`chainFor:${cur.trigger_message_id}`);
      if (raw) {
        try {
          return JSON.parse(raw) as ChainMeta;
        } catch {
          return null;
        }
      }
      const prev: string | null = cur.retry_of ?? cur.follow_up_of;
      cur = prev ? this.run(prev) : undefined;
    }
    return null;
  }

  /** The member a run handed its work over to (meta `handedOver:<runId>`), or null. */
  protected handedOverOf(runId: string): { to: string; name: string; at: number } | null {
    const raw = this.meta(`handedOver:${runId}`);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as { to: string; name: string; at: number };
    } catch {
      return null;
    }
  }

  /** Results of split-off parts kept for a run that had no machine yet; taken once. */
  protected takeParts(runId: string): string | null {
    const raw = this.meta(`partsIn:${runId}`);
    if (raw === null) return null;
    this.sql.exec("DELETE FROM meta WHERE key = ?", `partsIn:${runId}`);
    try {
      const parts = JSON.parse(raw) as string[];
      return parts.length > 0 ? parts.join("\n\n") : null;
    } catch {
      return null;
    }
  }

  /** Acknowledges the message that started the run with a reaction (no text line). */
  protected async ack(r: RunRow, messageId = r.trigger_message_id): Promise<void> {
    try {
      unwrap(await this.room(r).rpc(r.group_id, ROOM_REACT_RPC, this.actor(), { messageId, emoji: ACK_EMOJI, on: true }));
    } catch (err) {
      this.log.warn("ack reaction failed", { runId: r.id, err });
    }
  }

  /** Takes the acknowledgement back (接 1.11: the front desk decided the message is not for it). */
  protected async unack(groupId: string, messageId: string): Promise<void> {
    try {
      unwrap(await roomStub(this.env, groupId).rpc(groupId, ROOM_REACT_RPC, this.actor(), { messageId, emoji: ACK_EMOJI, on: false }));
    } catch (err) {
      this.log.warn("ack removal failed", { messageId, err });
    }
  }

  /** Writes the run's new status and stores a `run.updated` event in the group log. */
  protected async setStatus(r: RunRow, status: RunStatus, change: Partial<RunRow> = {}): Promise<void> {
    // Strictly newer than the last copy: clients keep the copy with the highest `updatedAt`.
    const now = Math.max(Date.now(), r.updated_at + 1);
    Object.assign(r, change, { status, updated_at: now });
    this.sql.exec(
      "UPDATE runs SET status = ?, updated_at = ?, machine_id = ?, started_at = ?, ended_at = ?, summary = ? WHERE id = ?",
      status,
      now,
      r.machine_id,
      r.started_at,
      r.ended_at,
      r.summary,
      r.id,
    );
    try {
      unwrap(await this.room(r).rpc(r.group_id, "bots.runUpdated", this.actor(), { run: this.toRun(r) }));
    } catch (err) {
      this.log.warn("run.updated failed", { runId: r.id, err });
    }
  }

  /** False when this bot member is deactivated: it gets no more work. */
  protected async botActive(): Promise<boolean> {
    const row = await this.env.DB.prepare("SELECT deactivated FROM members WHERE id = ?")
      .bind(this.botId)
      .first<{ deactivated: number }>();
    return Boolean(row && !row.deactivated);
  }

  protected async names(ids: string[]): Promise<Map<string, string>> {
    const uniq = [...new Set(ids)];
    const rows = await this.env.DB.prepare(`SELECT id, name FROM members WHERE id IN (${uniq.map(() => "?").join(",")})`)
      .bind(...uniq)
      .all<{ id: string; name: string }>();
    return new Map(rows.results.map((x) => [x.id, x.name]));
  }

  protected meta(key: string): string | null {
    return this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = ?", key).toArray()[0]?.value ?? null;
  }

  // Implemented by the feature modules that extend this class.
  protected abstract end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string, success?: boolean, waitingOn?: string[]): Promise<void>;
  protected abstract scheduleStall(): Promise<void>;
  protected abstract markStalled(r: RunRow): Promise<void>;
  protected abstract start(
    m: TopicMessage,
    followUpOf?: RunRow,
    followUps?: { authorId: string; body: string }[],
    retryOf?: string,
    brief?: Brief,
    opts?: { newTask?: boolean },
  ): Promise<void>;
  /** A RunEvent `output` (`chat-run`, 1.1): performed and answered by the interaction module. */
  protected abstract output(r: RunRow, ev: { outputId?: unknown; op?: unknown; body?: unknown }, f: { at: number; serverAt?: number }): Promise<void>;
}
