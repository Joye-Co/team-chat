// What a run reads of its group besides messages (docs/machine-protocol.md, "Topic context"):
// one card whole, the members, and one compact state card of a topic. Members cooperate through
// 产出 and state, so they must be able to read them. Room RPCs plus pure text; the routes are in
// routes.ts.
import type { RunMember } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoomRpc } from "../core/registry";
import { contextRows } from "../bots/room";
import { cut } from "../bots/dispatch";
import { type HandoverLine, topicHandovers } from "../bots/chains";
import { topicBots } from "../bots/participants";
import { relayLine } from "../bots/relay/engine";
import { type ContextRow, type Names, stamp } from "./render";

/** The longest state card (`topic state`). */
export const MAX_STATE_CHARS = 2000;

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** A card message of this group, as `viewerId` may see it (private cards hide their voters). */
export interface CardRead {
  topicId: string;
  row: ContextRow;
  card: unknown;
}

registerRoomRpc("context.card", (room, actor, args): CardRead => {
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const { messageId } = args as { messageId: string };
  const m = room.sql
    .exec<{ topic_id: string }>("SELECT topic_id FROM messages WHERE id = ? AND deleted_at IS NULL", messageId)
    .toArray()[0];
  const row = m ? contextRows(room.sql, actor.id, m.topic_id, "AND id = ?", [messageId], "ASC", 1)[0] : undefined;
  const ext = row?.ext as (ContextRow["ext"] & { cardV2?: unknown; card?: unknown }) | undefined;
  const card = ext?.cardV2 ?? ext?.card;
  if (!m || !row || card === undefined) throw new ApiError("not_found", "card not found");
  return { topicId: m.topic_id, row, card };
});

/** Raw state of one topic; `topicStateText` makes the card. */
export interface TopicStateData {
  title: string;
  messages: number;
  authors: { authorId: string; count: number; lastSeq: number; lastAt: number; lastBody: string }[];
  artifacts: { id: string; title: string; kind: string; version: number; updatedAt: number; createdBy: string; note: string | null }[];
  openCards: { messageId: string; title: string; from: string[] }[];
  decisions: { title: string; label: string; choices: string[]; by: string; at: number }[];
  /** design-relay: the AI members that ran here, and how their latest run stands. */
  participants?: { memberId: string; working: boolean; report: string | null; at: number }[];
  /** design-relay R2: hand-overs and split-offs of the pieces of work that touch this topic. */
  handovers?: HandoverLine[];
  /** design-relay R3: the topic's latest relay (按顺序做). */
  relay?: ReturnType<typeof relayLine>;
}

