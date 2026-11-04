// BotDesk runs: starting a run from a mention (claim, ack, machine, prompt, assign) and the
// MachineLink answers to an assignment (accepted, rejected, lost).
import { type BotRunner, RUN_KIND_ASIDE, type RunAssignFrame, type TopicContext } from "@chat/protocol";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { LEASE_MS, MAX_RUN_MS, type MachineView, TEXT, buildAsidePrompt, buildPrompt, candidates, cut, hasFeature, pickMachine } from "../dispatch";
import { linkStub } from "../env";
import type { ClaimResult, SourceContext, TopicMessage } from "../room";
import { refsText } from "../refs";
import { loadMachines, loadRunner } from "../store";
import { DeskCore, QUEUED, type RunRow } from "./core";

/** What started a run (meta `request:{runId}`): the message and the follow-ups it carried. */
export interface RunRequest {
  m: TopicMessage;
  followUps: { authorId: string; body: string }[];
}

export abstract class DeskRuns extends DeskCore {
  // -------------------------------------------------------------------------
  // Starting a run
  // -------------------------------------------------------------------------

  protected async start(m: TopicMessage, followUpOf?: RunRow, followUps: { authorId: string; body: string }[] = [], retryOf?: string): Promise<void> {
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
    // A side chat topic (单独问) gets a read-only run that answers only the asker.
    const kind = claim.contextOf ? RUN_KIND_ASIDE : null;
    this.sql.exec(
      `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at, follow_up_of, kind, retry_of)
       VALUES (?, ?, ?, 'created', ?, ?, ?, ?, ?, ?, ?, ?)`,
      runId,
      m.groupId,
      m.topicId,
      m.authorId,
      m.messageId,
      now,
      now,
      now,
      followUpOf?.id ?? null,
      kind,
      retryOf ?? null,
    );
    // The request as it came, so the run can be retried with the same one.
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `request:${runId}`, JSON.stringify({ m, followUps } satisfies RunRequest));
    const r = this.run(runId) as RunRow;
    await this.serial(runId, async () => {
      try {
        await this.env.DB.prepare(
          "INSERT OR IGNORE INTO bot_runs (id, bot_id, group_id, topic_id, machine_id, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
        )
          .bind(runId, botId, m.groupId, m.topicId, now)
          .run();
        // Answers and live events start runs too: a card answer acknowledges on the answered card,
        // other synthetic keys get no reaction.
        if (m.ackMessageId) await this.ack(r, m.ackMessageId);
        else if (!m.messageId.includes(":")) await this.ack(r);
        await this.setStatus(r, "created");
        await this.topic(r, "in_progress");

        const machines = await loadMachines(this.env);
        const machine = this.pick(r, runner, machines);
        // Every machine that could take it is busy: the run waits (排队) instead of failing.
        const busy = !machine && this.couldTake(r, runner, machines);
        if (!machine && !busy) {
          const why = kind ? TEXT.noAsideMachine : TEXT.noMachine;
          return this.end(r, "failed", why, "blocked", why);
        }

        const prompt = claim.contextOf
          ? await this.asidePrompt(m, claim, claim.contextOf, followUps)
          : await this.topicPrompt(m, claim, followUps, hasFeature(machine, "acceptance"));
        // Sources of refs the run saw: later run.input texts give their context only once.
        if (m.refs && m.refs.length > 0) {
          this.seeRefs(runId, [...claim.messages.map((x) => x.id), ...m.refs.map((x) => x.messageId)]);
        }
        if (!machine) {
          this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
          r.queued_at = Date.now();
          this.sql.exec("UPDATE runs SET queued_at = ? WHERE id = ?", r.queued_at, r.id);
          await this.setStatus(r, "created");
          await this.scheduleStall();
          return;
        }
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

  /** A machine would take the run but has no free slot (`maxParallelRuns`). */
  protected couldTake(r: RunRow, runner: BotRunner, machines: MachineView[]): boolean {
    const roomy = machines.map((x) => (x.info ? { ...x, info: { ...x.info, maxParallelRuns: Number.MAX_SAFE_INTEGER } } : x));
    const usable = r.kind === RUN_KIND_ASIDE ? roomy.filter((x) => hasFeature(x, "aside")) : roomy;
    return candidates(usable, runner, Date.now()).length > 0;
  }

  /**
   * Gives queued runs (排队) a machine, oldest first, while machines have free slots. Called
   * when a run of this bot ends and by the alarm (slots also free up in other bots' desks).
   */
  protected async dispatchQueued(): Promise<void> {
    const queued = this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${QUEUED} ORDER BY queued_at, id`).toArray();
    if (queued.length === 0) return;
    const runner = await loadRunner(this.env.DB, this.botId as string);
    if (!runner?.enabled) return;
    let changed = false;
    for (const { id } of queued) {
      const placed = await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null || r.machine_id || r.status !== "created" || !r.queued_at) return false;
        const prompt = this.meta(`prompt:${id}`);
        const machine = prompt === null ? null : this.pick(r, runner, await loadMachines(this.env), JSON.parse(r.excluded) as string[]);
        if (!machine || prompt === null) return null;
        await this.assign(r, runner, machine, prompt);
        this.sql.exec("UPDATE runs SET queued_at = NULL WHERE id = ?", id);
        r.queued_at = null;
        await this.setStatus(r, "created");
        return true;
      });
      if (placed === null) break; // No free machine: the rest keep waiting in order.
      changed ||= placed;
    }
    // The runs still waiting moved up: tell clients their new place.
    if (changed) {
      for (const { id } of this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${QUEUED}`).toArray()) {
        await this.serial(id, async () => {
          const r = this.run(id);
          if (r && r.queued_at && r.ended_at === null && !r.machine_id) await this.setStatus(r, "created");
        });
      }
    }
  }

