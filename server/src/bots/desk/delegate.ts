// BotDesk delegation between members (docs/design/agent-harness.zh.md 3.5, design-relay R2), from
// a run's chat-run:
// - 转交 (`member.handoff`): the whole piece of work goes to another AI member in the same topic,
//   which becomes the topic's routing owner and starts a run with the work order; this run stops.
// - 分出去 (`member.assign`): a part goes to another member in a new topic; its result comes back
//   to this member (as input to its run, or one follow-up run once every part is back).
// The room counts every hand-over of a piece of work in one place (chains.ts): a step is reserved
// first and confirmed once the other member took it, so a refusal changes neither the counts nor
// the routing owner. Bots' own messages never start runs: the hand-over goes desk to desk, and a
// card in the topic says who took what, with 全部停止 for any person in the group.
import { type CardV2, isMainTopicId } from "@chat/protocol";
import { ulid } from "../../core/ids";
import { ApiError } from "../../core/errors";
import { isGroupMember } from "../../core/db";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import type { Brief } from "../../agent/frontdesk";
import { requestAuthors, requestText } from "../../context/render";
import type { ChainStepOut, PartRow } from "../chains";
import { cut, personalBlock } from "../dispatch";
import { deskStub, linkStub } from "../env";
import { loadMachines, loadRunner } from "../store";
import type { RunProof } from "../proof";
import { memberInfo, type TopicMessage } from "../room";
import { ACTIVE, type ChainMeta, type RunRow, codeOf } from "./core";
import { CARD_TEXT, CHAIN_STOP, closedCard, handoverCard, lastWords, workOrderText } from "./delegate_card";
import { DeskFront } from "./front";

/** A run that handed its work over and has not stopped by itself is stopped after this long. */
export const HANDED_OVER_STOP_MS = 2 * 60_000;
/** design-roles 3.3: the other member may not work for the chain's person (docs/protocol.md). */
export const NOT_FOR_REQUESTER = "delegation.not_for_requester";
/** A run report the room did not take is sent again after this long. */
const PENDING_STATE_RETRY_MS = 30_000;

export interface DelegateIn {
  groupId: string;
  topicId: string;
  requesterId: string;
  /** The person the work is for (the giving run's `requested_for`); absent from an older desk. */
  requestedFor?: string | null;
  /** The giving run's machine (design-roles 3.3 rule 2); absent from an older desk. */
  fromMachine?: string;
  brief: Brief;
  fromBotId: string;
  kind: "handoff" | "assign";
  /** The new step's place in its chain. */
  chain: ChainMeta;
  /** The request the receiving run gets (delegate_card.ts `workOrderText`). */
  workOrder: string;
}

