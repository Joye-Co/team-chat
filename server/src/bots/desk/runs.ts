// BotDesk runs: starting a run from a mention (claim, ack, machine, prompt, assign) and the
// MachineLink answers to an assignment (accepted, rejected, lost).
import { type BotRunner, isMainTopicId, RUN_KIND_ASIDE, type RunAssignFrame, type RunContextManifest, type TopicContext } from "@chat/protocol";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { effectivePolicy } from "../../settings/approvals";
import { policyPrompt, runGates } from "../../settings/rules";
import { CONTEXT_CHARS, LEASE_MS, MAX_RUN_MS, type MachineView, TEXT, buildAsidePrompt, buildPrompt, buildResumePrompt, candidates, cut, gatesRuntime, hasFeature, hasRuntime, pickMachine } from "../dispatch";
import { type ContextRow, type Selection, selectLines, withSummary } from "../../context/render";
import { type Brief, briefText } from "../../agent/frontdesk";
import { linkStub } from "../env";
import type { ClaimResult, SourceContext, TopicMessage } from "../room";
import { refsText } from "../refs";
import { EXECUTOR_MEMORY_BYTES, memoryIndex } from "../../memory/inject";
import { loadMachines, loadRunner } from "../store";
import { DeskCore, QUEUED, type RunRow } from "./core";

/** The prompts of a run (Run context): the full one, and the one for a resumed session. */
interface Prompts {
  prompt: string;
  manifest: RunContextManifest;
  resume?: { prompt: string; manifest: RunContextManifest };
}

function manifestOf(sel: Selection, resumed: boolean): RunContextManifest {
  const out: RunContextManifest = {
    resumed,
    chars: sel.chars,
    messages: sel.rows.length,
    omitted: sel.omitted,
    files: sel.stats.files,
    cards: sel.stats.cards,
    refs: sel.stats.refs,
  };
  if (sel.rows.length > 0) {
    out.firstSeq = sel.rows[0].seq;
    out.lastSeq = sel.rows[sel.rows.length - 1].seq;
  }
  return out;
}

/** What started a run (meta `request:{runId}`): the message and the follow-ups it carried. */
export interface RunRequest {
  m: TopicMessage;
  followUps: { authorId: string; body: string }[];
  /** The front desk's work order (交代单), when it dispatched the request. */
  brief?: Brief;
  /** Gated kinds a person approved for the whole piece of work (这件事里都同意). */
  approvedGates?: string[];
}

export abstract class DeskRuns extends DeskCore {
  /** 盯活 (stall.ts): ends the run quietly and starts the same request again; false when out of tries. */
  protected abstract restartElsewhere(r: RunRow, why: string): Promise<boolean>;

  // -------------------------------------------------------------------------
  // Starting a run
  // -------------------------------------------------------------------------