  /** The best machine for the run; an aside run needs a daemon that runs it read-only. The
   * machine of the topic's last session comes first, so the run can continue that session. */
  protected pick(r: RunRow, runner: BotRunner, machines: MachineView[], excluded: string[] = []): MachineView | null {
    const usable = r.kind === RUN_KIND_ASIDE ? machines.filter((x) => hasFeature(x, "aside")) : machines;
    const prev = this.lastSession(r, runner);
    if (prev) {
      const same = pickMachine(usable.filter((x) => x.id === prev.machine_id), runner, Date.now(), excluded);
      if (same) return same;
    }
    return pickMachine(usable, runner, Date.now(), excluded);
  }

  /** The topic's last ended run of the same kind and runtime with a CLI session, if any. */
  protected lastSession(r: RunRow, runner: BotRunner): RunRow | undefined {
    const prev = this.sql
      .exec<RunRow>(
        "SELECT * FROM runs WHERE topic_id = ? AND id != ? AND ended_at IS NOT NULL AND kind IS ? ORDER BY created_at DESC LIMIT 1",
        r.topic_id,
        r.id,
        r.kind,
      )
      .toArray()[0] as RunRow | undefined;
    return prev?.session_id && prev.machine_id && prev.runtime === runner.runtime && !prev.external ? prev : undefined;
  }

  protected async topicPrompt(m: TopicMessage, claim: ClaimResult, followUps: { authorId: string; body: string }[], acceptance = false): Promise<string> {
    const botId = this.botId as string;
    const refIds = (m.refs ?? []).map((x) => x.fromId).filter((x): x is string => Boolean(x));
    const names = await this.names([...claim.messages.map((x) => x.authorId), m.authorId, botId, ...refIds]);
    const seen = new Set(claim.messages.map((x) => x.id).filter((x): x is string => Boolean(x)));
    const refs = m.refs && m.refs.length > 0 ? refsText(names.get(m.authorId) ?? m.authorId, m.refs, names, seen) : undefined;
    return buildPrompt({
      botName: names.get(botId) ?? "bot",
      title: claim.title,
      messages: claim.messages.filter((x) => x.body !== "" && x.id !== m.messageId),
      names,
      request: { authorId: m.authorId, body: m.body },
      followUps,
      refs,
      acceptance,
    });
  }