export abstract class DeskDelegate extends DeskFront {
  /** A run hands the work over (or a part of it) to another AI member. Answers the chat-run command. */
  protected async delegate(r: RunRow, op: "member.handoff" | "member.assign", body: unknown): Promise<unknown> {
    const b = (body ?? {}) as { member?: unknown; goal?: unknown; doneWhen?: unknown; notes?: unknown };
    if (typeof b.member !== "string" || b.member.trim() === "") throw new ApiError("invalid", "member is the other member's name or id");
    if (typeof b.goal !== "string" || b.goal.trim() === "" || [...b.goal].length > 1000) throw new ApiError("invalid", "goal is 1..1000 characters");
    const doneWhen = Array.isArray(b.doneWhen) ? b.doneWhen.filter((x): x is string => typeof x === "string" && x.trim() !== "").slice(0, 10).map((x) => x.slice(0, 300)) : [];
    const brief: Brief = { goal: b.goal.trim(), doneWhen, ...(typeof b.notes === "string" && b.notes.trim() ? { notes: b.notes.slice(0, 2000) } : {}) };
    const botId = this.botId as string;
    const kind = op === "member.handoff" ? "handoff" : "assign";
    const target = await this.findMember(r.group_id, b.member.trim());
    if (target === botId) throw new ApiError("invalid", "that is you");
    const mine = this.chainOf(r);
    const names = await this.names([target, botId, r.requested_by]);
    const name = (id: string) => names.get(id) ?? id;
    const rootRequest = mine?.rootRequest ?? (await this.rootRequestText(r));
    const rootTopicId = mine?.rootTopicId ?? r.topic_id;

    // 1. Reserve the step: the room counts and refuses (stopped, too many, too deep, a cycle).
    const reserved = unwrap(
      await this.room(r).rpc(r.group_id, "bots.chainStep", this.actor(), {
        fromRun: r.id,
        ...(mine ? { chain: { chainId: mine.chainId, seq: mine.seq } } : {}),
        requesterId: r.requested_by,
        kind,
        to: target,
        topicId: r.topic_id,
      }),
    ) as ChainStepOut;
    if (reserved.root) this.putMeta(`chain:${r.id}`, JSON.stringify({ ...reserved.root, rootRequest, rootTopicId } satisfies ChainMeta));
    const step: ChainMeta = { chainId: reserved.chainId, seq: reserved.seq, depth: reserved.depth, path: reserved.path, rootRequest, rootTopicId };
    this.log.info("delegation reserved", { runId: r.id, chainId: step.chainId, seq: step.seq, depth: step.depth });

    // 2. The other member takes it (分出去: in a new topic it is asked in).
    let topicId = r.topic_id;
    let runId: string | null;
    try {
      if (kind === "assign") {
        const sent = await this.room(r).send(r.group_id, this.actor(), {
          t: "send",
          rid: "bot",
          groupId: r.group_id,
          clientMsgId: `run:${r.id}:assign:${step.chainId}:${step.seq}`.slice(0, 64),
          body: `@${name(target)} ${brief.goal}`,
          mentions: [target],
        } as never);
        if (!sent.ok) throw new ApiError(sent.error.code, sent.error.message, sent.error.reason);
        topicId = sent.value.message.topicId as string;
      }
      const pathNames = (await this.names(step.path)) as Map<string, string>;
      const workOrder = workOrderText({
        kind,
        fromName: name(botId),
        requesterName: name(r.requested_by),
        goal: brief.goal,
        doneWhen,
        ...(brief.notes ? { notes: brief.notes } : {}),
        seq: step.seq,
        depth: step.depth,
        path: step.path.map((id) => pathNames.get(id) ?? id),
        rootRequest,
        rootTopicId,
        fromTopicId: r.topic_id,
        plan: this.planOf(r),
        artifacts: await this.runArtifacts(r),
        lastWords: lastWords(r.progress_text),
        code: this.codeOfWork(r),
      });
      runId = await this.handOver(target, { groupId: r.group_id, topicId, requesterId: r.requested_by, requestedFor: r.requested_for ?? undefined, ...(r.machine_id ? { fromMachine: r.machine_id } : {}), brief, fromBotId: botId, kind, chain: step, workOrder });
      if (!runId) throw new ApiError("conflict", "that member did not start the work", "delegation.refused");
    } catch (err) {
      // Refused: the room drops the step, the count and the routing owner stay as they were.
      try {
        unwrap(await this.room(r).rpc(r.group_id, "bots.chainStepResult", this.actor(), { chainId: step.chainId, seq: step.seq, runId: null }));
      } catch (e) {
        this.log.warn("chain step release failed", { runId: r.id, chainId: step.chainId, seq: step.seq, err: e });
      }
      throw err;
    }

    // 3. Confirm: the step works; a hand-over moves the routing owner.
    unwrap(await this.room(r).rpc(r.group_id, "bots.chainStepResult", this.actor(), { chainId: step.chainId, seq: step.seq, runId, topicId }));
    this.log.info("delegation taken", { runId: r.id, chainId: step.chainId, seq: step.seq, to: runId });
    if (kind === "handoff") {
      // This run stops: no result card, and the desk stops it if it goes on (alarm).
      this.putMeta(`handedOver:${r.id}`, JSON.stringify({ to: target, name: name(target), at: Date.now() }));
      await this.scheduleStall();
    }
    try {
      const card = handoverCard({
        kind,
        chainId: step.chainId,
        fromName: name(botId),
        toName: name(target),
        goal: brief.goal,
        doneWhen,
        ...(kind === "assign" ? { part: { groupId: r.group_id, topicId } } : {}),
      });
      const messageId = await this.post(r, `run:${r.id}:chain:${step.chainId}:${step.seq}`.slice(0, 64), card.fallback, { cardV2: card });
      this.putMeta(`chainCard:${messageId}`, JSON.stringify({ card, groupId: r.group_id, chainId: step.chainId, seq: step.seq }));
      this.putMeta(`partCard:${step.chainId}:${step.seq}`, messageId);
    } catch (err) {
      this.log.warn("hand-over card failed", { runId: r.id, err });
    }
    return { member: target, topicId, runId };
  }

