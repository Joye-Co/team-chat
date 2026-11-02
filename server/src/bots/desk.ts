// BotDesk: one per runner-backed bot member, the bot's front desk. It never runs commands and
// has no model: fixed rules turn a mention into a run on a machine, run events into topic
// messages and topic status, and later topic messages into run input. The normalized run log
// lives in its SQLite.
import { DurableObject } from "cloudflare:workers";
import { type BotRun, type BotRunner, type Card, MAX_PLAN_ITEMS, MAX_PLAN_TEXT_CHARS, type RunAssignFrame, type RunPlan, RunPlanItem, type RunStatus, type TopicStatus } from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError, type Result, toErrorBody, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import { logger as baseLogger, type Logger } from "../core/logger";
import { runMigrations } from "../core/migrations";
import { TokenBucket } from "../core/ratelimit";
import type { Actor } from "../core/registry";
import {
  LEASE_MS,
  MAX_RESULT_CHARS,
  MAX_RUN_MS,
  type MachineView,
  Progress,
  STALL_MS,
  TEXT,
  approvalDecision,
  buildPrompt,
  cut,
  firstLine,
  pickMachine,
} from "./dispatch";
import { type BotsEnv, linkStub } from "./env";
import type { SeqFrame } from "./link";
import type { CardPress, ClaimResult, TopicMessage } from "./room";
import { loadMachines, loadRunner } from "./store";

type RunRow = {
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
};

const ACTIVE = "ended_at IS NULL";
/** A run still `created` with no machine this long after it was made failed to start. */
const START_STUCK_MS = 2 * 60_000;
/** Follow-up runs in a row (leftover input after a run ended) before the bot stops chaining. */
const MAX_FOLLOW_UPS = 5;
/** Plan updates over HTTP, per run: 2 per second, bursts of 10. */
const PLAN_RATE = { rate: 2, burst: 10 };