  /** Remembers source messages a run saw (prompt messages and refs with their context). */
  protected seeRefs(runId: string, messageIds: (string | undefined)[]): void {
    for (const id of new Set(messageIds)) {
      if (id) this.sql.exec("INSERT OR IGNORE INTO run_refs_seen (run_id, message_id) VALUES (?, ?)", runId, id);
    }
  }

  protected seenRefs(runId: string): Set<string> {
    return new Set(
      this.sql
        .exec<{ message_id: string }>("SELECT message_id FROM run_refs_seen WHERE run_id = ?", runId)
        .toArray()
        .map((x) => x.message_id),
    );
  }

  /** The side chat's prompt: the source topic up to the asker's point, then this DM topic. */
  protected async asidePrompt(
    m: TopicMessage,
    claim: ClaimResult,
    ctx: TopicContext,
    followUps: { authorId: string; body: string }[],
  ): Promise<string> {
    const botId = this.botId as string;
    // The asker is the side chat's own person: a run that an answer or a live event starts
    // can carry "anonymous" (a private card) as its author.
    const source = unwrap(
      await roomStub(this.env, ctx.groupId).rpc(ctx.groupId, "bots.context", this.actor(), {
        topicId: ctx.topicId,
        uptoSeq: ctx.uptoSeq,
        askerId: claim.askerId ?? m.authorId,
      }),
    ) as SourceContext;
    const chat = claim.messages.filter((x) => x.body !== "" && x.id !== m.messageId);
    const names = await this.names([...source.messages.map((x) => x.authorId), ...chat.map((x) => x.authorId), m.authorId, botId]);
    return buildAsidePrompt({
      botName: names.get(botId) ?? "bot",
      source: { title: source.title, messages: source.messages.filter((x) => x.body !== "") },
      chat,
      names,
      request: { authorId: m.authorId, body: m.body },
      followUps,
    });
  }

  protected assignFrame(r: RunRow, runner: BotRunner, prompt: string, machine?: MachineView): RunAssignFrame {
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
    if (r.kind === RUN_KIND_ASIDE) frame.kind = RUN_KIND_ASIDE;
    if (runner.repo) frame.repo = { ...runner.repo, ...(runner.baseBranch ? { baseBranch: runner.baseBranch } : {}) };
    // Continue the topic's last session when this run goes to the machine that has it.
    const prev = machine && hasFeature(machine, "resume") ? this.lastSession(r, runner) : undefined;
    if (prev?.session_id && prev.machine_id === machine?.id) frame.resumeSessionId = prev.session_id;
    return frame;
  }

  protected async assign(r: RunRow, runner: BotRunner, machine: MachineView, prompt: string): Promise<void> {
    const frame = this.assignFrame(r, runner, prompt, machine);
    this.sql.exec("UPDATE runs SET machine_id = ?, runtime = ? WHERE id = ?", machine.id, runner.runtime, r.id);
    r.machine_id = machine.id;
    r.runtime = runner.runtime;
    await this.env.DB.prepare("UPDATE bot_runs SET machine_id = ? WHERE id = ?").bind(machine.id, r.id).run();
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
    await linkStub(this.env, machine.id).assign(machine.id, this.botId as string, frame);
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
      const machine = runner?.enabled ? this.pick(r, runner, await loadMachines(this.env), excluded) : null;
      const prompt = this.meta(`prompt:${runId}`);
      if (!runner || !machine || prompt === null) {
        const why = r.kind === RUN_KIND_ASIDE ? TEXT.noAsideMachine : TEXT.noMachine;
        return this.end(r, "failed", why, "blocked", why);
      }
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
}