  protected putMeta(key: string, value: string): void {
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", key, value);
  }

  /** The person's original request of a run's work (the first run of its line), with files and quotes. */
  protected async rootRequestText(r: RunRow): Promise<string> {
    let first: RunRow = r;
    for (let i = 0; i < 20; i++) {
      const prev = first.follow_up_of ?? first.retry_of;
      const p = prev ? this.run(prev) : undefined;
      if (!p) break;
      first = p;
    }
    const req = this.requestOf(first) ?? this.requestOf(r);
    if (!req) return "";
    const names = await this.names([req.m.authorId, ...requestAuthors(req.m.ext)]);
    return requestText(req.m.body, req.m.ext, (id) => names.get(id) ?? id);
  }

  /** 产出 versions the run made, for the work order. */
  protected async runArtifacts(r: RunRow): Promise<{ artifactId: string; version: number; title: string }[]> {
    try {
      const proof = unwrap(await this.room(r).rpc(r.group_id, "bots.runProof", this.actor(), { topicId: r.topic_id, runId: r.id, since: r.created_at })) as RunProof;
      return proof.artifacts;
    } catch {
      return [];
    }
  }

  /** Where the code of the run's work is: the run's own, else the newest a run before it reported. */
  protected codeOfWork(r: RunRow) {
    let cur: RunRow | undefined = r;
    for (let i = 0; cur && i < 20; i++) {
      const code = codeOf(cur.code);
      if (code) return code;
      const prev: string | null = cur.follow_up_of ?? cur.retry_of;
      cur = prev ? this.run(prev) : undefined;
    }
    return null;
  }

  /** The other member's desk takes the work; its refusal comes back as an API error of this call. */
  protected async handOver(target: string, d: DelegateIn): Promise<string | null> {
    try {
      return await deskStub(this.env, target).takeDelegation(target, d);
    } catch (err) {
      const message = String((err as Error)?.message ?? err);
      // design-roles 3.3: the other member may not work for this person (a reason of its own).
      const reason = message.includes(NOT_FOR_REQUESTER) ? NOT_FOR_REQUESTER : "delegation.refused";
      throw new ApiError("conflict", message.slice(0, 300), reason);
    }
  }

  /** The member a run names: an AI member of the group with a runner, by id or by name. */
  protected async findMember(groupId: string, nameOrId: string): Promise<string> {
    const rows = await this.env.DB.prepare(
      `SELECT m.id, m.name FROM members m JOIN group_members g ON g.member_id = m.id AND g.group_id = ?
       JOIN bot_runners r ON r.member_id = m.id AND r.enabled = 1
       WHERE m.kind = 'bot' AND m.deactivated = 0`,
    )
      .bind(groupId)
      .all<{ id: string; name: string }>();
    const hit = rows.results.find((x) => x.id === nameOrId) ?? rows.results.find((x) => x.name === nameOrId.replace(/^@/, ""));
    if (!hit) {
      const known = rows.results.map((x) => x.name).join(", ") || "none";
      throw new ApiError("not_found", `no AI member "${nameOrId}" in this group (members that work on computers: ${known}; see chat-run member list)`, "delegation.unknown_member");
    }
    return hit.id;
  }

