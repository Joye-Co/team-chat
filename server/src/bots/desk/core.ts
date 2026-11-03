// BotDesk core: the Durable Object's state, its SQLite migrations, run rows and the writes
// it makes in rooms as the bot. The feature modules extend it in this order: runs (start and
// machine answers), events (run log and results), stall (stall detection), input (topic
// messages, approvals, stop), takeover (pause, resume, acceptance) and the BotDesk class itself
// (external runs and reads).
import { DurableObject } from "cloudflare:workers";
import {
  AcceptanceItem,
  type BotRun,
  type Card,
  type CardV2,
  MAX_ACCEPTANCE_ITEMS,
  MAX_PLAN_ITEMS,
  MAX_PLAN_TEXT_CHARS,
  type RunPlan,
  RunPlanItem,
  RUN_KIND_ASIDE,
  type RunStatus,
  type TopicStatus,
  Usage,
} from "@chat/protocol";
import { roomStub } from "../../core/env";
import { type Result, toErrorBody, unwrap } from "../../core/errors";
import { logger as baseLogger, type Logger } from "../../core/logger";
import { runMigrations } from "../../core/migrations";
import type { TokenBucket } from "../../core/ratelimit";
import type { Actor } from "../../core/registry";
import { ROOM_REACT_RPC } from "../../reactions";
import type { BotsEnv } from "../env";
import { ACK_EMOJI } from "../proof";
import type { TopicMessage } from "../room";

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
};

export const ACTIVE = "ended_at IS NULL";
/** A run still `created` with no machine this long after it was made failed to start. */
export const START_STUCK_MS = 2 * 60_000;
/** Follow-up runs in a row (leftover input after a run ended) before the bot stops chaining. */
export const MAX_FOLLOW_UPS = 5;
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
    if (parsed.success) out.push({ text: parsed.data.text, state: parsed.data.state });
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
      ]);
      const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'bot_id'").toArray()[0];
      this.botId = row?.value ?? null;
      if (this.botId) this.log = baseLogger.child({ obj: "BotDesk", botId: this.botId });
    });
  }

  protected bind(botId: string): void {
    if (this.botId === botId) return;
    if (this.botId !== null) throw new Error("desk bound to another bot");
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('bot_id', ?)", botId);
    this.botId = botId;
    this.log = baseLogger.child({ obj: "BotDesk", botId });
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
    return run;
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

  protected async topic(r: RunRow, status: TopicStatus, waitingOn: string[] = []): Promise<void> {
    // A side chat answers in the asker's DM; its topic keeps no work status.
    if (r.kind === RUN_KIND_ASIDE) return;
    try {
      unwrap(
        await this.room(r).topicUpdate(r.group_id, this.actor(), {
          t: "topic.update",
          rid: "bot",
          groupId: r.group_id,
          topicId: r.topic_id,
          status,
          waitingOn,
          ownerId: this.botId,
        } as never),
      );
    } catch (err) {
      this.log.warn("topic update failed", { runId: r.id, status, err });
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
  protected abstract end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string, success?: boolean): Promise<void>;
  protected abstract scheduleStall(): Promise<void>;
  protected abstract markStalled(r: RunRow): Promise<void>;
  protected abstract start(m: TopicMessage, followUpOf?: RunRow, followUps?: { authorId: string; body: string }[]): Promise<void>;
}