/** Runs `fn` and turns its error into a Result (thrown errors lose their class over RPC). */
async function result<T>(fn: () => Promise<T>): Promise<Result<T>> {
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

/** A daemon status event from its own stall detector (machine-protocol.md). */
function isStallStatus(event: unknown): boolean {
  const e = event as { type?: unknown; state?: unknown } | null;
  return e?.type === "status" && (e.state === "stalled" || e.state === "retrying");
}

function samePlan(a: RunPlan | null, b: RunPlan): boolean {
  return (a ?? []).length === b.length && (a ?? []).every((x, i) => x.text === b[i].text && x.state === b[i].state);
}
const MAX_PAGE = 500;
const DEFAULT_PAGE = 200;

export class BotDesk extends DurableObject<BotsEnv> {
  private readonly sql: SqlStorage;
  private botId: string | null = null;
  private log: Logger = baseLogger.child({ obj: "BotDesk" });
  /** One chain per run so frames, accepts and timeouts of one run apply in order. */
  private readonly chains = new Map<string, Promise<unknown>>();
  private readonly planBuckets = new Map<string, TokenBucket>();

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
      ]);
      const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'bot_id'").toArray()[0];
      this.botId = row?.value ?? null;
      if (this.botId) this.log = baseLogger.child({ obj: "BotDesk", botId: this.botId });
    });
  }

  private bind(botId: string): void {
    if (this.botId === botId) return;
    if (this.botId !== null) throw new Error("desk bound to another bot");
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('bot_id', ?)", botId);
    this.botId = botId;
    this.log = baseLogger.child({ obj: "BotDesk", botId });
  }

  /** Runs `fn` after every earlier operation on the same run. */
  private serial<T>(runId: string, fn: () => Promise<T>): Promise<T> {
    const prev = this.chains.get(runId) ?? Promise.resolve();
    const next = prev.then(fn, fn);
    const tail = next.catch(() => {});
    this.chains.set(runId, tail);
    tail.then(() => {
      if (this.chains.get(runId) === tail) this.chains.delete(runId);
    });
    return next;
  }

  private actor(): Actor {
    return { id: this.botId as string, level: "member", kind: "bot" };
  }

  private run(id: string): RunRow | undefined {
    return this.sql.exec<RunRow>("SELECT * FROM runs WHERE id = ?", id).toArray()[0] as RunRow | undefined;
  }

  private activeRun(topicId: string): RunRow | undefined {
    return this.sql
      .exec<RunRow>(`SELECT * FROM runs WHERE topic_id = ? AND ${ACTIVE} ORDER BY created_at DESC LIMIT 1`, topicId)
      .toArray()[0] as RunRow | undefined;
  }

  private toRun(r: RunRow): BotRun {
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
    return run;
  }

  private planOf(r: Pick<RunRow, "plan">): RunPlan | null {
    if (!r.plan) return null;
    try {
      return JSON.parse(r.plan) as RunPlan;
    } catch {
      return null;
    }
  }

  /** Keeps `plan` as the run's latest plan. Returns false when it equals the current one. */
  private storePlan(r: RunRow, plan: RunPlan): boolean {
    if (samePlan(this.planOf(r), plan)) return false;
    r.plan = plan.length > 0 ? JSON.stringify(plan) : null;
    this.sql.exec("UPDATE runs SET plan = ? WHERE id = ?", r.plan, r.id);
    return true;
  }

  // -------------------------------------------------------------------------
  // Room writes (as the bot)
  // -------------------------------------------------------------------------

  private room(r: Pick<RunRow, "group_id">) {
    return roomStub(this.env, r.group_id);
  }

  private async post(
    r: Pick<RunRow, "group_id" | "topic_id">,
    clientMsgId: string,
    body: string,
    extra: { streaming?: boolean; card?: Card; mentions?: string[] } = {},
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

  private async note(r: RunRow, body: string): Promise<void> {
    const n = r.notes + 1;
    this.sql.exec("UPDATE runs SET notes = ? WHERE id = ?", n, r.id);
    r.notes = n;
    await this.post(r, `run:${r.id}:note:${n}`, body);
  }

  private async topic(r: RunRow, status: TopicStatus, waitingOn: string[] = []): Promise<void> {
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

  /** Writes the run's new status and stores a `run.updated` event in the group log. */
  private async setStatus(r: RunRow, status: RunStatus, change: Partial<RunRow> = {}): Promise<void> {
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

  // -------------------------------------------------------------------------
  // Messages from the topic
  // -------------------------------------------------------------------------

  /** A human message in a topic this bot is responsible for, or one that mentions it. */
  /**
   * A message for this bot, from the room's outbox. The room retries until this returns, so it
   * throws on failure and is idempotent by message id: a message that started a run or was
   * passed to one is not handled again.
   */
  async onMessage(botId: string, m: TopicMessage): Promise<void> {
    this.bind(botId);
    if (this.handledMessage(m.messageId)) return;
    if (this.messagesInFlight.has(m.messageId)) throw new ApiError("conflict", "message is being handled");
    this.messagesInFlight.add(m.messageId);
    try {
      if (!(await this.botActive())) return;
      const active = this.activeRun(m.topicId);
      if (active) return await this.serial(active.id, () => this.toActive(active.id, m));
      if (!m.mentioned) return;
      await this.start(m);
    } catch (err) {
      this.log.error("onMessage failed", { err, topicId: m.topicId });
      throw err;
    } finally {
      this.messagesInFlight.delete(m.messageId);
    }
  }

  private readonly messagesInFlight = new Set<string>();

  private handledMessage(messageId: string): boolean {
    return (
      this.sql.exec("SELECT 1 FROM runs WHERE trigger_message_id = ? LIMIT 1", messageId).toArray().length > 0 ||
      this.sql.exec("SELECT 1 FROM inputs WHERE message_id = ? LIMIT 1", messageId).toArray().length > 0
    );
  }

  /** False when this bot member is deactivated: it gets no more work. */
  private async botActive(): Promise<boolean> {
    const row = await this.env.DB.prepare("SELECT deactivated FROM members WHERE id = ?")
      .bind(this.botId)
      .first<{ deactivated: number }>();
    return Boolean(row && !row.deactivated);
  }

  /** Only a human who is the run's requester, or an owner, may answer its approval. */
  private mayApprove(r: RunRow, actorId: string, kind: string | null | undefined, level: string | null | undefined): boolean {
    return kind === "human" && (actorId === r.requested_by || level === "owner");
  }

  private async auditApproval(r: RunRow, actorId: string, decision: string, via: "card" | "text"): Promise<void> {
    try {
      await this.env.DB.prepare(
        "INSERT INTO audit (actor_id, credential_id, action, target, details, at) VALUES (?, NULL, 'run.approval', ?, ?, ?)",
      )
        .bind(actorId, r.id, JSON.stringify({ approvalId: r.approval_id, decision, via, botId: this.botId }), Date.now())
        .run();
    } catch (err) {
      this.log.warn("approval audit failed", { runId: r.id, err });
    }
  }

  private async toActive(runId: string, m: TopicMessage): Promise<void> {
    const r = this.run(runId);
    if (!r) return;
    if (r.ended_at !== null) {
      // Ended meanwhile: treat as a new request when it mentions the bot.
      if (m.mentioned) await this.start(m);
      return;
    }
    const link = r.machine_id ? linkStub(this.env, r.machine_id) : null;
    const decision = r.approval_id ? approvalDecision(m.body) : null;
    // The room hook sends only human messages; other members' 同意 stays plain input.
    if (r.approval_id && decision && link && this.mayApprove(r, m.authorId, "human", m.authorLevel)) {
      await this.auditApproval(r, m.authorId, decision, "text");
      // Recorded as an input so a retried call does not answer twice.
      this.sql.exec(
        "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, accepted) VALUES (?, ?, ?, ?, ?, ?, 1)",
        ulid(),
        r.id,
        m.messageId,
        m.authorId,
        m.body,
        Date.now(),
      );
      await link.toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision } as never);
      return;
    }
    const inputId = ulid();
    this.sql.exec(
      "INSERT INTO inputs (id, run_id, message_id, author_id, body, at) VALUES (?, ?, ?, ?, ?, ?)",
      inputId,
      r.id,
      m.messageId,
      m.authorId,
      m.body,
      Date.now(),
    );
    if (link) await link.toRun({ t: "run.input", runId: r.id, inputId, text: m.body, authorId: m.authorId } as never);
  }

  /** A card button of this bot's message was pressed (approval cards). */
  async cardAction(botId: string, a: CardPress): Promise<void> {
    this.bind(botId);
    const r = this.sql
      .exec<RunRow>(`SELECT * FROM runs WHERE ${ACTIVE} AND approval_id IS NOT NULL AND group_id = ?`, a.groupId)
      .toArray()
      .find((x) => (x as RunRow).approval_id && a.actionId.endsWith(`:${(x as RunRow).approval_id}`)) as RunRow | undefined;
    if (!r?.machine_id || !r.approval_id) return;
    const decision = a.actionId.startsWith("approve:") ? "approve" : a.actionId.startsWith("deny:") ? "deny" : null;
    if (!decision) return;
    if (!this.mayApprove(r, a.actorId, a.actorKind, a.actorLevel)) {
      this.log.info("approval press ignored", { runId: r.id, actorId: a.actorId, kind: a.actorKind });
      return;
    }
    await this.auditApproval(r, a.actorId, decision, "card");
    await linkStub(this.env, r.machine_id).toRun({ t: "run.approval", runId: r.id, approvalId: r.approval_id, decision } as never);
  }

  // -------------------------------------------------------------------------
  // Starting a run
  // -------------------------------------------------------------------------

  private async start(m: TopicMessage, followUpOf?: RunRow, followUps: { authorId: string; body: string }[] = []): Promise<void> {
    const botId = this.botId as string;
    const runner = await loadRunner(this.env.DB, botId);
    if (!runner?.enabled) return;
    const claim = unwrap(
      await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId }),
    ) as ClaimResult;
    if (claim.botId !== botId) return; // Another bot is responsible for this topic.
    if (this.activeRun(m.topicId)) return; // Raced with another message.

    const now = Date.now();
    const runId = ulid(now);
    this.sql.exec(
      `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at, follow_up_of)
       VALUES (?, ?, ?, 'created', ?, ?, ?, ?, ?, ?)`,
      runId,
      m.groupId,
      m.topicId,
      m.authorId,
      m.messageId,
      now,
      now,
      now,
      followUpOf?.id ?? null,
    );
    const r = this.run(runId) as RunRow;
    await this.serial(runId, async () => {
      try {
        await this.env.DB.prepare(
          "INSERT OR IGNORE INTO bot_runs (id, bot_id, group_id, topic_id, machine_id, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
        )
          .bind(runId, botId, m.groupId, m.topicId, now)
          .run();
        await this.post(r, `run:${runId}:ack`, TEXT.ack);
        await this.setStatus(r, "created");
        await this.topic(r, "in_progress");

        const machine = pickMachine(await loadMachines(this.env), runner, Date.now());
        if (!machine) return this.end(r, "failed", TEXT.noMachine, "blocked", TEXT.noMachine);

        const names = await this.names([...claim.messages.map((x) => x.authorId), m.authorId, botId]);
        const prompt = buildPrompt({
          botName: names.get(botId) ?? "bot",
          title: claim.title,
          messages: claim.messages.filter((x) => x.body !== "" && x.id !== m.messageId),
          names,
          request: { authorId: m.authorId, body: m.body },
          followUps,
        });
        await this.assign(r, runner, machine, prompt);
      } catch (err) {
        // A run that never reached a machine must not block the topic.
        this.log.error("run start failed", { runId, err });
        if (this.run(runId)?.ended_at === null && !this.run(runId)?.machine_id) {
          await this.end(r, "failed", TEXT.failed(""), "blocked", TEXT.failed(""));
        }
      }
      await this.scheduleStall();
    });
  }

  private async names(ids: string[]): Promise<Map<string, string>> {
    const uniq = [...new Set(ids)];
    const rows = await this.env.DB.prepare(`SELECT id, name FROM members WHERE id IN (${uniq.map(() => "?").join(",")})`)
      .bind(...uniq)
      .all<{ id: string; name: string }>();
    return new Map(rows.results.map((x) => [x.id, x.name]));
  }

  private assignFrame(r: RunRow, runner: BotRunner, prompt: string): RunAssignFrame {
    const frame: RunAssignFrame = {
      t: "run.assign",
      runId: r.id,
      botMemberId: this.botId as string,
      groupId: r.group_id,
      topicId: r.topic_id,
      runtime: runner.runtime,
      prompt,
      envAllowlist: runner.envAllowlist,
      leaseMs: LEASE_MS,
      maxDurationMs: MAX_RUN_MS,
    };
    if (runner.model) frame.model = runner.model;
    if (runner.repo) frame.repo = { ...runner.repo, ...(runner.baseBranch ? { baseBranch: runner.baseBranch } : {}) };
    return frame;
  }

  private async assign(r: RunRow, runner: BotRunner, machine: MachineView, prompt: string): Promise<void> {
    const frame = this.assignFrame(r, runner, prompt);
    this.sql.exec("UPDATE runs SET machine_id = ? WHERE id = ?", machine.id, r.id);
    r.machine_id = machine.id;
    await this.env.DB.prepare("UPDATE bot_runs SET machine_id = ? WHERE id = ?").bind(machine.id, r.id).run();
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
    await linkStub(this.env, machine.id).assign(machine.id, this.botId as string, frame);
  }

  private meta(key: string): string | null {
    return this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = ?", key).toArray()[0]?.value ?? null;
  }

  // -------------------------------------------------------------------------
  // From MachineLink
  // -------------------------------------------------------------------------

  async runAccepted(runId: string, machineId: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId || r.status !== "created") return;
      await this.setStatus(r, "running", { started_at: Date.now(), last_event_at: Date.now() });
      this.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", r.last_event_at, runId);
      await this.scheduleStall();
    });
  }

  async runRejected(runId: string, machineId: string, reason: string, message: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId) return;
      this.log.info("run rejected", { runId, machineId, reason, message: cut(message, 200) });
      const excluded = [...(JSON.parse(r.excluded) as string[]), machineId];
      this.sql.exec("UPDATE runs SET excluded = ? WHERE id = ?", JSON.stringify(excluded), runId);
      const runner = await loadRunner(this.env.DB, this.botId as string);
      const machine = runner?.enabled ? pickMachine(await loadMachines(this.env), runner, Date.now(), excluded) : null;
      const prompt = this.meta(`prompt:${runId}`);
      if (!runner || !machine || prompt === null) return this.end(r, "failed", TEXT.noMachine, "blocked", TEXT.noMachine);
      await this.assign(r, runner, machine, prompt);
    });
  }

  async runLost(runId: string, machineId: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId) return;
      await this.end(r, "blocked", TEXT.lost, "blocked", TEXT.lost);
    });
  }

  /** Stores run frames idempotently by (runId, seq) and applies their effects. */
  async ingest(runId: string, frames: SeqFrame[]): Promise<{ lastSeq: number }> {
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "unknown run");
      for (const f of [...frames].sort((a, b) => a.seq - b.seq)) {
        this.sql.exec(
          "INSERT OR IGNORE INTO run_log (run_id, seq, at, frame) VALUES (?, ?, ?, ?)",
          runId,
          f.seq,
          f.at,
          JSON.stringify(f),
        );
      }
      const top = Math.max(r.last_seq, ...frames.map((f) => f.seq));
      this.sql.exec("UPDATE runs SET last_seq = ? WHERE id = ?", top, runId);
      // A late run.session still records its upload.
      if (r.ended_at !== null) return { lastSeq: top };
      // Stored frames whose effects did not apply yet (also after a failed or interrupted apply).
      const pending = this.sql
        .exec<{ seq: number; frame: string }>("SELECT seq, frame FROM run_log WHERE run_id = ? AND seq > ? ORDER BY seq", runId, r.applied_seq)
        .toArray()
        .map((x) => JSON.parse(x.frame) as SeqFrame);
      if (pending.length === 0) return { lastSeq: top };
      await this.apply(r, pending);
      this.sql.exec("UPDATE runs SET applied_seq = max(applied_seq, ?) WHERE id = ?", pending[pending.length - 1].seq, runId);
      return { lastSeq: top };
    });
  }

  private async apply(r: RunRow, frames: SeqFrame[]): Promise<void> {
    const progress = new Progress(r.progress_text, JSON.parse(r.items) as Record<string, string>, r.progress_full === 1);
    const now = Date.now();
    let done: Extract<SeqFrame, { t: "run.done" }> | null = null;
    let planChanged = false;
    // The daemon's own stall reports are not progress: they must not restart the stall clock.
    const progressed = frames.some((f) => f.t !== "run.event" || !isStallStatus(f.event));
    const stallReported = frames.some((f) => f.t === "run.event" && isStallStatus(f.event) && (f.event as { state?: unknown }).state === "stalled");
    if (!progressed) {
      if (stallReported) await this.markStalled(r);
      return;
    }
    this.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", now, r.id);
    r.last_event_at = now;
    if (r.stalled) {
      this.sql.exec("UPDATE runs SET stalled = 0 WHERE id = ?", r.id);
      r.stalled = 0;
      await this.setStatus(r, r.approval_id ? "waiting" : "running");
      await this.topic(r, r.approval_id ? "waiting_answer" : "in_progress", r.approval_id ? [r.requested_by] : []);
    } else if (r.status === "created") {
      await this.setStatus(r, "running", { started_at: now });
    }
    for (const f of frames) {
      if (f.t === "run.done") {
        done = f;
        continue;
      }
      if (f.t !== "run.event") continue;
      const ev = f.event as Record<string, unknown> & { type: string };
      switch (ev.type) {
        case "message.delta":
          progress.delta(String(ev.itemId), String(ev.text ?? ""));
          break;
        case "message":
          if (ev.role === "assistant") progress.message(String(ev.itemId), String(ev.text ?? ""));
          break;
        case "tool.call":
          progress.tool(String(ev.title || ev.tool || ""));
          break;
        case "approval.request": {
          await this.flush(r, progress);
          await this.askApproval(r, String(ev.approvalId), String(ev.title ?? ""), ev.detail ? String(ev.detail) : undefined);
          break;
        }
        case "status":
          // A stall report among real progress: the progress wins.
          break;
        case "approval.resolved":
          if (r.approval_id === ev.approvalId) {
            this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
            r.approval_id = null;
            await this.setStatus(r, "running");
            await this.topic(r, "in_progress");
          }
          break;
        case "input.accepted":
          this.sql.exec("UPDATE inputs SET accepted = 1 WHERE id = ? AND run_id = ?", String(ev.inputId), r.id);
          break;
        case "plan": {
          // The link reads events loosely (newer daemons): keep what fits the plan rules.
          const plan = sanitizePlan(ev.items);
          if (plan && this.storePlan(r, plan)) planChanged = true;
          break;
        }
        default:
          break;
      }
    }
    await this.flush(r, progress);
    // The final run.updated of `finish` carries the plan too.
    if (planChanged && !done) await this.setStatus(r, r.status);
    if (done) await this.finish(r, done);
    else await this.scheduleStall();
  }

  /** Streams the new progress text into the run's one streaming message. */
  private async flush(r: RunRow, progress: Progress): Promise<void> {
    const append = progress.take();
    this.sql.exec(
      "UPDATE runs SET progress_text = ?, progress_full = ?, items = ? WHERE id = ?",
      progress.text,
      progress.full ? 1 : 0,
      JSON.stringify(progress.items),
      r.id,
    );
    r.progress_text = progress.text;
    if (append === "") return;
    try {
      if (!r.progress_message_id) {
        const id = await this.post(r, `run:${r.id}:progress`, progress.text, { streaming: true });
        this.sql.exec("UPDATE runs SET progress_message_id = ? WHERE id = ?", id, r.id);
        r.progress_message_id = id;
      } else {
        unwrap(await this.room(r).delta(r.group_id, this.actor(), r.progress_message_id, append));
      }
    } catch (err) {
      this.log.warn("progress stream failed", { runId: r.id, err });
    }
  }

  /** Ends the streaming progress message with its full text. */
  private async closeProgress(r: RunRow): Promise<void> {
    if (!r.progress_message_id) return;
    if (r.progress_text.trim() === "" && r.progress_full === 0) {
      // Nothing but the answer was streamed: the result message alone says it.
      try {
        unwrap(await this.room(r).remove(r.group_id, this.actor(), r.progress_message_id));
      } catch (err) {
        this.log.warn("progress remove failed", { runId: r.id, err });
      }
      return;
    }
    try {
      unwrap(
        await this.room(r).edit(r.group_id, this.actor(), {
          t: "edit",
          rid: "bot",
          groupId: r.group_id,
          messageId: r.progress_message_id,
          body: r.progress_text.trimEnd() || TEXT.progress,
          streaming: false,
        } as never),
      );
    } catch (err) {
      this.log.warn("progress close failed", { runId: r.id, err });
    }
  }

  private async askApproval(r: RunRow, approvalId: string, title: string, detail?: string): Promise<void> {
    this.sql.exec("UPDATE runs SET approval_id = ? WHERE id = ?", approvalId, r.id);
    r.approval_id = approvalId;
    const body = TEXT.approval(cut(title, 500), detail ? cut(detail, 4000) : undefined);
    const card: Card = {
      v: 1,
      title: cut(`需要你确认：${title}`, 200),
      tone: "warning",
      blocks: detail ? [{ type: "code", text: cut(detail, 8000) }] : [],
      actions: [
        { id: cut(`approve:${approvalId}`, 64), label: TEXT.approve, style: "primary" },
        { id: cut(`deny:${approvalId}`, 64), label: TEXT.deny, style: "danger" },
      ],
    };
    const cmid = `run:${r.id}:approval:${cut(approvalId, 24)}`;
    try {
      await this.post(r, cmid, body, { card, mentions: [r.requested_by] });
    } catch (err) {
      // A card the room rejects (for example an action id that is too long) falls back to text.
      this.log.warn("approval card failed", { runId: r.id, err });
      try {
        await this.post(r, `${cmid}:text`, body, { mentions: [r.requested_by] });
      } catch (err2) {
        // Nobody can be asked (for example the bot left the group): the answer is no.
        this.log.warn("approval post failed, denying", { runId: r.id, err: err2 });
        this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
        r.approval_id = null;
        if (r.machine_id) {
          await linkStub(this.env, r.machine_id)
            .toRun({ t: "run.approval", runId: r.id, approvalId, decision: "deny" } as never)
            .catch((e) => this.log.warn("approval deny failed", { runId: r.id, err: e }));
        }
        return;
      }
    }
    await this.setStatus(r, "waiting");
    await this.topic(r, "waiting_answer", [r.requested_by]);
  }

  private async finish(r: RunRow, f: Extract<SeqFrame, { t: "run.done" }>): Promise<void> {
    if (f.status === "succeeded") {
      const result = (f.resultText ?? "").trim() || (f.summary ?? "").trim() || TEXT.done;
      this.dropResultFromProgress(r, result);
      await this.end(r, "done", cut(result, MAX_RESULT_CHARS), "done", firstLine(result));
    } else if (f.status === "cancelled") {
      await this.end(r, "cancelled", TEXT.cancelled, "blocked", TEXT.cancelled);
    } else {
      const why = firstLine(f.error ?? f.summary ?? "", 300);
      await this.end(r, "failed", TEXT.failed(why), "blocked", cut(TEXT.failed(why), 280));
    }
    // Follow-ups the CLI never took start a new run with the topic context.
    const left = this.sql
      .exec<{ message_id: string; author_id: string; body: string }>(
        "SELECT message_id, author_id, body FROM inputs WHERE run_id = ? AND accepted = 0 ORDER BY at",
        r.id,
      )
      .toArray();
    if (left.length === 0) return;
    if (this.followUpDepth(r) >= MAX_FOLLOW_UPS) {
      try {
        await this.note(r, TEXT.followUpLimit);
      } catch (err) {
        this.log.warn("follow-up note failed", { runId: r.id, err });
      }
      return;
    }
    const last = left[left.length - 1];
    try {
      await this.start(
        { groupId: r.group_id, topicId: r.topic_id, messageId: last.message_id, authorId: last.author_id, body: last.body, mentioned: true },
        r,
        left.slice(0, -1).map((x) => ({ authorId: x.author_id, body: x.body })),
      );
    } catch (err) {
      this.log.error("follow-up start failed", { runId: r.id, err });
    }
  }

  /** How many runs in a row before `r` were follow-ups. */
  private followUpDepth(r: RunRow): number {
    let depth = 0;
    let parent = r.follow_up_of;
    while (parent && depth < MAX_FOLLOW_UPS) {
      depth++;
      parent = this.run(parent)?.follow_up_of ?? null;
    }
    return depth;
  }

  /** The CLI's last assistant message is also the result: keep it out of the progress text. */
  private dropResultFromProgress(r: RunRow, result: string): void {
    const text = r.progress_text.trimEnd();
    if (!result || !text.endsWith(result)) return;
    const rest = text.slice(0, text.length - result.length).trimEnd();
    r.progress_text = rest;
    this.sql.exec("UPDATE runs SET progress_text = ? WHERE id = ?", rest, r.id);
  }

  /** Final state: closes the progress message, posts `body`, sets the run and topic status. */
  private async end(r: RunRow, status: RunStatus, body: string, topicStatus: TopicStatus, summary: string): Promise<void> {
    const now = Date.now();
    await this.closeProgress(r);
    this.sql.exec("UPDATE runs SET approval_id = NULL WHERE id = ?", r.id);
    r.approval_id = null;
    try {
      await this.post(r, `run:${r.id}:result`, body);
    } catch (err) {
      this.log.warn("result post failed", { runId: r.id, err });
    }
    await this.setStatus(r, status, { ended_at: now, summary: cut(summary, 280) || null });
    await this.topic(r, topicStatus);
  }

  // -------------------------------------------------------------------------
  // Stall detection
  // -------------------------------------------------------------------------

  private async scheduleStall(): Promise<void> {
    const stall = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(last_event_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL`,
      )
      .one().at;
    const stuck = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(created_at) AS at FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL`,
      )
      .one().at;
    const times = [stall === null ? null : stall + STALL_MS, stuck === null ? null : stuck + START_STUCK_MS].filter(
      (x): x is number => x !== null,
    );
    if (times.length === 0) return;
    const at = Math.min(...times);
    const cur = await this.ctx.storage.getAlarm();
    if (cur === null || cur > at || cur < Date.now()) await this.ctx.storage.setAlarm(at);
  }

  async alarm(): Promise<void> {
    const now = Date.now();
    const due = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'running' AND stalled = 0 AND approval_id IS NULL AND last_event_at <= ?`,
        now - STALL_MS,
      )
      .toArray();
    for (const { id } of due) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.external || r.ended_at !== null || r.stalled || r.status !== "running" || r.last_event_at > now - STALL_MS) return;
        await this.markStalled(r);
      });
    }
    // Runs whose start broke before they reached a machine (for example a reset mid-start).
    const stuck = this.sql
      .exec<{ id: string }>(
        `SELECT id FROM runs WHERE ${ACTIVE} AND external = 0 AND status = 'created' AND machine_id IS NULL AND created_at <= ?`,
        now - START_STUCK_MS,
      )
      .toArray();
    for (const { id } of stuck) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id || r.status !== "created") return;
        await this.end(r, "failed", TEXT.failed(""), "blocked", TEXT.failed(""));
      });
    }
    await this.scheduleStall();
  }

  /** No progress: one note, the run and the topic show blocked until events come again. */
  private async markStalled(r: RunRow): Promise<void> {
    if (r.stalled || r.ended_at !== null || r.approval_id) return;
    this.sql.exec("UPDATE runs SET stalled = 1 WHERE id = ?", r.id);
    r.stalled = 1;
    try {
      await this.note(r, TEXT.stalled);
    } catch (err) {
      this.log.warn("stall note failed", { runId: r.id, err });
    }
    await this.setStatus(r, "blocked");
    await this.topic(r, "blocked");
  }

  // -------------------------------------------------------------------------
  // Stopping runs (members, and a deactivated bot)
  // -------------------------------------------------------------------------

  /** A member stops a run: the machine gets run.cancel, and the run ends here at once. */
  async cancel(botId: string, runId: string, actorId: string): Promise<Result<BotRun>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.run(runId);
        if (!r) throw new ApiError("not_found", "run not found");
        if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
        await this.stop(r);
        this.log.info("run cancelled", { runId, actorId });
        return this.toRun(r);
      });
    });
  }

  private async stop(r: RunRow): Promise<void> {
    if (r.machine_id && !r.external) {
      try {
        await linkStub(this.env, r.machine_id).toRun({ t: "run.cancel", runId: r.id } as never);
      } catch (err) {
        this.log.warn("run.cancel failed", { runId: r.id, err });
      }
    }
    this.planBuckets.delete(r.id);
    if (r.external) {
      await this.setStatus(r, "cancelled", { ended_at: Date.now(), summary: TEXT.cancelled });
      return;
    }
    await this.end(r, "cancelled", TEXT.cancelled, "open", TEXT.cancelled);
  }

  /** The bot was deactivated: stop its active runs and give up its topics. */
  async deactivated(botId: string): Promise<void> {
    this.bind(botId);
    const active = this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${ACTIVE}`).toArray();
    for (const { id } of active) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (r && r.ended_at === null) await this.stop(r);
      });
    }
    const groups = this.sql.exec<{ group_id: string }>("SELECT DISTINCT group_id FROM runs").toArray();
    for (const { group_id } of groups) {
      try {
        unwrap(await roomStub(this.env, group_id).rpc(group_id, "bots.release", null, { botId }));
      } catch (err) {
        this.log.warn("claim release failed", { groupId: group_id, err });
      }
    }
  }

  // -------------------------------------------------------------------------
  // Runs of external bots and plans over HTTP (docs/protocol.md, "Run plans")
  // -------------------------------------------------------------------------

  /**
   * Starts a run of this (external) bot in the topic, or returns its active run there. The API
   * Worker checked that the caller is this bot, not runner-backed, and a member of the group.
   */
  async startExternal(botId: string, groupId: string, topicId: string, plan?: RunPlan): Promise<Result<BotRun>> {
    return result(() => this.doStartExternal(botId, groupId, topicId, plan));
  }

  private async doStartExternal(botId: string, groupId: string, topicId: string, plan?: RunPlan): Promise<BotRun> {
    this.bind(botId);
    const active = this.activeRun(topicId);
    if (active) {
      if (plan) return this.doSetPlan(botId, active.id, plan);
      return this.toRun(active);
    }
    // The topic must exist in the group (the room also checks membership).
    unwrap(await roomStub(this.env, groupId).rpc(groupId, "bots.topicExists", this.actor(), { topicId }));
    const now = Date.now();
    const runId = ulid(now);
    return this.serial(runId, async () => {
      if (this.activeRun(topicId)) {
        const again = this.activeRun(topicId) as RunRow;
        return this.toRun(again);
      }
      this.sql.exec(
        `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, started_at, updated_at, last_event_at, external, plan)
         VALUES (?, ?, ?, 'running', '', '', ?, ?, ?, ?, 1, ?)`,
        runId,
        groupId,
        topicId,
        now,
        now,
        now,
        now,
        plan && plan.length > 0 ? JSON.stringify(plan) : null,
      );
      await this.env.DB.prepare(
        "INSERT INTO bot_runs (id, bot_id, group_id, topic_id, machine_id, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
      )
        .bind(runId, botId, groupId, topicId, now)
        .run();
      const r = this.run(runId) as RunRow;
      await this.setStatus(r, "running");
      return this.toRun(r);
    });
  }

  /** Replaces the plan of an active run of this bot (HTTP; the caller is the bot). */
  async setPlan(botId: string, runId: string, plan: RunPlan): Promise<Result<BotRun>> {
    return result(() => this.doSetPlan(botId, runId, plan));
  }

  private async doSetPlan(botId: string, runId: string, plan: RunPlan): Promise<BotRun> {
    this.bind(botId);
    let bucket = this.planBuckets.get(runId);
    if (!bucket) {
      bucket = new TokenBucket(PLAN_RATE.rate, PLAN_RATE.burst);
      this.planBuckets.set(runId, bucket);
    }
    if (!bucket.take()) throw new ApiError("rate_limited", "too many plan updates for this run");
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "run not found");
      if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
      if (this.storePlan(r, plan)) await this.setStatus(r, r.status);
      return this.toRun(r);
    });
  }

  /** Ends a run that this external bot started. */
  async endExternal(botId: string, runId: string, status: "done" | "failed" | "cancelled", summary?: string): Promise<Result<BotRun>> {
    return result(() => this.doEndExternal(botId, runId, status, summary));
  }

  private async doEndExternal(botId: string, runId: string, status: "done" | "failed" | "cancelled", summary?: string): Promise<BotRun> {
    this.bind(botId);
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r) throw new ApiError("not_found", "run not found");
      if (!r.external) throw new ApiError("forbidden", "runs of a runner end on the machine");
      if (r.ended_at !== null) throw new ApiError("conflict", "the run ended");
      this.planBuckets.delete(runId);
      await this.setStatus(r, status, { ended_at: Date.now(), summary: summary ? cut(summary, 280) : r.summary });
      return this.toRun(r);
    });
  }

  // -------------------------------------------------------------------------
  // Reads (API Worker)
  // -------------------------------------------------------------------------

  async getRun(botId: string, runId: string): Promise<BotRun | null> {
    this.bind(botId);
    const r = this.run(runId);
    return r ? this.toRun(r) : null;
  }

  async events(
    botId: string,
    runId: string,
    afterSeq = 0,
    limit = DEFAULT_PAGE,
  ): Promise<{ run: BotRun; entries: unknown[]; hasMore: boolean; lastSeq: number } | null> {
    this.bind(botId);
    const r = this.run(runId);
    if (!r) return null;
    const n = Math.max(1, Math.min(limit, MAX_PAGE));
    const rows = this.sql
      .exec<{ frame: string }>("SELECT frame FROM run_log WHERE run_id = ? AND seq > ? ORDER BY seq LIMIT ?", runId, afterSeq, n + 1)
      .toArray();
    return {
      run: this.toRun(r),
      entries: rows.slice(0, n).map((x) => JSON.parse(x.frame)),
      hasMore: rows.length > n,
      lastSeq: r.last_seq,
    };
  }
}