  protected async topicTitle(r: RunRow): Promise<string> {
    try {
      const claim = unwrap(await roomStub(this.env, r.group_id).rpc(r.group_id, "bots.claim", this.actor(), { topicId: r.topic_id, peek: true })) as { title: string };
      return claim.title;
    } catch {
      return "";
    }
  }

  /**
   * The other side: start a run with the work order handed over. Returns the run id or null.
   * Busy in that topic, deactivated, out of the group or without a runner: refused (the giver
   * hears `delegation.refused` and may choose someone else). Busy elsewhere: the run waits (排队);
   * the computer offline: it waits for the computer.
   */
  async takeDelegation(botId: string, d: DelegateIn): Promise<string | null> {
    this.bind(botId);
    const info = (await memberInfo(this.env.DB, [botId])).get(botId);
    if (!info?.runner || info.deactivated || !(await isGroupMember(this.env.DB, d.groupId, botId))) {
      throw new ApiError("conflict", "that member takes no work now", "delegation.unavailable");
    }
    if (this.startBlocked(d.topicId)) throw new ApiError("conflict", "that member is already working in this topic; wait until it is done or do it yourself", "delegation.busy");
    // design-roles 3.3: refused at once when no machine may ever take it for the person (the
    // member works only on someone else's own machine); the giver may choose another member.
    const runner = await loadRunner(this.env.DB, botId);
    if (runner) {
      const ctx = await this.placeCtx({ id: "", requested_for: d.requestedFor ?? null });
      if (d.fromMachine) ctx.fromMachine = d.fromMachine;
      const owner = personalBlock(await loadMachines(this.env), runner, ctx);
      if (owner) {
        const names = await this.names([botId, owner]);
        throw new ApiError(
          "conflict",
          `${names.get(botId) ?? botId} works only for ${names.get(owner) ?? owner} on their own computer (${NOT_FOR_REQUESTER}); choose another member`,
          NOT_FOR_REQUESTER,
        );
      }
    }
    const key = `delegated:${ulid()}`;
    // The run's step is known before it starts, so its first report already names it.
    this.putMeta(`chainFor:${key}`, JSON.stringify(d.chain));
    const m: TopicMessage = { groupId: d.groupId, topicId: d.topicId, messageId: key, authorId: d.requesterId, body: d.workOrder, mentioned: true };
    if (d.requestedFor !== undefined) m.requestedFor = d.requestedFor;
    if (d.fromMachine) m.fromMachine = d.fromMachine;
    await this.start(m);
    const run = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (!run) {
      this.sql.exec("DELETE FROM meta WHERE key = ?", `chainFor:${key}`);
      return null;
    }
    this.putMeta(`chain:${run.id}`, JSON.stringify(d.chain));
    this.log.info("delegation taken", { runId: run.id, from: d.fromBotId, chainId: d.chain.chainId, seq: d.chain.seq, depth: d.chain.depth });
    return run.id;
  }

