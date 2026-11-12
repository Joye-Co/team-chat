// BotDesk runs: starting a run from a mention (claim, ack, machine, prompt, assign) and the
// MachineLink answers to an assignment (accepted, rejected, lost).
import { workerModel } from "../../settings/models";
import type { WorkerModel } from "@chat/protocol";
import { type BotRunner, isMainTopicId, RUN_KIND_ASIDE, type RunAssignFrame, type RunContextManifest, type TopicContext } from "@chat/protocol";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { effectivePolicy } from "../../settings/approvals";
import { policyPrompt, runGates } from "../../settings/rules";
import { CONTEXT_CHARS, type DispatchCtx, LEASE_MS, LIMIT_SLACK_MS, MAX_RUN_MS, type MachineView, type NoMachine, TEXT, asideRuntime, cliName, clockText, buildAsidePrompt, buildPrompt, buildResumePrompt, cut, gatesRuntime, hasFeature, hasRuntime, personalBlock, pickMachine, placesFor, resumesRuntime, whyNoMachine } from "../dispatch";
import { type ContextRow, type Selection, requestAuthors, requestText, selectLines, withSummary } from "../../context/render";
import { teamZone } from "../../memory/consolidate";
import { type Brief, briefText, isNewTask } from "../../agent/frontdesk";
import { linkStub } from "../env";
import type { ClaimResult, SourceContext, TopicMessage } from "../room";
import { refsText } from "../refs";
import { EXECUTOR_MEMORY_BYTES, memoryIndex } from "../../memory/inject";
import { groupMembers, membersText } from "../../context/read";
import { loadMachines, loadRunner } from "../store";
import { ANONYMOUS_ID } from "../../cards2/privacy";
import { githubFor } from "../../github/assign";
import { supersedeWaits } from "../../github/machine";
import { DeskCore, QUEUED, type RunRow } from "./core";
import { takeoverOf } from "./hold";
import { type FailureNote, failureNote } from "./failure";
import { LIMIT_NOTE, LIMIT_REACHED, type LoggedFrame, type RecoveryFacts, outwardAction, recoveryNote, stepOf } from "../../agent/recovery";
import type { RunProof } from "../proof";
import { needsReply, reportOf } from "./report";
import { runLimitMs } from "../../settings/runs";
import { LIMIT_BACKSTOP_MS } from "../../agent/supervise";

/** The prompts of a run (Run context): the full one, and the one for a resumed session. */
interface Prompts {
  prompt: string;
  manifest: RunContextManifest;
  resume?: { prompt: string; manifest: RunContextManifest };
}

/**
 * What a run's prompt carried. Problem 6: the request's own files count with the files, and its
 * quote, 产出 and 引用 with the refs (the topic lines leave the request out).
 */
function manifestOf(sel: Selection, resumed: boolean, m: Pick<TopicMessage, "ext" | "refs">): RunContextManifest {
  const out: RunContextManifest = {
    resumed,
    chars: sel.chars,
    messages: sel.rows.length,
    omitted: sel.omitted,
    files: sel.stats.files + (m.ext?.attachments?.length ?? 0),
    cards: sel.stats.cards,
    refs: sel.stats.refs + (m.ext?.replyTo ? 1 : 0) + (m.ext?.artifactRefs?.length ?? 0) + (m.refs?.length ?? 0),
  };
  if (sel.rows.length > 0) {
    out.firstSeq = sel.rows[0].seq;
    out.lastSeq = sel.rows[sel.rows.length - 1].seq;
  }
  return out;
}

/**
 * The code folder a run gets on its machine (machine-protocol.md `run.assign.repo`). On a
 * personal machine (design-roles 3.3, 机 3.8) only the owner's folder for this member counts
 * (`machine_bots.repo_path`); a folder an admin set on the member is a path on other machines,
 * so there it clones the member's repo URL, or works in a scratch folder.
 */
export function repoOn(runner: Pick<BotRunner, "repo">, machine: Pick<MachineView, "ownerId" | "personalBots"> | undefined, botId: string): { url?: string; path?: string } | undefined {
  if (!machine?.ownerId) return runner.repo;
  const own = machine.personalBots?.get(botId);
  if (own) return { path: own };
  return runner.repo?.url ? { url: runner.repo.url } : undefined;
}

