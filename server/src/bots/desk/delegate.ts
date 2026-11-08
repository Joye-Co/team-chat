// BotDesk delegation between members (docs/design/agent-harness.zh.md 3.5), from a run's chat-run:
// - 转交 (`member.handoff`): the whole piece of work goes to another AI member in the same topic,
//   which becomes the topic's responsible member and starts a run with the work order.
// - 分出去 (`member.assign`): a part goes to another member in a new topic; the first member stays
//   responsible, and the result comes back to its topic (as input to its run, or a follow-up run).
// Bots' own messages never start runs, so the hand-over goes desk to desk, and a visible message
// says who took what. Depth and count limits stop delegation loops.
import { ulid } from "../../core/ids";
import { ApiError } from "../../core/errors";
import { isGroupMember } from "../../core/db";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import type { Brief } from "../../agent/frontdesk";
import { deskStub, linkStub } from "../env";
import { memberInfo, type TopicMessage } from "../room";
import type { RunRow } from "./core";
import { DeskFront } from "./front";

/** How deep a chain of hand-overs may go (A → B → C → D). */
export const MAX_DELEGATION_DEPTH = 3;
/** Parts one piece of work may give out by itself. */
export const MAX_ASSIGNMENTS = 8;

export interface Delegation {
  depth: number;
  /** The run the chain started from. */
  root: string;
  /** 分出去: where the result goes back to. */
  parent?: { botId: string; groupId: string; topicId: string; runId: string; title: string };
}

export interface DelegateIn {
  groupId: string;
  topicId: string;
  requesterId: string;
  brief: Brief;
  fromBotId: string;
  delegation: Delegation;
}

export abstract class DeskDelegate extends DeskFront {
  protected delegationOf(runId: string): Delegation | null {
    const raw = this.meta(`delegation:${runId}`);
    return raw ? (JSON.parse(raw) as Delegation) : null;
  }