  /**
   * A part this member gave out came back (the room's `part` row; idempotent). To the run of the
   * step on its machine as input; to a run that waits for a machine with its prompt; with no run,
   * kept until the last part is back, then one follow-up run gets them all.
   */
  async partDone(botId: string, p: PartRow & { groupId: string }): Promise<void> {
    this.bind(botId);
    // A retried part comes back again with its new run.
    const seen = `partSeen:${p.chainId}:${p.seq}${p.runId ? `:${p.runId}` : ""}${p.cardOnly ? ":card" : ""}`;
    if (this.meta(seen) !== null) return;
    const names = await this.names([botId, p.fromBot, ...(p.stoppedBy ? [p.stoppedBy] : []), ...(p.givenBy ? [p.givenBy] : [])]);
    const fromName = names.get(p.fromBot) ?? p.fromBot;
    // The giver handed its work over since: only its split-off card says how the part ended.
    if (p.cardOnly) {
      await this.patchPartCard(p, fromName, names.get(botId) ?? botId);
      this.putMeta(seen, "1");
      return;
    }
    const how = p.state === "done" ? "is done" : p.state === "cancelled" ? `was stopped by ${p.stoppedBy ? (names.get(p.stoppedBy) ?? p.stoppedBy) : "a person"}` : "did not finish";
    const whose = p.givenBy ? `${names.get(p.givenBy) ?? p.givenBy} gave to ${fromName} before handing the work over to you` : `you gave to ${fromName}`;
    const text = `[team-chat] The part ${whose} ${how} (its topic ${p.partTopicId}; read it with chat-run topic state --topic ${p.partTopicId}):\n${p.result}`;
    if (!p.givenBy) await this.patchPartCard(p, fromName, names.get(botId) ?? botId);
    const live = this.liveRunOf(p.parentRun, p.topicId);
    if (live?.machine_id) {
      const inputId = `desk-part-${p.chainId}-${p.seq}${p.runId ? `-${p.runId.slice(-8)}` : ""}`.slice(0, 64);
      await linkStub(this.env, live.machine_id).toRun({ t: "run.input", runId: live.id, inputId, text, authorId: botId } as never);
    } else if (live) {
      this.addPart(live.id, text);
    } else {
      const anchor = p.parentRun ? this.run(p.parentRun) : undefined;
      if (anchor) {
        this.addPart(anchor.id, text);
        if (p.last) {
          const all = this.takeParts(anchor.id) ?? text;
          await this.start({ groupId: p.groupId, topicId: p.topicId, messageId: `part:${p.chainId}:${p.seq}${p.runId ? `:${p.runId.slice(-8)}` : ""}`, authorId: p.requesterId, body: all, mentioned: true }, anchor);
        }
      } else if (p.last) {
        await this.start({ groupId: p.groupId, topicId: p.topicId, messageId: `part:${p.chainId}:${p.seq}${p.runId ? `:${p.runId.slice(-8)}` : ""}`, authorId: p.requesterId, body: text, mentioned: true });
      }
    }
    this.putMeta(seen, "1");
  }

  /** The run of the step that is still going: the giver's run, a newer run of its line, or its active run in that 话题. */
  protected liveRunOf(runId: string | null, topicId: string): RunRow | undefined {
    let cur = runId ? this.run(runId) : undefined;
    for (let i = 0; cur && i < 20; i++) {
      if (cur.ended_at === null) return cur;
      const next = this.sql
        .exec<RunRow>("SELECT * FROM runs WHERE retry_of = ? OR follow_up_of = ? ORDER BY created_at DESC LIMIT 1", cur.id, cur.id)
        .toArray()[0] as RunRow | undefined;
      cur = next;
    }
    return isMainTopicId(topicId) ? undefined : this.activeRun(topicId);
  }

  protected addPart(runId: string, text: string): void {
    const raw = this.meta(`partsIn:${runId}`);
    const parts = raw ? (JSON.parse(raw) as string[]) : [];
    parts.push(text);
    this.putMeta(`partsIn:${runId}`, JSON.stringify(parts));
  }

  /** The split-off card says once how the part ended. Never throws. */
  protected async patchPartCard(p: PartRow & { groupId: string }, fromName: string, giverName: string): Promise<void> {
    const messageId = this.meta(`partCard:${p.chainId}:${p.seq}`);
    if (!messageId) return;
    const why = p.state === "cancelled" && p.stoppedBy ? CARD_TEXT.stoppedBy((await this.names([p.stoppedBy])).get(p.stoppedBy) ?? "") : cut(p.result.split("\n").find((l) => l.trim() !== "") ?? "", 200);
    const line = p.state === "done" ? CARD_TEXT.partDone(fromName, giverName) : CARD_TEXT.partNotDone(fromName, why);
    await this.closeChainCard(p.groupId, messageId, line);
  }