  protected async start(
    m: TopicMessage,
    followUpOf?: RunRow,
    followUps: { authorId: string; body: string }[] = [],
    retryOf?: string,
    brief?: Brief,
  ): Promise<void> {
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
    // Run context: in the 群聊 only a follow-up, or a request that quotes this bot, continues
    // the bot's last session; any other request is a new piece of work.
    const quotesBot = m.replyToAuthorId === botId || (m.refs ?? []).some((x) => x.fromId === botId);
    if (isMainTopicId(m.topicId) && !followUpOf && !quotesBot) this.sql.exec("UPDATE runs SET fresh = 1 WHERE id = ?", runId);
    if (claim.newest?.[0]) this.sql.exec("UPDATE runs SET context_seq = ? WHERE id = ?", claim.newest[0].seq, runId);
    // The request as it came, so the run can be retried with the same one. A follow-up or a
    // retry is the same piece of work: it keeps the kinds a person approved for all of it.
    const approvedGates = this.approvedGates(followUpOf?.id ?? retryOf);
    this.sql.exec(
      "INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)",
      `request:${runId}`,
      JSON.stringify({ m, followUps, ...(brief ? { brief } : {}), ...(approvedGates.length > 0 ? { approvedGates } : {}) } satisfies RunRequest),
    );
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

        // A queued run gets its machine later: use a feature when any machine has it.
        const feature = (f: string) => (machine ? hasFeature(machine, f) : machines.some((x) => hasFeature(x, f)));
        // The prompt tells the CLI the machine asks for approvals only when it surely will: the
        // run's machine stops gated commands, or (queued) every machine with the runtime does.
        const able = machines.filter((x) => hasRuntime(x, runner.runtime));
        const gated = machine ? gatesRuntime(machine, runner.runtime) : able.length > 0 && able.every((x) => gatesRuntime(x, runner.runtime));
        let prompt: string;
        if (claim.contextOf) {
          prompt = await this.asidePrompt(m, claim, claim.contextOf, followUps);
        } else {
          const p = await this.topicPrompts(r, runner, m, claim, followUps, feature("acceptance"), feature("context"), brief, gated);
          prompt = p.prompt;
          this.storeContext(r.id, p);
        }
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

  /** The request a run started with (meta `request:{runId}`), or null for runs before it. */
  protected requestOf(r: RunRow): RunRequest | null {
    const raw = this.meta(`request:${r.id}`);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as RunRequest;
    } catch {
      return null;
    }
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
    if (r.fresh) return undefined;
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

  /** The run's prompts (Run context): the topic within the budget, and what is new for a resumed session. */
  protected async topicPrompts(
    r: RunRow,
    runner: BotRunner,
    m: TopicMessage,
    claim: ClaimResult,
    followUps: { authorId: string; body: string }[],
    acceptance = false,
    contextTools = false,
    brief?: Brief,
    gated = false,
  ): Promise<Prompts> {
    const botId = this.botId as string;
    const notRequest = (x: ContextRow) => x.id !== m.messageId;
    const first = claim.first && notRequest(claim.first) ? claim.first : undefined;
    const newest = (claim.newest ?? []).filter(notRequest);
    const total = Math.max(0, (claim.total ?? 0) - (claim.newest ?? []).length + newest.length);
    const prev = this.lastSession(r, runner);
    let since: { newest: ContextRow[]; total: number } | undefined;
    if (prev && prev.context_seq !== null) {
      since = unwrap(
        await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.since", this.actor(), {
          topicId: m.topicId,
          afterSeq: prev.context_seq,
          exceptAuthor: botId,
        }),
      ) as { newest: ContextRow[]; total: number };
      since = { newest: since.newest.filter(notRequest), total: since.total - (since.newest.length - since.newest.filter(notRequest).length) };
    }
    const refIds = (m.refs ?? []).map((x) => x.fromId).filter((x): x is string => Boolean(x));
    const authors = [first, ...newest, ...(since?.newest ?? [])].flatMap((x) => (x ? [x.authorId, x.ext.replyTo?.authorId ?? x.authorId, ...(x.ext.cardState?.answers ?? []).map((a) => a.actorId)] : []));
    const names = await this.names([...new Set([...authors, m.authorId, botId, ...refIds])]);
    const name = (id: string) => names.get(id) ?? id;
    const lines = {
      more: (x: ContextRow) => (contextTools ? `rest: chat-run topic message --id ${x.id}` : ""),
      gap: (n: number, before: number) =>
        contextTools ? `[… ${n} messages left out; read them with chat-run topic history --before ${before}]` : `[… ${n} messages left out …]`,
    };
    const sel = selectLines(first, newest, total, name, CONTEXT_CHARS, lines);
    const seen = new Set(sel.rows.map((x) => x.id));
    const refs = m.refs && m.refs.length > 0 ? refsText(name(m.authorId), m.refs, names, seen) : undefined;
    const request = { authorId: m.authorId, body: m.body };
    const prompt = buildPrompt({
      botName: name(botId),
      title: claim.title,
      messages: [],
      context: withSummary(sel, claim.summary),
      contextTools,
      names,
      request,
      followUps,
      refs,
      // settings: who must approve what (merge to main, publish, spend over the limit).
      rules: policyPrompt(await effectivePolicy(this.env.DB), gated),
      acceptance,
      ...(brief ? { brief: briefText(brief) } : {}),
      // 沉淀: what the team, the group, this member and the requester keep (reference material).
      memory: await memoryIndex(this.env, { botId, groupId: m.groupId, requesterId: m.authorId, requesterName: name(m.authorId), maxBytes: EXECUTOR_MEMORY_BYTES }),
    });
    const out: Prompts = { prompt, manifest: manifestOf(sel, false) };
    if (since) {
      const delta = selectLines(undefined, since.newest, since.total, name, CONTEXT_CHARS, lines);
      out.resume = {
        prompt: buildResumePrompt({ names, context: delta.text, request, followUps, refs, contextTools, ...(brief ? { brief: briefText(brief) } : {}) }),
        manifest: manifestOf(delta, true),
      };
    }
    return out;
  }

  /** Keeps a run's prompts for its assignment and `GET /v1/runs/:id/context`. */
  protected storeContext(runId: string, p: Prompts): void {
    const put = (k: string, v: string) => this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", k, v);
    put(`context:${runId}`, JSON.stringify({ full: p.manifest, resume: p.resume?.manifest }));
    if (p.resume) put(`prompt_resume:${runId}`, p.resume.prompt);
  }

  /** What the run's prompt carried (docs/protocol.md, "Run context"), or null for older runs. */
  async runContext(botId: string, runId: string): Promise<{ prompt: string; manifest: RunContextManifest } | null> {
    this.bind(botId);
    const ctx = this.meta(`context:${runId}`);
    if (ctx === null) return null;
    const m = JSON.parse(ctx) as { full: RunContextManifest; resume?: RunContextManifest };
    const resumed = this.meta(`sent:${runId}`) === "resume" && m.resume !== undefined;
    const prompt = resumed ? this.meta(`prompt_resume:${runId}`) : this.meta(`prompt:${runId}`);
    if (prompt === null) return null;
    return { prompt, manifest: resumed ? (m.resume as RunContextManifest) : m.full };
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
    // 思考强度 only to a daemon that knows the field (an old one would ignore it silently).
    if (runner.effort && machine && hasFeature(machine, "effort")) frame.effort = runner.effort;
    if (r.kind === RUN_KIND_ASIDE) frame.kind = RUN_KIND_ASIDE;
    if (runner.repo) frame.repo = { ...runner.repo, ...(runner.baseBranch ? { baseBranch: runner.baseBranch } : {}) };
    // Continue the topic's last session when this run goes to the machine that has it.
    const prev = machine && hasFeature(machine, "resume") ? this.lastSession(r, runner) : undefined;
    if (prev?.session_id && prev.machine_id === machine?.id) frame.resumeSessionId = prev.session_id;
    return frame;
  }

  protected async assign(r: RunRow, runner: BotRunner, machine: MachineView, prompt: string): Promise<void> {
    const frame = this.assignFrame(r, runner, prompt, machine);
    // Gated actions: the team's checked kinds, to a machine whose CLI stops them; kinds a person
    // approved for this piece of work are left out.
    if (r.kind !== RUN_KIND_ASIDE && gatesRuntime(machine, runner.runtime)) {
      const gates = runGates(await effectivePolicy(this.env.DB), this.approvedGates(r.id));
      if (gates.length > 0) frame.gates = gates;
    }
    // A resumed session gets only what it has not seen (Run context).
    const resumePrompt = frame.resumeSessionId ? this.meta(`prompt_resume:${r.id}`) : null;
    if (resumePrompt !== null) frame.prompt = resumePrompt;
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `sent:${r.id}`, resumePrompt !== null ? "resume" : "full");
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
      // 盯活: the work goes on on another computer while the desk has tries left.
      if (await this.restartElsewhere(r, "machine lost")) return;
      await this.end(r, "blocked", TEXT.lost, "blocked", TEXT.lost);
    });
  }
}