registerRoomRpc("context.topicState", (room, actor, args): TopicStateData => {
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const { topicId } = args as { topicId: string };
  const topic = room.sql.exec<{ title: string }>("SELECT title FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const live = "topic_id = ? AND deleted_at IS NULL";
  const messages = room.sql.exec<{ n: number }>(`SELECT COUNT(*) AS n FROM messages WHERE ${live}`, topicId).toArray()[0]?.n ?? 0;
  const authors = room.sql
    .exec<{ author_id: string; n: number; last: number }>(
      `SELECT author_id, COUNT(*) AS n, MAX(created_seq) AS last FROM messages
       WHERE ${live} AND COALESCE(kind, 'text') != 'system' GROUP BY author_id ORDER BY last DESC LIMIT 8`,
      topicId,
    )
    .toArray()
    .map((a) => {
      const m = room.sql
        .exec<{ body: string; created_at: number }>("SELECT body, created_at FROM messages WHERE topic_id = ? AND created_seq = ?", topicId, a.last)
        .toArray()[0];
      return { authorId: a.author_id, count: a.n, lastSeq: a.last, lastAt: m?.created_at ?? 0, lastBody: m?.body ?? "" };
    });
  const artifacts = room.sql
    .exec<{ id: string; title: string; kind: string; current_version: number; updated_at: number; created_by: string; note: string | null }>(
      `SELECT a.id, a.title, a.kind, a.current_version, a.updated_at, COALESCE(v.created_by, a.created_by) AS created_by, v.note
       FROM artifacts a LEFT JOIN artifact_versions v ON v.artifact_id = a.id AND v.version = a.current_version
       WHERE a.topic_id = ? ORDER BY a.updated_at DESC LIMIT 8`,
      topicId,
    )
    .toArray()
    .map((a) => ({ id: a.id, title: a.title, kind: a.kind, version: a.current_version, updatedAt: a.updated_at, createdBy: a.created_by, note: a.note }));
  // Open asks: the stored card state says who is waited on.
  const openCards = room.sql
    .exec<{ id: string; ext: string }>(`SELECT id, ext FROM messages WHERE ${live} AND ext LIKE '%"status":"open"%' ORDER BY created_seq DESC LIMIT 20`, topicId)
    .toArray()
    .flatMap((r) => {
      try {
        const x = JSON.parse(r.ext) as { cardV2?: { title?: string }; card?: { title?: string }; cardState?: { ask?: { status?: string; from?: string[] } } };
        const ask = x.cardState?.ask;
        if (ask?.status !== "open") return [];
        return [{ messageId: r.id, title: x.cardV2?.title ?? x.card?.title ?? "", from: ask.from ?? [] }];
      } catch {
        return [];
      }
    })
    .slice(0, 5);
  const answered = room.sql
    .exec<{ seq: number; title: string; action_label: string; choices: string | null; actor_id: string; at: number }>(
      `SELECT d.seq, d.title, d.action_label, d.choices, d.actor_id, d.at FROM card_decisions d
       JOIN messages m ON m.id = d.message_id AND m.deleted_at IS NULL
       WHERE d.topic_id = ? AND d.message_id NOT IN (SELECT message_id FROM card_private)
       ORDER BY d.seq DESC LIMIT 5`,
      topicId,
    )
    .toArray()
    .map((d) => ({ seq: d.seq, title: d.title, label: d.action_label, choices: d.choices ? (JSON.parse(d.choices) as string[]) : [], by: d.actor_id, at: d.at }));
  // Decisions recorded by hand (记一条) in this topic; `label` "" marks them.
  const recorded = room.sql
    .exec<{ seq: number; text: string; recorded_by: string; recorded_at: number }>(
      "SELECT seq, text, recorded_by, recorded_at FROM recorded_decisions WHERE topic_id = ? ORDER BY seq DESC LIMIT 5",
      topicId,
    )
    .toArray()
    .map((r) => ({ seq: r.seq, title: r.text, label: "", choices: [] as string[], by: r.recorded_by, at: r.recorded_at }));
  const decisions = [...answered, ...recorded]
    .sort((a, b) => b.seq - a.seq)
    .slice(0, 5)
    .map(({ seq: _seq, ...d }) => d);
  const participants = topicBots(room.sql, topicId).map((r) => ({ memberId: r.bot_id, working: r.working === 1, report: r.report, at: r.ended_at ?? r.started_at }));
  const handovers = topicHandovers(room.sql, topicId);
  const relay = relayLine(room.sql, topicId);
  return { title: topic.title, messages, authors, artifacts, openCards, decisions, participants, handovers, ...(relay ? { relay } : {}) };
});

/** The member ids a state card names. */
export function stateIds(d: TopicStateData): string[] {
  return [
    ...d.authors.map((a) => a.authorId),
    ...d.artifacts.map((a) => a.createdBy),
    ...d.openCards.flatMap((c) => c.from),
    ...d.decisions.map((x) => x.by),
    ...(d.participants ?? []).map((p) => p.memberId),
    ...(d.handovers ?? []).flatMap((h) => [...(h.from ? [h.from] : []), h.to]),
    ...(d.relay?.steps ?? []).map((s) => s.memberId),
  ];
}

/**
 * One compact state card of a topic (at most `MAX_STATE_CHARS`): who did what, which 产出
 * exist and their latest versions, open cards and who is waited on, the latest decisions.
 */
export function topicStateText(d: TopicStateData, name: Names): string {
  const parts: string[] = [`Topic "${cut(oneLine(d.title), 80)}" · ${d.messages} messages`];
  if (d.authors.length > 0) {
    parts.push("Who did what (newest first):");
    for (const a of d.authors) {
      parts.push(`- ${name(a.authorId)}: ${a.count} messages, last #${a.lastSeq} ${stamp(a.lastAt)}: ${cut(oneLine(a.lastBody), 80)}`);
    }
  }
  parts.push(d.artifacts.length > 0 ? "产出 (newest first; read with chat-run artifact get --id <id>):" : "产出: none yet");
  for (const a of d.artifacts) {
    parts.push(`- ${cut(oneLine(a.title), 60)} · a ${a.id} v${a.version} · ${a.kind} · ${name(a.createdBy)} ${stamp(a.updatedAt)}${a.note ? ` · ${cut(oneLine(a.note), 60)}` : ""}`);
  }
  if (d.participants && d.participants.length > 0) {
    parts.push("AI members in this topic:");
    for (const p of d.participants) parts.push(`- ${name(p.memberId)} · ${p.working ? "working" : (p.report ?? "ran here")} · ${stamp(p.at)}`);
  }
  if (d.handovers && d.handovers.length > 0) {
    parts.push("Hand-overs of this work (交接; a split-off works in its own topic):");
    d.handovers.forEach((h, i) => {
      const how = h.kind === "handoff" ? "转交" : h.kind === "relay" ? "接力" : "分出去";
      parts.push(`${i + 1}. ${h.from ? name(h.from) : "?"} →(${how}) ${name(h.to)} · ${h.state} · topic ${h.topicId}`);
    });
  }
  if (d.relay) {
    const r = d.relay;
    const steps = r.steps.map((s, i) => `${i === r.current ? "[" : ""}${name(s.memberId)}${s.note ? ` ${cut(oneLine(s.note), 30)}` : ""} · ${s.state}${i === r.current ? "]" : ""}`);
    parts.push(`Relay (按顺序做; the room wakes each step): ${steps.join(" → ")} · step ${r.current + 1}/${r.steps.length} · ${r.state}${r.reason ? ` (${r.reason})` : ""}`);
  }
  if (d.openCards.length > 0) {
    parts.push("Open cards (waiting for an answer):");
    for (const c of d.openCards) {
      parts.push(`- m ${c.messageId} · ${cut(oneLine(c.title), 60)} · waiting on ${c.from.length > 0 ? c.from.map(name).join(", ") : "anyone"}`);
    }
  }
  if (d.decisions.length > 0) {
    parts.push("Latest decisions:");
    for (const x of d.decisions) {
      if (x.label === "") {
        parts.push(`- ${cut(oneLine(x.title), 120)} (recorded by ${name(x.by)} ${stamp(x.at)})`);
        continue;
      }
      const what = x.choices.length > 0 ? `${x.label}: ${x.choices.join(", ")}` : x.label;
      parts.push(`- ${cut(oneLine(x.title), 60)} → ${cut(oneLine(what), 60)} (${name(x.by)} ${stamp(x.at)})`);
    }
  }
  // Whole lines within the budget; the cut says so.
  const out: string[] = [];
  let used = 0;
  for (const p of parts) {
    const n = [...p].length + 1;
    if (used + n > MAX_STATE_CHARS - 2) {
      out.push("…");
      break;
    }
    out.push(p);
    used += n;
  }
  return out.join("\n");
}

/** Members a run reads (`member list`); more are left out. */
export const MAX_RUN_MEMBERS = 200;

/** The active members of a group: name, 岗位, whether it is an AI member; people first. */
export async function groupMembers(db: D1Database, groupId: string): Promise<RunMember[]> {
  const rows = await db
    .prepare(
      `SELECT m.id, m.name, m.role, m.kind FROM group_members g JOIN members m ON m.id = g.member_id
       WHERE g.group_id = ? AND m.deactivated = 0 ORDER BY m.kind = 'bot', m.name LIMIT ?`,
    )
    .bind(groupId, MAX_RUN_MEMBERS)
    .all<{ id: string; name: string; role: string | null; kind: string }>();
  return rows.results.map((r) => ({ id: r.id, name: r.name, ...(r.role ? { role: r.role } : {}), ai: r.kind === "bot" }));
}

/** Members named in a run's prompt; the rest are read with `chat-run member list`. */
export const PROMPT_MEMBERS = 40;

/** The group's members for an executor's prompt, one line each. */
export function membersText(members: RunMember[], selfId: string): string {
  const shown = members.slice(0, PROMPT_MEMBERS).map((m) => {
    const tags = [m.role ? cut(oneLine(m.role), 60) : "", m.ai ? "AI member" : "", m.id === selfId ? "you" : ""].filter((t) => t !== "");
    return `- ${m.name}${tags.length > 0 ? ` · ${tags.join(" · ")}` : ""}`;
  });
  const more = members.length - shown.length;
  return ["Members of this group (name · 岗位):", ...shown, ...(more > 0 ? [`- … ${more} more: chat-run member list`] : [])].join("\n");
}