  protected async closeChainCard(groupId: string, messageId: string, line: string): Promise<void> {
    const raw = this.meta(`chainCard:${messageId}`);
    if (!raw) return;
    try {
      const saved = JSON.parse(raw) as { card: CardV2 };
      const card = closedCard(saved.card, line);
      unwrap(await roomStub(this.env, groupId).edit(groupId, this.actor(), { t: "edit", rid: "bot", groupId, messageId, body: card.fallback, cardV2: card } as never));
      this.putMeta(`chainCard:${messageId}`, JSON.stringify({ ...JSON.parse(raw), card }));
    } catch (err) {
      this.log.warn("hand-over card patch failed", { messageId, err });
    }
  }

  /**
   * 全部停止 on a hand-over card (a footer press, or an answer): the room checks the person again
   * and stops the whole piece of work; the card loses its button.
   */
  protected async chainStopPressed(groupId: string, messageId: string, actionId: string, actorId: string): Promise<void> {
    const chainId = actionId.slice(CHAIN_STOP.length);
    try {
      unwrap(await roomStub(this.env, groupId).rpc(groupId, "bots.chainStop", this.actor(), { chainId, byActorId: actorId }));
    } catch (err) {
      this.log.warn("chain stop failed", { chainId, err });
      return;
    }
    this.log.info("chain stopped", { chainId, actorId });
    const who = (await this.names([actorId])).get(actorId) ?? "";
    await this.closeChainCard(groupId, messageId, CARD_TEXT.stoppedBy(who));
  }

  // -------------------------------------------------------------------------
  // Alarm: runs that handed their work over, and reports the room did not take
  // -------------------------------------------------------------------------

  protected override async scheduleStall(): Promise<void> {
    await super.scheduleStall();
    const times: number[] = [];
    for (const row of this.sql.exec<{ key: string; value: string }>("SELECT key, value FROM meta WHERE key LIKE 'handedOver:%'").toArray()) {
      const r = this.run(row.key.slice("handedOver:".length));
      if (!r || r.ended_at !== null) continue;
      times.push((JSON.parse(row.value) as { at: number }).at + HANDED_OVER_STOP_MS);
    }
    if (this.sql.exec("SELECT 1 FROM meta WHERE key LIKE 'pendingState:%' LIMIT 1").toArray().length > 0) times.push(Date.now() + PENDING_STATE_RETRY_MS);
    if (times.length === 0) return;
    const at = Math.min(...times);
    const cur = await this.ctx.storage.getAlarm();
    if (cur === null || cur > at || cur < Date.now()) await this.ctx.storage.setAlarm(at);
  }

  override async alarm(): Promise<void> {
    await super.alarm();
    const now = Date.now();
    // 转交: the run that handed its work over and did not stop stops now, as done.
    const handed = this.sql.exec<{ id: string }>(`SELECT id FROM runs WHERE ${ACTIVE}`).toArray().filter(({ id }) => {
      const h = this.handedOverOf(id);
      return h !== null && h.at + HANDED_OVER_STOP_MS <= now;
    });
    for (const { id } of handed) {
      await this.serial(id, async () => {
        const r = this.run(id);
        if (!r || r.ended_at !== null) return;
        if (r.machine_id && !r.external) {
          await linkStub(this.env, r.machine_id)
            .toRun({ t: "run.cancel", runId: r.id } as never)
            .catch((err) => this.log.warn("run.cancel failed", { runId: r.id, err }));
        }
        await this.end(r, "done", "", "done", "");
      });
    }
    // Reports the room did not take go again (oldest first).
    for (const row of this.sql.exec<{ key: string; value: string }>("SELECT key, value FROM meta WHERE key LIKE 'pendingState:%'").toArray()) {
      try {
        const p = JSON.parse(row.value) as { groupId: string; s: Parameters<DeskDelegate["sendWorkState"]>[1] };
        await this.sendWorkState(p.groupId, p.s);
      } catch {
        this.sql.exec("DELETE FROM meta WHERE key = ?", row.key);
      }
    }
    await this.scheduleStall();
  }
}