/** How a restarted run goes on (meta `restart:{runId}` of the run it restarts). */
export interface Restart {
  /** Continue the CLI session on the same machine (盯 1.3: interrupt and retry). */
  sameSession?: boolean;
  /** Machines the new run must not go to. */
  exclude?: string[];
  /** The only machine the new run may go to (a same-session restart stays on its machine). */
  machine?: string;
  /** Why the work starts again (developer words): the recovery note names it. */
  why?: string;
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
  protected abstract restartElsewhere(r: RunRow, why: string, how?: Restart, opts?: { count?: boolean; step?: number; limitRounds?: number }): Promise<boolean>;
  /** 盯 1.1 (stall.ts): the lease of a run ran out; true when the run keeps waiting for its machine. */
  protected abstract machineLost(r: RunRow): Promise<boolean>;

  // -------------------------------------------------------------------------
  // Starting a run
  // -------------------------------------------------------------------------

  protected async start(
    m: TopicMessage,
    followUpOf?: RunRow,
    followUps: { authorId: string; body: string }[] = [],
    retryOf?: string,
    brief?: Brief,
    opts: { newTask?: boolean } = {},
  ): Promise<void> {
    const botId = this.botId as string;
    const runner = await loadRunner(this.env.DB, botId);
    if (!runner?.enabled) return;
    // 3.2: whether a request in a chat area is in a DM (a card answer or a live event does not say).
    const dm = isMainTopicId(m.topicId) && !followUpOf && !retryOf && (m.dm === true || (await this.isDm(m.groupId)));
    const claim = unwrap(
      await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId }),
    ) as ClaimResult;
    if (this.startBlocked(m.topicId)) return; // Raced with another message.
    // 接 1.5: a request next to a running one in a chat area is its own piece of work.
    const alongside = isMainTopicId(m.topicId) && this.activeRun(m.topicId) !== undefined;

    const requestedFor = await this.requestedFor(m, followUpOf, retryOf);
    if (this.startBlocked(m.topicId)) return; // Raced with another message.
    const now = Date.now();
    const runId = ulid(now);
    // A side chat topic (单独问) gets a read-only run that answers only the asker.
    const kind = claim.contextOf ? RUN_KIND_ASIDE : null;
    this.sql.exec(
      `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at, follow_up_of, kind, retry_of, requested_for)
       VALUES (?, ?, ?, 'created', ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
      requestedFor,
    );
    // Run context: in the 群聊 only a follow-up, or a request that quotes this bot, continues
    // the bot's last session; any other request is a new piece of work. 接 3.2: in a DM's chat
    // area the next request continues it unless it is said to be new work (新任务, or the front
    // desk says so). 接 1.5: a request next to a running one never shares that run's session.
    const quotesBot = m.replyToAuthorId === botId || (m.refs ?? []).some((x) => x.fromId === botId);
    // 盯活: a restart says whether it continues the session and which machines it leaves out.
    const how = retryOf ? this.restartOf(retryOf) : null;
    const newWork = dm ? opts.newTask === true || isNewTask(m.body) : !quotesBot;
    // A restart continues the work it replaces (盯 1.2): never a fresh start in the 群聊 either.
    if (isMainTopicId(m.topicId) && !followUpOf && !how && (alongside || newWork)) this.sql.exec("UPDATE runs SET fresh = 1 WHERE id = ?", runId);
    if (how?.exclude && how.exclude.length > 0) this.sql.exec("UPDATE runs SET excluded = ? WHERE id = ?", JSON.stringify(how.exclude), runId);
    // design-roles 3.3 rule 2: handed over from a run on the person's own machine. A restart
    // keeps the machine its work came from.
    const fromMachine = m.fromMachine ?? (retryOf ? this.meta(`fromMachine:${retryOf}`) : null);
    if (fromMachine) this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `fromMachine:${runId}`, fromMachine);
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
        // A new run of this member in a topic answers the GitHub waits of its earlier ones there.
        if (!isMainTopicId(m.topicId)) await supersedeWaits(this.env, botId, m.topicId).catch((err) => this.log.warn("github waits cleanup failed", { runId, err }));
        const all = await loadMachines(this.env);
        const ctx = await this.placeCtx(r);
        // design-roles 3.3: no machine may ever take it for its person (the member works only on
        // someone else's own machine): said at once, without 👀, and the run does not wait.
        const blocked = personalBlock(all, runner, ctx);
        if (blocked) return await this.endPersonal(r, blocked);
        // Answers and live events start runs too: a card answer acknowledges on the answered card,
        // other synthetic keys get no reaction.
        if (m.ackMessageId) await this.ack(r, m.ackMessageId);
        else if (!m.messageId.includes(":")) await this.ack(r);
        await this.setStatus(r, "created");
        await this.topic(r, "in_progress");
        await this.answeredReports(r);

        const machines = this.machinesFor(r, all);
        const machine = this.pick(r, runner, machines, how?.exclude ?? [], ctx);
        // Every machine that could take it is busy, or (机 2.2) offline: the run waits (排队)
        // instead of failing, and starts by itself when one is free or online again.
        const none = machine ? null : this.whyNone(r, runner, machines, ctx);
        // 模 3.4: so does a run whose computers are at their subscription limit, until the reset.
        const waits = none !== null && (none.why === "busy" || ((none.why === "offline" || none.why === "limit") && !kind));
        if (!machine && !waits) {
          // 模 3.1: the computers' CLI is not signed in: the topic says which, and who signs in.
          if (!kind && none?.why === "signed_out") {
            const note = await this.signedOutNote(r, runner.runtime, none.machines);
            return this.end(r, "failed", note.text, "blocked", cut(note.text, 280), false, note.mentions);
          }
          const why = kind ? TEXT.noAsideMachine : this.noMachineText(none);
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
          // 盯 1.2: a restart starts with the recovery note, in a resumed session and in a new one.
          const note = retryOf ? await this.recoveryNoteOf(retryOf, how?.why ?? "restarted") : null;
          if (note) {
            p.prompt = `${p.prompt}\n\n${note}`;
            // 模 3.4: the session that stopped at the limit goes on with a short note.
            if (p.resume) p.resume.prompt = `${p.resume.prompt}\n\n${how?.why === LIMIT_REACHED ? LIMIT_NOTE : note}`;
          }
          prompt = p.prompt;
          this.storeContext(r.id, p);
        }
        // Sources of refs the run saw: later run.input texts give their context only once.
        if (m.refs && m.refs.length > 0) {
          this.seeRefs(runId, [...claim.messages.map((x) => x.id), ...m.refs.map((x) => x.messageId)]);
        }
        // 到了预算: the run waits for an owner instead of starting (budget/index.ts).
        if (await this.budgetGate(r, prompt)) return;
        if (!machine) {
          this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
          r.queued_at = Date.now();
          this.sql.exec("UPDATE runs SET queued_at = ? WHERE id = ?", r.queued_at, r.id);
          if (none?.why === "limit") this.keepLimit(r, none.until);
          const places = placesFor(machines, runner, ctx);
          this.keepPlaces(r, places.machines, places.checked && none?.why === "offline");
          await this.setStatus(r, "created");
          // Waiting for a machine to come online: the topic says which one (design-roles 3.3:
          // the person's checked machines, and how not to wait).
          if (none?.why === "offline") {
            const text = places.checked ? TEXT.waitPersonal(none.names) : TEXT.waitOffline(none.names);
            await this.note(r, text).catch((err) => this.log.warn("wait note failed", { runId, err }));
          }
          // Waiting for the limit to reset: the topic says until when.
          if (none?.why === "limit") await this.limitNote(r, runner.runtime, none);
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

  /**
   * The person a new run works for (protocol "Who a run works for"): what the starter says
   * (`m.requestedFor`), else a retry's original run's, else the author when a person; a follow-up
   * whose author is nobody (a private answer as input) keeps its run's.
   */
  protected async requestedFor(m: TopicMessage, followUpOf?: RunRow, retryOf?: string): Promise<string | null> {
    if (m.requestedFor !== undefined) return m.requestedFor;
    const retried = retryOf ? this.run(retryOf) : undefined;
    if (retried?.requested_for) return retried.requested_for;
    const author =
      m.authorId === ANONYMOUS_ID ? null : await this.env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(m.authorId).first<{ kind: string }>();
    if (author?.kind === "human") return m.authorId;
    return followUpOf?.requested_for ?? null;
  }

  /**
   * design-roles 3.3: what the ownership rules of personal machines read for a run: its person,
   * the machine its work was handed over from, and (a hook's run) who last placed the member.
   */
  protected async placeCtx(r: Pick<RunRow, "id" | "requested_for">): Promise<DispatchCtx> {
    const ctx: DispatchCtx = { botId: this.botId as string, requestedFor: r.requested_for ?? null };
    const from = this.meta(`fromMachine:${r.id}`);
    if (from) ctx.fromMachine = from;
    if (ctx.requestedFor === null) {
      const row = await this.env.DB.prepare("SELECT updated_by FROM bot_runners WHERE member_id = ?").bind(ctx.botId).first<{ updated_by: string | null }>();
      ctx.updatedBy = row?.updated_by ?? null;
    }
    return ctx;
  }

  /**
   * design-roles 3.3 `personalOnly`: no machine may ever take the run for its person; the topic
   * says whose machine the member works on, and the run ends (reason `machine.personal_requester`).
   */
  protected async endPersonal(r: RunRow, ownerId: string): Promise<void> {
    const botId = this.botId as string;
    const names = await this.names([botId, ownerId]);
    const text = TEXT.personalOnly(names.get(botId) ?? botId, names.get(ownerId) ?? ownerId);
    this.log.info("run not for this machine owner", { runId: r.id, reason: "machine.personal_requester" });
    // The front desk may have put 👀 on the request while it read it: nothing works on it now.
    if (!r.trigger_message_id.includes(":")) await this.unack(r.group_id, r.trigger_message_id).catch(() => undefined);
    await this.end(r, "failed", text, "blocked", text);
  }

  /**
   * Keeps the machines a queued run may go to (its place in the queue counts only runs that
   * wait for the same machines) and whether it waits for its person's own machines.
   */
  protected keepPlaces(r: Pick<RunRow, "id">, machines: MachineView[], waitsPersonal: boolean): void {
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `places:${r.id}`, JSON.stringify(machines.map((m) => m.id)));
    if (waitsPersonal) this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `waitPersonal:${r.id}`, "1");
    else this.sql.exec("DELETE FROM meta WHERE key = ?", `waitPersonal:${r.id}`);
  }

  /**
   * True when the run must wait for an owner before it starts (到了预算). The budget feature
   * (BotDesk) holds the run; here nothing is held.
   */
  protected async budgetGate(_r: RunRow, _prompt: string): Promise<boolean> {
    return false;
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

  /** The run and the runs it restarted (`retry_of`), newest first: one piece of work. */
  protected retryChain(runId: string): RunRow[] {
    const out: RunRow[] = [];
    let id: string | null = runId;
    while (id && out.length < 20) {
      const r = this.run(id);
      if (!r) break;
      out.push(r);
      id = r.retry_of;
    }
    return out;
  }

  /** What the earlier tries of a piece of work did (the run `runId` and the runs it restarted). */
  protected async recoveryFacts(runId: string, why: string): Promise<RecoveryFacts> {
    const chain = this.retryChain(runId);
    const steps: string[] = [];
    const actions: string[] = [];
    for (const run of [...chain].reverse()) {
      const rows = this.sql
        .exec<{ frame: string }>(
          `SELECT frame FROM run_log WHERE run_id = ? AND (frame LIKE '%"tool.call"%' OR frame LIKE '%"output"%') ORDER BY seq`,
          run.id,
        )
        .toArray();
      for (const row of rows) {
        let f: LoggedFrame;
        try {
          f = JSON.parse(row.frame) as LoggedFrame;
        } catch {
          continue;
        }
        const step = stepOf(f);
        if (step) steps.push(step);
        const action = outwardAction(f);
        if (action) actions.push(action);
      }
    }
    const last = chain[0];
    const first = chain[chain.length - 1];
    let prs: string[] = [];
    let artifacts: string[] = [];
    // 产出 of every try: a version keeps the run that made it, and an earlier try may have made it.
    if (last) {
      try {
        const proof = unwrap(
          await roomStub(this.env, last.group_id).rpc(last.group_id, "bots.runProof", this.actor(), { topicId: last.topic_id, runId: last.id, runIds: chain.map((x) => x.id), since: first.created_at }),
        ) as RunProof;
        prs = proof.prs.map((p) => `${p.url}${p.state ? ` (${p.state})` : ""}`);
        artifacts = proof.artifacts.map((a) => `"${a.title}" version ${a.version}`);
      } catch (err) {
        this.log.warn("recovery proof failed", { runId, err });
      }
    }
    // The newest plan any try reported.
    const plan = chain.map((x) => this.planOf(x)).find((x) => x !== null && x.length > 0) ?? null;
    return { why, tries: chain.length, plan, steps, actions, prs, artifacts };
  }

  /** The recovery note a restart of `runId` starts with (盯 1.2). */
  protected async recoveryNoteOf(runId: string, why: string): Promise<string> {
    return recoveryNote(await this.recoveryFacts(runId, why));
  }

  /** How a restart of `runId` goes on (meta `restart:{runId}`), or null when it was no restart. */
  protected restartOf(runId: string): Restart | null {
    const raw = this.meta(`restart:${runId}`);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as Restart;
    } catch {
      return null;
    }
  }

  /**
   * The machines a run may go to. A same-session restart (盯 1.3) stays on its own machine, where
   * the run it replaces no longer takes a slot (the daemon has not answered the cancel yet); when
   * that machine is busy or away, the restart waits for it (排队) instead of going elsewhere.
   */
  protected machinesFor(r: RunRow, machines: MachineView[]): MachineView[] {
    const how = r.retry_of ? this.restartOf(r.retry_of) : null;
    if (!how?.machine) return machines;
    const old = r.retry_of as string;
    return machines
      .filter((x) => x.id === how.machine)
      .map((x) =>
        x.runningRuns.includes(old)
          ? { ...x, runningRuns: x.runningRuns.filter((id) => id !== old), ...(x.health ? { health: { ...x.health, activeRuns: Math.max(0, x.health.activeRuns - 1) } } : {}) }
          : x,
      );
  }

  /** Why no machine takes the run now (机 2.10); an aside run needs a daemon that runs it read-only. */
  protected whyNone(r: RunRow, runner: BotRunner, machines: MachineView[], ctx?: DispatchCtx): NoMachine {
    const placed = placesFor(machines, runner, ctx).machines;
    const usable = r.kind === RUN_KIND_ASIDE ? placed.filter((x) => asideRuntime(x, runner.runtime)) : placed;
    return whyNoMachine(usable, runner, Date.now(), ctx);
  }

  /** Why a rejected run did not start: the machines left decide, else the daemon's reason. */
  protected rejectText(none: NoMachine | null, reason: string): string {
    if (reason === "busy" || reason === "draining" || none?.why === "busy") return TEXT.allBusy;
    if (none?.why === "runtime" || none?.why === "disk") return this.noMachineText(none);
    if (reason === "runtime_unavailable") return TEXT.noRuntime;
    if (reason === "disk_low") return TEXT.noDisk;
    return TEXT.failed("");
  }

  /**
   * 模 3.1, 3.5: no run starts because every computer with the runtime has its CLI signed out.
   * The run keeps the failure; the note names the computers and who signs them in.
   */
  protected async signedOutNote(r: RunRow, runtime: string, machines: MachineView[]): Promise<FailureNote> {
    r.failure = JSON.stringify({ kind: "signed_out" });
    this.sql.exec("UPDATE runs SET failure = ? WHERE id = ?", r.failure, r.id);
    const on = machines.map((m) => {
      const kind = m.clis?.find((c) => c.runtime === runtime)?.kind;
      return { id: m.id, name: m.name, ...(m.ownerId ? { ownerId: m.ownerId } : {}), ...(kind ? { kind } : {}) };
    });
    return failureNote(this.env, { kind: "signed_out", machines: on, runtime, requesterId: r.requested_by, groupId: r.group_id, started: false });
  }

  /** 模 3.4: keeps when the run's subscription limit resets (the run waits for it). */
  protected keepLimit(r: RunRow, until: number | null): void {
    if (r.limit_until === until) return;
    r.limit_until = until;
    this.sql.exec("UPDATE runs SET limit_until = ? WHERE id = ?", until, r.id);
  }

  /** 模 3.4: the topic says the run waits for the limit, until when (team clock), or when it tries again. Never throws. */
  protected async limitNote(r: RunRow, runtime: string, none: Extract<NoMachine, { why: "limit" }>): Promise<void> {
    const now = Date.now();
    const cli = cliName(none.machines.map((m) => m.clis?.find((c) => c.runtime === runtime)?.kind).find((k) => k !== undefined), runtime);
    const text = none.known
      ? TEXT.waitLimit(cli, clockText(none.until - LIMIT_SLACK_MS, now, teamZone(this.env)), r.retry_of !== null)
      : TEXT.waitLimitRetry(cli, Math.max(1, Math.round((none.until - now) / 60_000)));
    await this.note(r, text).catch((err) => this.log.warn("limit note failed", { runId: r.id, err }));
  }

  protected noMachineText(none: NoMachine | null): string {
    if (none?.why === "runtime") return TEXT.noRuntime;
    if (none?.why === "disk") return TEXT.noDisk;
    return TEXT.noMachine;
  }

  /**
   * design-roles 3.3: a person checked or unchecked this member on a machine, or a machine's owner
   * changed: the queued runs are placed again now (not at the next alarm).
   */
  async redispatch(botId: string): Promise<void> {
    this.bind(botId);
    await this.dispatchQueued();
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
        const all = await loadMachines(this.env);
        const ctx = await this.placeCtx(r);
        // design-roles 3.3: a check, a placement or an owner changed while it waited, and no
        // machine may take it for its person any more: it stops waiting and the topic says why.
        const blocked = personalBlock(all, runner, ctx);
        // An ended run leaves the queue: the runs behind it move up (returns true).
        if (blocked) {
          await this.endPersonal(r, blocked);
          return true;
        }
        const machines = this.machinesFor(r, all);
        const machine = prompt === null ? null : this.pick(r, runner, machines, JSON.parse(r.excluded) as string[], ctx);
        const none = machine ? null : this.whyNone(r, runner, machines, ctx);
        if (!machine) {
          // It waits for the person's own machines while those are offline (not only when it
          // first queued for that: a checked laptop that was busy may go offline meanwhile).
          const places = placesFor(machines, runner, ctx);
          const waitsPersonal = places.checked && none?.why === "offline";
          const before = this.meta(`waitPersonal:${id}`) !== null;
          this.keepPlaces(r, places.machines, waitsPersonal);
          if (waitsPersonal && !before && none?.why === "offline") {
            await this.note(r, TEXT.waitPersonal(none.names)).catch((err) => this.log.warn("wait note failed", { runId: id, err }));
            await this.setStatus(r, "created");
          }
        }
        if (none && prompt !== null && r.kind !== RUN_KIND_ASIDE) {
          // 模 3.1: the computers' CLI was signed out while the run waited: it does not wait
          // for a sign-in nobody knows is needed; the topic says which computer needs it.
          if (none.why === "signed_out") {
            const note = await this.signedOutNote(r, runner.runtime, none.machines);
            await this.end(r, "failed", note.text, "blocked", cut(note.text, 280), false, note.mentions);
            return true;
          }
          // 模 3.4: its computers are at their limit: it waits for the reset (the topic hears
          // once, when the wait starts), and later runs that can go elsewhere still do.
          if (none.why === "limit") {
            const first = r.limit_until === null;
            this.keepLimit(r, none.until);
            if (first) {
              await this.setStatus(r, "created");
              await this.limitNote(r, runner.runtime, none);
            }
            return false;
          }
        }
        if (!machine || prompt === null) return false;
        if (await this.budgetGate(r, prompt)) return false;
        await this.assign(r, runner, machine, prompt);
        this.sql.exec("UPDATE runs SET queued_at = NULL WHERE id = ?", id);
        this.sql.exec("DELETE FROM meta WHERE key IN (?, ?)", `places:${id}`, `waitPersonal:${id}`);
        r.queued_at = null;
        // 模 3.4: the limit reset: the run goes on, and the topic says so.
        const afterLimit = r.limit_until !== null;
        this.keepLimit(r, null);
        await this.setStatus(r, "created");
        if (afterLimit) await this.note(r, TEXT.limitBack(r.retry_of !== null)).catch((err) => this.log.warn("limit back note failed", { runId: id, err }));
        return true;
      });
      // design-roles 3.3: a run with no free machine does not hold up the later ones, which may
      // wait for other machines (A's closed laptop does not stop B's work on the team machine).
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

  /**
   * 接 4.2: the topic's earlier runs that ended waiting for a reply (`needs_reply`) are answered
   * by the run `r` that starts now: their `run.updated` says so (phase `done`). Never throws.
   */
  protected async answeredReports(r: RunRow): Promise<void> {
    const waiting = this.sql
      .exec<RunRow>("SELECT * FROM runs WHERE topic_id = ? AND id != ? AND ended_at IS NOT NULL AND report IS NOT NULL AND status = 'done' ORDER BY created_at DESC LIMIT 5", r.topic_id, r.id)
      .toArray() as RunRow[];
    for (const prev of waiting) {
      if (!needsReply(reportOf(prev))) continue;
      try {
        await this.setStatus(prev, prev.status);
      } catch (err) {
        this.log.warn("answered report update failed", { runId: prev.id, err });
      }
    }
  }

  /** The best machine for the run; an aside run needs a daemon that runs it read-only. The
   * machine of the topic's last session comes first, so the run can continue that session. */
  protected pick(r: RunRow, runner: BotRunner, machines: MachineView[], excluded: string[] = [], ctx?: DispatchCtx): MachineView | null {
    // design-roles 3.3: only machines that may take it for its person; the person's checked ones
    // first and alone, and the machine the work was handed over from before the rest.
    const placed = placesFor(machines, runner, ctx).machines;
    const usable = r.kind === RUN_KIND_ASIDE ? placed.filter((x) => asideRuntime(x, runner.runtime)) : placed;
    const now = Date.now();
    if (ctx?.fromMachine) {
      const from = pickMachine(usable.filter((x) => x.id === ctx.fromMachine), runner, now, excluded, ctx);
      if (from) return from;
    }
    const prev = this.lastSession(r, runner);
    if (prev) {
      const same = pickMachine(usable.filter((x) => x.id === prev.machine_id), runner, now, excluded, ctx);
      if (same) return same;
    }
    return pickMachine(usable, runner, now, excluded, ctx);
  }

  /**
   * The session the run continues: a follow-up or a retry continues its own run's session (with
   * several runs in a chat area the newest ended run may be another request's); any other run the
   * topic's last ended run of the same kind. Only with a CLI session on the same runtime.
   */
  protected lastSession(r: RunRow, runner: BotRunner): RunRow | undefined {
    if (r.fresh) return undefined;
    const of = r.follow_up_of ?? r.retry_of;
    const own = of ? this.run(of) : undefined;
    const prev =
      own && own.ended_at !== null
        ? own
        : (this.sql
            .exec<RunRow>(
              "SELECT * FROM runs WHERE topic_id = ? AND id != ? AND ended_at IS NOT NULL AND kind IS ? ORDER BY created_at DESC LIMIT 1",
              r.topic_id,
              r.id,
              r.kind,
            )
            .toArray()[0] as RunRow | undefined);
    return prev?.session_id && prev.machine_id && prev.runtime === runner.runtime && !prev.external ? prev : undefined;
  }

  /** Whether the group is a DM (接 3.2). */
  protected async isDm(groupId: string): Promise<boolean> {
    const row = await this.env.DB.prepare("SELECT kind FROM groups WHERE id = ?").bind(groupId).first<{ kind: string }>();
    return row?.kind === "dm";
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
    const names = await this.names([...new Set([...authors, m.authorId, botId, ...refIds, ...requestAuthors(m.ext)])]);
    const name = (id: string) => names.get(id) ?? id;
    const lines = {
      more: (x: ContextRow) => (contextTools ? `rest: chat-run topic message --id ${x.id}` : ""),
      gap: (n: number, before: number) =>
        contextTools ? `[… ${n} messages left out; read them with chat-run topic history --before ${before}]` : `[… ${n} messages left out …]`,
      zone: teamZone(this.env),
    };
    const sel = selectLines(first, newest, total, name, CONTEXT_CHARS, lines);
    const seen = new Set(sel.rows.map((x) => x.id));
    const refs = m.refs && m.refs.length > 0 ? refsText(name(m.authorId), m.refs, names, seen) : undefined;
    const request = { authorId: m.authorId, body: requestText(m.body, m.ext, name) };
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
      // Chosen by relevance to the request; the rest is found with chat-run memory search.
      memory: await memoryIndex(this.env, {
        botId,
        groupId: m.groupId,
        requesterId: m.authorId,
        requesterName: name(m.authorId),
        maxBytes: EXECUTOR_MEMORY_BYTES,
        query: [request.body, ...followUps.map((f) => f.body), ...(brief ? [briefText(brief)] : [])].join("\n"),
        runNote: contextTools,
      }),
      // Who is in the group and their 岗位: hand-offs and parts go to members by name.
      members: membersText(await groupMembers(this.env.DB, m.groupId), botId),
    });
    const out: Prompts = { prompt, manifest: manifestOf(sel, false, m) };
    if (since) {
      const delta = selectLines(undefined, since.newest, since.total, name, CONTEXT_CHARS, lines);
      out.resume = {
        prompt: buildResumePrompt({ names, context: delta.text, request, followUps, refs, contextTools, ...(brief ? { brief: briefText(brief) } : {}) }),
        manifest: manifestOf(delta, true, m),
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
    const names = await this.names([...source.messages.map((x) => x.authorId), ...chat.map((x) => x.authorId), m.authorId, botId, ...requestAuthors(m.ext)]);
    return buildAsidePrompt({
      botName: names.get(botId) ?? "bot",
      source: { title: source.title, messages: source.messages.filter((x) => x.body !== "") },
      chat,
      names,
      request: { authorId: m.authorId, body: requestText(m.body, m.ext, (id) => names.get(id) ?? id) },
      followUps,
    });
  }

  protected assignFrame(r: RunRow, runner: BotRunner, prompt: string, machine?: MachineView, team?: WorkerModel | null): RunAssignFrame {
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
    // The member's own choice wins, then the team's for this CLI (设置 › 模型), then the CLI's default.
    const model = runner.model ?? team?.model;
    const effort = runner.effort ?? team?.effort;
    if (model) frame.model = model;
    // 思考强度 only to a daemon that knows the field (an old one would ignore it silently).
    if (effort && machine && hasFeature(machine, "effort")) frame.effort = effort;
    if (r.kind === RUN_KIND_ASIDE) frame.kind = RUN_KIND_ASIDE;
    const repo = repoOn(runner, machine, this.botId as string);
    if (repo) frame.repo = { ...repo, ...(runner.baseBranch ? { baseBranch: runner.baseBranch } : {}) };
    // Continue the topic's last session when this run goes to the machine that has it.
    // Only a CLI that can continue it (ACP: an agent with session/load); else the full prompt.
    const prev = machine && resumesRuntime(machine, runner.runtime) ? this.lastSession(r, runner) : undefined;
    if (prev?.session_id && prev.machine_id === machine?.id) frame.resumeSessionId = prev.session_id;
    return frame;
  }

  protected async assign(r: RunRow, runner: BotRunner, machine: MachineView, prompt: string): Promise<void> {
    // 分出去 (design-relay R2): parts that came back while the run waited for a machine; kept in
    // its prompts, so a later assignment of the run carries them too.
    const parts = this.takeParts(r.id);
    if (parts !== null) {
      prompt = `${prompt}\n\n${parts}`;
      const resume = this.meta(`prompt_resume:${r.id}`);
      if (resume !== null) this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt_resume:${r.id}`, `${resume}\n\n${parts}`);
    }
    const frame = this.assignFrame(r, runner, prompt, machine, await workerModel(this.env.DB, runner.runtime));
    // 盯 1.6: the desk keeps the team's limit (work time, without waits) and saves and continues
    // the run there; the daemon's own deadline lies past it and is only a backstop.
    if (r.kind !== RUN_KIND_ASIDE) {
      if (r.limit_ms === null || r.limit_ms === undefined) {
        r.limit_ms = await runLimitMs(this.env.DB);
        this.sql.exec("UPDATE runs SET limit_ms = ? WHERE id = ?", r.limit_ms, r.id);
      }
      frame.maxDurationMs = r.limit_ms + LIMIT_BACKSTOP_MS;
    }
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
    // Whose GitHub the run uses on this machine (design-github-identity 3.2); never the token.
    const github = r.kind === RUN_KIND_ASIDE ? null : await githubFor(this.env, { requestedFor: r.requested_for, requestedBy: r.requested_by, botId: this.botId as string }, machine);
    if (github) frame.github = github.github;
    // 模 4.1: the sign-in the run starts on, from the machine's latest heartbeat; the budget counts by it.
    const methods = await linkStub(this.env, machine.id)
      .loginMethods()
      .catch((): Record<string, string> => ({}));
    const loginMethod = methods[runner.runtime] ?? null;
    // design-takeover 3.6: how a person can take over the run on this machine.
    r.takeover = takeoverOf(machine);
    this.sql.exec(
      "UPDATE runs SET machine_id = ?, runtime = ?, login_method = ?, takeover = ? WHERE id = ?",
      machine.id,
      runner.runtime,
      loginMethod,
      r.takeover,
      r.id,
    );
    r.machine_id = machine.id;
    r.runtime = runner.runtime;
    await this.env.DB.prepare("UPDATE bot_runs SET machine_id = ?, github_member_id = ? WHERE id = ?").bind(machine.id, github?.memberId ?? null, r.id).run();
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
      const machines = await loadMachines(this.env);
      const ctx = await this.placeCtx(r);
      const machine = runner?.enabled ? this.pick(r, runner, machines, excluded, ctx) : null;
      const prompt = this.meta(`prompt:${runId}`);
      if (!runner || !machine || prompt === null) {
        // 机 2.10: the true reason, from the machines left or else from the daemon's answer.
        const none = runner ? this.whyNone(r, runner, machines.filter((x) => !excluded.includes(x.id)), ctx) : null;
        if (runner && r.kind !== RUN_KIND_ASIDE && none?.why === "signed_out") {
          const note = await this.signedOutNote(r, runner.runtime, none.machines);
          return this.end(r, "failed", note.text, "blocked", cut(note.text, 280), false, note.mentions);
        }
        const why = r.kind === RUN_KIND_ASIDE ? TEXT.noAsideMachine : this.rejectText(none, reason);
        return this.end(r, "failed", why, "blocked", why);
      }
      await this.assign(r, runner, machine, prompt);
    });
  }

  /**
   * The run's lease ran out (machine-protocol.md, "Leases"). True: the run waits for its machine
   * and MachineLink keeps it, so a daemon that comes back still holding it goes on (盯 1.1).
   */
  async runLost(runId: string, machineId: string): Promise<boolean> {
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId) return false;
      // design-takeover 3.7: a run a person holds is not moved or ended; it waits for its machine.
      if (r.paused_by) {
        this.log.info("lease ran out while held", { runId, machineId });
        return true;
      }
      return this.machineLost(r);
    });
  }
}
