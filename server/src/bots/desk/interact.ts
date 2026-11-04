// BotDesk interaction loop (INTERACT §6.2–§6.4): live card events and page events for this
// runner member's cards and pages, coalesced to the newest event per block in each 3 s window
// and given to the topic's run as `run.interaction` (or a text envelope for older daemons), or
// starting a short follow-up run within the limits; the `expect` of an ack; and the `chat-run`
// outputs of a run (RunEvent `output`), performed as the bot with the rules of the matching
// WebSocket frames and answered with `run.output.result`.
import { ClientFrame, type ExpectMode, MAX_DESK_NOTE_CHARS, type RunInteractionFrame } from "@chat/protocol";
import { isGroupMember } from "../../core/db";
import { roomStub } from "../../core/env";
import { ApiError, type Result, schemaReason, toErrorBody, unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { type FrameContext, frameHandlers, frameValidators } from "../../core/registry";
import { setDeskNote } from "../../desknote";
import { answerBlocks, cardEventText, pageEventText } from "../answers";
import { linkStub } from "../env";
import { ETA_MS, type InteractionIn } from "../interact";
import type { CardAnswerIn } from "../room";
import { loadRunner } from "../store";
import { type RunRow, result } from "./core";
import { DeskTakeover } from "./takeover";

/** One coalescing window per card (or page version): the newest event per block in 3 s. */
export const COALESCE_MS = 3000;
/** Short follow-up runs started by live events: 1 per card at a time, 6 per hour per card, 30 per hour per member. */
export const SHORT_RUNS_PER_CARD_HOUR = 6;
export const SHORT_RUNS_PER_BOT_HOUR = 30;
/** How long `chat-run` waits; an output older than this is not answered any more. */
export const OUTPUT_TIMEOUT_MS = 15_000;
/** An output is performed only this soon after the daemon made it, on the server clock, so its result reaches `chat-run` in time. */
export const OUTPUT_DEADLINE_MS = OUTPUT_TIMEOUT_MS - 3000;

const HOUR_MS = 3600_000;
/** Room RPC of the 产出 bridge (files/bridge.ts): the receiver of a page `ask` answers it. */
const ARTIFACT_REPLY_RPC = "artifacts.reply";

/** What the client is told: who responds and how soon. */
export interface DeskExpect {
  mode: ExpectMode;
  etaMs?: number;
}

type PendingRow = { card_key: string; block_key: string; run_id: string; frame: string; at: number };

/** `chat-run` ops and the client frame each one is performed as. */
const OUTPUT_FRAME: Record<string, string> = {
  "card.post": "send",
  "card.patch": "card.patch",
  "interaction.resolve": "interaction.resolve",
  "artifact.data": "artifact.data",
  "artifact.reply": "artifact.reply",
};

export abstract class DeskInteract extends DeskTakeover {
  // -------------------------------------------------------------------------
  // Expect (§6.4)
  // -------------------------------------------------------------------------

  protected runExpect(r: RunRow): DeskExpect {
    return { mode: "run", etaMs: r.in_turn ? ETA_MS.runInTurn : ETA_MS.runIdle };
  }

  /** The `expect` of a stored answer to this bot's card in the topic (no limits: answers always start a run). */
  async expectFor(botId: string, topicId: string): Promise<DeskExpect> {
    this.bind(botId);
    if (!(await this.botActive())) return { mode: "none" };
    const runner = await loadRunner(this.env.DB, botId);
    if (!runner?.enabled) return { mode: "none" };
    const active = this.activeRun(topicId);
    if (active && !active.external) return this.runExpect(active);
    return { mode: "starting", etaMs: ETA_MS.starting };
  }

  /** The requester of this bot's active run in the topic (INTERACT §3.5 "requester"). */
  async requesterFor(botId: string, topicId: string): Promise<string | null> {
    this.bind(botId);
    const r = this.activeRun(topicId);
    return r ? (this.requester(r)[0] ?? null) : null;
  }

  // -------------------------------------------------------------------------
  // Live card events and page events (§6.2)
  // -------------------------------------------------------------------------

  /**
   * A live card event or a page event for this bot. The run of the version (pages), else the
   * topic's active run, gets it; with no run, a short follow-up run starts within the limits.
   * Returns the ack's `expect`; "none" when nobody will respond.
   */
  async interaction(botId: string, x: InteractionIn): Promise<DeskExpect> {
    this.bind(botId);
    return this.serial("interact", async () => {
      if (!(await this.botActive())) return { mode: "none" };
      const runner = await loadRunner(this.env.DB, botId);
      if (!runner?.enabled) return { mode: "none" };
      const byRun = x.runId ? this.run(x.runId) : undefined;
      const target = byRun && byRun.ended_at === null && !byRun.external ? byRun : this.activeRun(x.topicId);
      const names = await this.names([x.actorId]);
      const actor = { id: x.actorId, name: names.get(x.actorId) ?? x.actorId };
      if (target && !target.external) {
        await this.queueInteraction(target, x, actor);
        return this.runExpect(target);
      }
      // A page `ask` with no run to answer it is declined at once, so the page's promise settles.
      if (x.source.kind === "artifact" && x.source.ask) {
        await this.declinePageAsk(x);
        return { mode: "none" };
      }
      if (target) return { mode: "none" };
      // cards2 (F5): a retract never starts a run (no run has the answer).
      if (x.event === "answer.retract") return { mode: "none" };
      // No run: a short follow-up run, within the limits.
      const key = cardKey(x);
      const now = Date.now();
      const perCard = this.sql
        .exec<{ n: number }>("SELECT count(*) AS n FROM interact_runs WHERE card_key = ? AND at > ?", key, now - HOUR_MS)
        .one().n;
      const perBot = this.sql.exec<{ n: number }>("SELECT count(*) AS n FROM interact_runs WHERE at > ?", now - HOUR_MS).one().n;
      if (perCard >= SHORT_RUNS_PER_CARD_HOUR || perBot >= SHORT_RUNS_PER_BOT_HOUR) return { mode: "none" };
      const inputId = ulid();
      await this.start({
        groupId: x.groupId,
        topicId: x.topicId,
        messageId: `interact:${x.eventId}`,
        authorId: x.actorId,
        body: this.interactionText(x, actor, inputId),
        mentioned: true,
      });
      const started = this.activeRun(x.topicId);
      if (!started || started.trigger_message_id !== `interact:${x.eventId}`) return { mode: "none" };
      this.sql.exec("INSERT OR IGNORE INTO interact_runs (run_id, card_key, at) VALUES (?, ?, ?)", started.id, key, now);
      if (started.ended_at !== null) return { mode: "none" };
      return { mode: "starting", etaMs: ETA_MS.starting };
    });
  }

  /** Answers a page `ask` that no run of this member answers: declined, reason `run.ended`. */
  protected async declinePageAsk(x: InteractionIn): Promise<void> {
    if (x.source.kind !== "artifact") return;
    try {
      unwrap(
        await roomStub(this.env, x.groupId).rpc(x.groupId, ARTIFACT_REPLY_RPC, this.actor(), {
          eventId: x.source.eventId,
          ok: false,
          error: { code: "declined", message: "no run of this member is open", reason: "run.ended" },
        }),
      );
    } catch (err) {
      this.log.warn("page ask decline failed", { groupId: x.groupId, err });
    }
  }

  /** The text form (with the envelope) for an older daemon or a run prompt. */
  protected interactionText(x: InteractionIn, actor: { id: string; name: string }, inputId: string): string {
    if (x.source.kind === "artifact") {
      return pageEventText(
        {
          artifactId: x.source.artifactId,
          version: x.source.version,
          name: x.event,
          payload: x.value,
          eventId: x.source.eventId,
          kind: x.source.ask ? "ask" : "emit",
        },
        actor,
        x.title ?? null,
        inputId,
      );
    }
    const data: Record<string, unknown> & { event: string; messageId: string } = {
      messageId: x.source.messageId,
      rev: x.source.rev,
      eventId: x.eventId,
      event: x.event,
    };
    if (x.source.blockId) data.blockId = x.source.blockId;
    if (x.source.actionId) data.actionId = x.source.actionId;
    if (x.value !== undefined) data.value = x.value;
    if (x.block !== undefined) data.block = x.block;
    return cardEventText(data, actor, x.title ?? null, inputId);
  }

  protected interactionFrame(r: RunRow, x: InteractionIn, actor: { id: string; name: string }): RunInteractionFrame & { fallback: Record<string, unknown> } {
    const inputId = ulid();
    const frame: RunInteractionFrame & { fallback: Record<string, unknown> } = {
      t: "run.interaction",
      runId: r.id,
      inputId,
      source: x.source,
      event: x.event,
      actor,
      fallback: { text: this.interactionText(x, actor, inputId), authorId: actor.id },
    };
    if (x.value !== undefined) frame.value = x.value as never;
    if (x.eventId) frame.eventId = x.eventId;
    if (x.block !== undefined) frame.block = x.block as never;
    return frame;
  }

  /**
   * Coalescing per card: the first event of a window goes out at once; later ones in the same
   * 3 s keep only the newest per block and go out when the window ends. Page `ask` events are
   * never merged (each one waits for its reply).
   */
  protected async queueInteraction(r: RunRow, x: InteractionIn, actor: { id: string; name: string }): Promise<void> {
    const frame = this.interactionFrame(r, x, actor);
    const key = cardKey(x);
    const ask = x.source.kind === "artifact" && x.source.ask;
    const block = ask ? `ask:${x.eventId}` : blockKey(x);
    const now = Date.now();
    // Not on a machine yet: kept until the run is accepted.
    if (!r.machine_id || r.status === "created") {
      this.keepPending(key, block, r.id, frame, now);
      return;
    }
    if (ask) {
      await this.sendInteraction(r, frame);
      return;
    }
    const until = this.sql.exec<{ until: number }>("SELECT until FROM interact_windows WHERE card_key = ?", key).toArray()[0]?.until ?? 0;
    if (until > now || this.hasPending(key)) {
      this.keepPending(key, block, r.id, frame, now);
      await this.scheduleStall();
      return;
    }
    this.sql.exec("INSERT OR REPLACE INTO interact_windows (card_key, run_id, until) VALUES (?, ?, ?)", key, r.id, now + COALESCE_MS);
    await this.sendInteraction(r, frame);
    await this.scheduleStall();
  }

  private hasPending(key: string): boolean {
    return this.sql.exec("SELECT 1 FROM interact_pending WHERE card_key = ? LIMIT 1", key).toArray().length > 0;
  }

  private keepPending(key: string, block: string, runId: string, frame: unknown, at: number): void {
    this.sql.exec(
      "INSERT OR REPLACE INTO interact_pending (card_key, block_key, run_id, frame, at) VALUES (?, ?, ?, ?, ?)",
      key,
      block,
      runId,
      JSON.stringify(frame),
      at,
    );
  }

  protected async sendInteraction(r: RunRow, frame: RunInteractionFrame & { fallback: Record<string, unknown> }): Promise<void> {
    // accepted = 1: a live event is not leftover input for a follow-up run.
    this.sql.exec(
      "INSERT INTO inputs (id, run_id, message_id, author_id, body, at, accepted) VALUES (?, ?, ?, ?, ?, ?, 1)",
      frame.inputId,
      r.id,
      `interaction:${frame.inputId}`,
      frame.actor.id,
      String(frame.fallback.text),
      Date.now(),
    );
    if (!r.machine_id || r.external) return;
    try {
      await linkStub(this.env, r.machine_id).toRun(frame as never);
    } catch (err) {
      this.log.warn("run.interaction failed", { runId: r.id, err });
    }
  }

  /** Sends what waited for a window to end (or for the run's machine), newest per block. */
  protected async flushInteractions(onlyRun?: string): Promise<void> {
    const now = Date.now();
    const rows = this.sql.exec<PendingRow>("SELECT * FROM interact_pending ORDER BY at").toArray() as PendingRow[];
    const windows = new Map(
      this.sql
        .exec<{ card_key: string; until: number }>("SELECT card_key, until FROM interact_windows")
        .toArray()
        .map((w) => [w.card_key, w.until]),
    );
    const sentCards = new Set<string>();
    for (const row of rows) {
      if (onlyRun && row.run_id !== onlyRun) continue;
      if ((windows.get(row.card_key) ?? 0) > now) continue;
      const r = this.run(row.run_id);
      this.sql.exec("DELETE FROM interact_pending WHERE card_key = ? AND block_key = ?", row.card_key, row.block_key);
      if (!r || r.ended_at !== null) continue;
      if (!r.machine_id || r.status === "created") {
        this.keepPending(row.card_key, row.block_key, row.run_id, JSON.parse(row.frame), row.at);
        continue;
      }
      await this.sendInteraction(r, JSON.parse(row.frame));
      sentCards.add(row.card_key);
    }
    for (const key of sentCards) {
      this.sql.exec("UPDATE interact_windows SET until = ? WHERE card_key = ?", now + COALESCE_MS, key);
    }
    this.sql.exec("DELETE FROM interact_windows WHERE until <= ? AND card_key NOT IN (SELECT card_key FROM interact_pending)", now);
  }

  protected override async scheduleStall(): Promise<void> {
    await super.scheduleStall();
    const due = this.sql
      .exec<{ at: number | null }>(
        `SELECT min(w.until) AS at FROM interact_windows w WHERE w.card_key IN (SELECT card_key FROM interact_pending)`,
      )
      .one().at;
    if (due === null) return;
    const cur = await this.ctx.storage.getAlarm();
    if (cur === null || cur > due || cur < Date.now()) await this.ctx.storage.setAlarm(due);
  }

  override async alarm(): Promise<void> {
    await this.serial("interact", () => this.flushInteractions());
    await super.alarm();
  }

  override async runAccepted(runId: string, machineId: string): Promise<void> {
    await super.runAccepted(runId, machineId);
    await this.serial("interact", () => this.flushInteractions(runId));
  }

  // -------------------------------------------------------------------------
  // Stored answers (§6.2): run.answer, or run.input with the envelope
  // -------------------------------------------------------------------------

  /** `run.answer` for a 1.1 daemon, with the `run.input` an older one gets instead. */
  protected answerFrame(r: RunRow, inputId: string, text: string, a: CardAnswerIn): Record<string, unknown> {
    const fallback: Record<string, unknown> = { text, authorId: a.actorId };
    if (a.answer.actionId) fallback.answer = { ...a.answer, actionId: a.answer.actionId, actorId: a.actorId };
    if (!a.answer.answerId || !a.answer.askId) return { t: "run.input", runId: r.id, inputId, ...fallback };
    const frame: Record<string, unknown> = {
      t: "run.answer",
      runId: r.id,
      inputId,
      source: { kind: "card", messageId: a.answer.messageId, askId: a.answer.askId, answerId: a.answer.answerId, rev: a.answer.rev },
      event: a.answer.event ?? "action.press",
      actor: { id: a.actorId, name: a.actorName },
      fallback,
    };
    for (const k of ["actionId", "blockId", "value", "inputs", "editedValues", "eventId"] as const) {
      if (a.answer[k] !== undefined) frame[k] = a.answer[k];
    }
    if (a.tally) frame.tally = a.tally;
    if (a.answer.blockId) {
      const block = answerBlocks(a.card, a.answer)[a.answer.blockId];
      if (block !== undefined) frame.block = block;
    }
    return frame;
  }

  // -------------------------------------------------------------------------
  // chat-run outputs (§6.3)
  // -------------------------------------------------------------------------

  /** Performs one RunEvent `output` as the bot and answers it with `run.output.result`. */
  protected override async output(r: RunRow, ev: { outputId?: unknown; op?: unknown; body?: unknown }, f: { at: number; serverAt?: number }): Promise<void> {
    const outputId = typeof ev.outputId === "string" ? ev.outputId : "";
    if (!outputId) return;
    // An output that `chat-run` already reported as timed out is not performed (for example one
    // replayed after a reconnect). The age is on the server clock (link `serverAt`); a frame
    // stored before that field keeps the old machine-clock rule.
    const expired = f.serverAt !== undefined ? Date.now() - f.serverAt > OUTPUT_DEADLINE_MS : Date.now() - f.at > OUTPUT_TIMEOUT_MS * 4;
    const res: Result<unknown> = expired
      ? { ok: false, error: { code: "conflict", reason: "output.expired", message: "chat-run stopped waiting for this output; it was not performed" } }
      : await this.performOutput(r, String(ev.op ?? ""), outputId, ev.body);
    const frame: Record<string, unknown> = { t: "run.output.result", runId: r.id, outputId, ok: res.ok };
    if (res.ok) {
      if (res.value !== undefined) frame.result = res.value;
    } else {
      frame.error = { code: res.error.code, message: res.error.message, ...(res.error.reason ? { reason: res.error.reason } : {}) };
    }
    if (!r.machine_id || r.external) return;
    try {
      await linkStub(this.env, r.machine_id).toRun(frame as never);
    } catch (err) {
      this.log.warn("run.output.result failed", { runId: r.id, err });
    }
  }

  /** The client frame an output op stands for, in the run's group (a run never acts in another group). */
  protected outputFrame(r: RunRow, op: string, outputId: string, body: unknown): Record<string, unknown> & { t: string } {
    const t = OUTPUT_FRAME[op];
    if (!t) throw new ApiError("invalid", "unknown output op", "output.unknown_op");
    if (!body || typeof body !== "object" || Array.isArray(body)) throw new ApiError("invalid", "output body is an object");
    const b = { ...(body as Record<string, unknown>) };
    const base = { rid: `out-${outputId}`.slice(0, 128), groupId: r.group_id };
    if (op === "card.post") {
      const card = b.cardV2 ?? b.card;
      const frame: Record<string, unknown> & { t: string } = {
        t,
        ...base,
        topicId: r.topic_id,
        // The daemon's idempotency key (one per command in the run), so a retry after a lost
        // result posts once; without it the output id, which is new on each retry.
        clientMsgId: typeof b.clientMsgId === "string" && b.clientMsgId ? b.clientMsgId.slice(0, 64) : `run:${r.id}:out:${outputId}`.slice(0, 64),
        body: typeof b.body === "string" ? b.body : "",
        cardV2: card,
      };
      const cause = b.causeEventId ?? b.cause;
      if (cause !== undefined) frame.causeEventId = cause;
      return frame;
    }
    // Output ids restart at o1 in every run, so the default keys name the run too.
    if (op === "card.patch") {
      return { ...b, t, ...base, clientPatchId: typeof b.clientPatchId === "string" ? b.clientPatchId : `o:${r.id}:${outputId}`.slice(0, 64) };
    }
    if (op === "artifact.data") {
      return { ...b, t, ...base, clientPushId: typeof b.clientPushId === "string" ? b.clientPushId : `o:${r.id}:${outputId}`.slice(0, 64) };
    }
    if (op === "artifact.reply" && typeof b.decline === "string") {
      return { t, ...base, eventId: b.eventId, ok: false, error: { code: "declined", message: b.decline.slice(0, 200) } };
    }
    return { ...b, t, ...base };
  }

  protected async performOutput(r: RunRow, op: string, outputId: string, body: unknown): Promise<Result<unknown>> {
    return result(async () => {
      if (r.kind === "aside") throw new ApiError("forbidden", "a side chat run is read only", "output.read_only");
      // 1.1: the desk note has no WebSocket frame; it is `PUT /v1/desk-note` in the run's group.
      if (op === "desk.note") {
        const text = (body as { text?: unknown } | null)?.text;
        if (typeof text !== "string" || text.length > 4 * MAX_DESK_NOTE_CHARS) throw new ApiError("invalid", "invalid output: text is a string");
        return setDeskNote(this.env, r.group_id, this.actor(), text);
      }
      const raw = this.outputFrame(r, op, outputId, body);
      const handler = frameHandlers.get(raw.t);
      const schema = raw.t === "send" ? ClientFrame : handler?.schema ?? ClientFrame;
      const parsed = schema.safeParse(raw);
      if (!parsed.success) {
        const issue = parsed.error.issues[0];
        throw new ApiError("invalid", `invalid output: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim(), schemaReason(raw, issue?.message, issue?.path));
      }
      const frame = parsed.data as Record<string, unknown> & { t: string };
      const c = this.outputContext();
      for (const v of [...(frameValidators.get("*") ?? []), ...(frameValidators.get(frame.t) ?? [])]) await v(frame as never, c);
      if (frame.t === "send") {
        const sent = await this.room(r).send(r.group_id, this.actor(), frame as never);
        if (!sent.ok) throw new ApiError(sent.error.code, sent.error.message, sent.error.reason);
        return { messageId: sent.value.message.id, message: sent.value.message };
      }
      if (!handler) throw new ApiError("invalid", "the server does not take this output yet", "output.not_supported");
      const out = await handler.handle(frame, c);
      return out && typeof out === "object" && "reply" in out ? undefined : out;
    });
  }

  /** The frame context of an output: the bot as the actor; nothing is sent back on a socket. */
  protected outputContext(): FrameContext {
    const botId = this.botId as string;
    return {
      env: this.env,
      actor: this.actor(),
      log: this.log,
      sql: this.sql,
      reply: () => {},
      broadcast: () => {},
      requireMember: async (groupId: string) => {
        if (!(await isGroupMember(this.env.DB, groupId, botId))) throw new ApiError("forbidden", "not a member of this group");
      },
    };
  }
}

function cardKey(x: InteractionIn): string {
  return x.source.kind === "card" ? `card:${x.source.messageId}` : `artifact:${x.source.artifactId}:${x.source.version}`;
}

function blockKey(x: InteractionIn): string {
  if (x.source.kind === "artifact") return `event:${x.event}`;
  return x.source.blockId ? `block:${x.source.blockId}` : `action:${x.source.actionId ?? ""}`;
}