  /** A run hands the work over (or a part of it) to another AI member. Answers the chat-run command. */
  protected async delegate(r: RunRow, op: "member.handoff" | "member.assign", body: unknown): Promise<unknown> {
    const b = (body ?? {}) as { member?: unknown; goal?: unknown; doneWhen?: unknown; notes?: unknown };
    if (typeof b.member !== "string" || b.member.trim() === "") throw new ApiError("invalid", "member is the other member's name or id");
    if (typeof b.goal !== "string" || b.goal.trim() === "" || [...b.goal].length > 1000) throw new ApiError("invalid", "goal is 1..1000 characters");
    const doneWhen = Array.isArray(b.doneWhen) ? b.doneWhen.filter((x): x is string => typeof x === "string" && x.trim() !== "").slice(0, 10).map((x) => x.slice(0, 300)) : [];
    const brief: Brief = { goal: b.goal.trim(), doneWhen, ...(typeof b.notes === "string" && b.notes.trim() ? { notes: b.notes.slice(0, 2000) } : {}) };
    const botId = this.botId as string;
    const target = await this.findMember(r.group_id, b.member.trim());
    if (target === botId) throw new ApiError("invalid", "that is you");
    const mine = this.delegationOf(r.id) ?? { depth: 0, root: r.id };
    if (mine.depth >= MAX_DELEGATION_DEPTH) throw new ApiError("conflict", `hand-overs go at most ${MAX_DELEGATION_DEPTH} deep`, "delegation.too_deep");
    const delegation: Delegation = { depth: mine.depth + 1, root: mine.root };
    const names = await this.names([target, botId]);
    const targetName = names.get(target) ?? target;

    if (op === "member.handoff") {
      // The topic becomes the other member's (a 群聊 has no responsible member).
      unwrap(await roomStub(this.env, r.group_id).rpc(r.group_id, "bots.transfer", this.actor(), { topicId: r.topic_id, to: target }));
      await this.post(r, `ho:${r.id}:${target}`, `@${targetName} 转交给你：${brief.goal}`, { mentions: [target] });
      const runId = await this.handOver(target, {
        groupId: r.group_id,
        topicId: r.topic_id,
        requesterId: r.requested_by,
        brief,
        fromBotId: botId,
        delegation,
      });
      return { member: target, topicId: r.topic_id, runId };
    }

    const count = Number(this.meta(`assigned:${mine.root}`) ?? "0");
    if (count >= MAX_ASSIGNMENTS) throw new ApiError("conflict", `one piece of work gives out at most ${MAX_ASSIGNMENTS} parts`, "delegation.too_many");
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `assigned:${mine.root}`, String(count + 1));
    const title = await this.topicTitle(r);
    // A new topic for the part, opened by this member with the other member mentioned.
    const sent = await this.room(r).send(r.group_id, this.actor(), {
      t: "send",
      rid: "bot",
      groupId: r.group_id,
      clientMsgId: `run:${r.id}:assign:${count + 1}`.slice(0, 64),
      body: `@${targetName} ${brief.goal}`,
      mentions: [target],
    } as never);
    if (!sent.ok) throw new ApiError(sent.error.code, sent.error.message, sent.error.reason);
    const topicId = sent.value.message.topicId as string;
    delegation.parent = { botId, groupId: r.group_id, topicId: r.topic_id, runId: r.id, title };
    const runId = await this.handOver(target, {
      groupId: r.group_id,
      topicId,
      requesterId: r.requested_by,
      brief,
      fromBotId: botId,
      delegation,
    });
    return { member: target, topicId, runId };
  }

  /** The other member's desk takes the work; its refusal comes back as an API error of this call. */
  protected async handOver(target: string, d: DelegateIn): Promise<string | null> {
    try {
      return await deskStub(this.env, target).takeDelegation(target, d);
    } catch (err) {
      throw new ApiError("conflict", String((err as Error)?.message ?? err).slice(0, 300), "delegation.refused");
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
      throw new ApiError("not_found", `no AI member "${nameOrId}" in this group (members that work on computers: ${known})`, "delegation.unknown_member");
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

  /** The other side: start a run with the work order handed over. Returns the run id or null. */
  async takeDelegation(botId: string, d: DelegateIn): Promise<string | null> {
    this.bind(botId);
    const info = (await memberInfo(this.env.DB, [botId])).get(botId);
    if (!info?.runner || info.deactivated || !(await isGroupMember(this.env.DB, d.groupId, botId))) {
      throw new ApiError("conflict", "that member takes no work now", "delegation.unavailable");
    }
    const key = `delegated:${ulid()}`;
    const m: TopicMessage = { groupId: d.groupId, topicId: d.topicId, messageId: key, authorId: d.requesterId, body: d.brief.goal, mentioned: true };
    const active = this.activeRun(d.topicId);
    if (active) throw new ApiError("conflict", "that member is already working in this topic", "delegation.busy");
    await this.start(m, undefined, [], undefined, d.brief);
    const run = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    if (!run) return null;
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `delegation:${run.id}`, JSON.stringify(d.delegation));
    this.log.info("delegation taken", { runId: run.id, from: d.fromBotId, depth: d.delegation.depth, parent: d.delegation.parent?.runId });
    return run.id;
  }

  /** A part this member gave out came back: to its run as input, or as a follow-up run. */
  async partDone(botId: string, p: { groupId: string; topicId: string; requesterId: string; fromName: string; topicTitle: string; ok: boolean; result: string }): Promise<void> {
    this.bind(botId);
    const text = `[team-chat] The part you gave to ${p.fromName} ${p.ok ? "is done" : "did not finish"} (topic "${p.topicTitle}"):\n${p.result}`;
    const active = this.activeRun(p.topicId);
    if (active?.machine_id) {
      await linkStub(this.env, active.machine_id).toRun({ t: "run.input", runId: active.id, inputId: `desk-part-${ulid()}`, text, authorId: botId } as never);
      return;
    }
    if (active) return;
    await this.start({ groupId: p.groupId, topicId: p.topicId, messageId: `part:${ulid()}`, authorId: p.requesterId, body: text, mentioned: true });
  }

  /** When a run that took a part ends, its result goes back to the member that gave it out. */
  protected async reportPart(r: RunRow, ok: boolean, result: string): Promise<void> {
    const d = this.delegationOf(r.id);
    if (!d?.parent) return;
    try {
      const names = await this.names([this.botId as string]);
      await deskStub(this.env, d.parent.botId).partDone(d.parent.botId, {
        groupId: d.parent.groupId,
        topicId: d.parent.topicId,
        requesterId: r.requested_by,
        fromName: names.get(this.botId as string) ?? "",
        topicTitle: await this.topicTitle(r),
        ok,
        result: result.slice(0, 4000),
      });
    } catch (err) {
      this.log.warn("part report failed", { runId: r.id, err });
    }
  }
}
