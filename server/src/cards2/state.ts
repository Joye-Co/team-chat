// Server-owned card state (INTERACT §3.5, §3.6): who an ask is for, ask rounds, blocking asks
// and the topic status, expiry and poll timers, tallies, the decision surface of a rev, and
// the derived v1 card for 1.0.x readers (CARD21 §7, INTERACT §4.5).
import {
  type Card,
  type CardAnswer,
  type CardAction,
  type CardState,
  type CardV2,
  type CardV2Action,
  type CardV2Block,
  type Cause,
  cardV2ToV1,
  childIds,
  pressableAsV1,
  takesInput,
} from "@chat/protocol";
import { listMembers } from "../core/db";
import { ApiError } from "../core/errors";
import type { RoomContext, RoomTx } from "../core/registry";
import { setFollowing } from "../core/topicstate";
import { requesterResolvers } from "./delivery";
import { privacyOf } from "./privacy";

export type MemberInfo = { name: string; kind: "human" | "bot"; level: "owner" | "member"; deactivated: boolean };
export type Directory = Map<string, MemberInfo>;

/** Every member of the server (small teams), for names, kinds and levels. */
export async function directory(room: RoomContext): Promise<Directory> {
  const rows = await listMembers(room.env.DB);
  return new Map(rows.map((r) => [r.id, { name: r.name, kind: r.kind, level: r.level, deactivated: !!r.deactivated }]));
}

// ---------------------------------------------------------------------------
// Decision surface
// ---------------------------------------------------------------------------

type Block = CardV2Block;

/** Footer actions that answer an open ask (`press` is live, `link` sends nothing). */
const isAnswerAction = (a: CardV2Action) => a.kind !== "press" && a.kind !== "link";

/** Block ids of the decision surface of the card's ask (INTERACT §2.2). */
export function surfaceIds(card: CardV2): Set<string> {
  const ids = new Set<string>();
  const byId = new Map(card.blocks.map((b) => [b.id, b] as const));
  const named = (card.actions ?? []).filter(isAnswerAction).flatMap((a) => a.inputs ?? []);
  for (const id of named) ids.add(id);
  for (const b of card.blocks) {
    if (b.deliver === "immediate" || b.type === "compare" || b.type === "poll") ids.add(b.id);
    if (b.type === "questionnaire") {
      ids.add(b.id);
      for (const c of b.children) ids.add(c);
    }
  }
  // 2.0 cards: a submit action takes every input block.
  if (named.length === 0) {
    for (const b of card.blocks) {
      if (b.type === "group") for (const c of b.blocks ?? []) if (takesInput(c) || c.type === "draft") ids.add(c.id);
      if ((takesInput(b) && b.type !== "table") || b.type === "draft") ids.add(b.id);
    }
  }
  void byId;
  return ids;
}

/** A stable text of the decision surface: the ask, its blocks and the answer actions. */
export function surfaceKey(card: CardV2): string {
  const ids = surfaceIds(card);
  const blocks = card.blocks.filter((b) => ids.has(b.id));
  const ask = card.ask ? { id: card.ask.id ?? "a1", from: card.ask.from } : null;
  const text = JSON.stringify({ ask, blocks, actions: (card.actions ?? []).filter(isAnswerAction) });
  // FNV-1a, two seeds: a short key, compared for equality only.
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 16777619);
    h2 = Math.imul(h2 ^ c, 2166136261);
  }
  return `${(h1 >>> 0).toString(16)}${(h2 >>> 0).toString(16)}:${text.length}`;
}

export function recordRev(sql: SqlStorage, messageId: string, card: CardV2): void {
  sql.exec("INSERT OR REPLACE INTO card_revs (message_id, rev, surface) VALUES (?, ?, ?)", messageId, card.rev, surfaceKey(card));
}

export function revSurface(sql: SqlStorage, messageId: string, rev: number): string | undefined {
  return sql.exec<{ surface: string }>("SELECT surface FROM card_revs WHERE message_id = ? AND rev = ?", messageId, rev).toArray()[0]?.surface;
}

/** The poll of the card's decision surface, if any. */
export function pollOf(card: CardV2): Extract<Block, { type: "poll" }> | undefined {
  return card.blocks.find((b): b is Extract<Block, { type: "poll" }> => b.type === "poll");
}

// ---------------------------------------------------------------------------
// Who: requester, ask.from, emitters
// ---------------------------------------------------------------------------

/**
 * The requester of the card's author in a topic (INTERACT §3.5): a registered resolver (the
 * delivery feature: the requester of the author's active run), else the author of the newest
 * human message in the topic that mentions the author, else the topic's creator.
 */
export async function requesterOf(room: RoomContext, dir: Directory, authorId: string, topicId: string | null): Promise<string | undefined> {
  if (!topicId) return undefined;
  for (const r of requesterResolvers) {
    const id = await r(room, authorId, topicId);
    if (id) return id;
  }
  const rows = room.sql
    .exec<{ author_id: string }>(
      `SELECT m.author_id FROM messages m JOIN mentions mn ON mn.message_id = m.id AND mn.member_id = ?
       WHERE m.topic_id = ? AND m.deleted_at IS NULL AND m.author_id != ? ORDER BY m.created_seq DESC LIMIT 20`,
      authorId,
      topicId,
      authorId,
    )
    .toArray();
  for (const r of rows) if (dir.get(r.author_id)?.kind === "human") return r.author_id;
  return room.sql.exec<{ created_by: string }>("SELECT created_by FROM topics WHERE id = ?", topicId).toArray()[0]?.created_by;
}

/** Member ids an `ask.from` (or `emitBy`) list names: ids, "owners", "requester". */
export async function resolveFrom(
  room: RoomContext,
  dir: Directory,
  from: string[],
  authorId: string,
  topicId: string | null,
  requester: () => Promise<string | undefined> = () => requesterOf(room, dir, authorId, topicId),
): Promise<string[]> {
  const out = new Set<string>();
  for (const f of from) {
    if (f === "owners") {
      for (const id of room.memberIds()) if (dir.get(id)?.level === "owner") out.add(id);
    } else if (f === "requester") {
      const id = await requester();
      if (id) out.add(id);
    } else if (room.isMember(f)) out.add(f);
  }
  return [...out].filter((id) => dir.get(id)?.deactivated !== true);
}

function pollVoters(room: RoomContext, dir: Directory, poll: Extract<Block, { type: "poll" }>): string[] {
  const ids = poll.voters === "group" ? room.memberIds() : poll.voters.filter((v) => room.isMember(v));
  return ids.filter((id) => dir.get(id)?.kind === "human" && !dir.get(id)?.deactivated);
}

// ---------------------------------------------------------------------------
// State transitions
// ---------------------------------------------------------------------------

/** The requester resolved for a new ask round of a card not stored yet (a send), by state. */
const unsavedRequester = new WeakMap<CardState, { askId: string; id: string | null }>();

function storeRequester(sql: SqlStorage, messageId: string, askId: string, id: string | null): void {
  sql.exec("INSERT OR IGNORE INTO card_requesters (message_id, ask_id, member_id) VALUES (?, ?, ?)", messageId, askId, id);
}

/** Stores the requester a send resolved for its ask round (call in the send's transaction). */
export function rememberRequester(sql: SqlStorage, messageId: string, state: CardState): void {
  const r = unsavedRequester.get(state);
  if (r) storeRequester(sql, messageId, r.askId, r.id);
}

/**
 * The requester of ask round `askId` (INTERACT §3.5): resolved on the round's first write and
 * stored, then kept for every later write with the same ask id, even when the author's active
 * run now has another requester.
 */
function roundRequester(
  room: RoomContext,
  dir: Directory,
  authorId: string,
  topicId: string | null,
  askId: string,
  sameRound: boolean,
  messageId: string | undefined,
  state: CardState,
): () => Promise<string | undefined> {
  return async () => {
    if (messageId !== undefined && sameRound) {
      const row = room.sql
        .exec<{ member_id: string | null }>("SELECT member_id FROM card_requesters WHERE message_id = ? AND ask_id = ?", messageId, askId)
        .toArray()[0];
      if (row) return row.member_id ?? undefined;
    }
    const id = (await requesterOf(room, dir, authorId, topicId)) ?? null;
    if (messageId !== undefined) storeRequester(room.sql, messageId, askId, id);
    else unsavedRequester.set(state, { askId, id });
    return id ?? undefined;
  };
}

/**
 * The card state after a write of `card` (send, edit or patch): the ask round (same id keeps
 * the round and its answers; a new id closes an open one as superseded; no ask closes it),
 * resolved `ask.from` and `requester` emitters, tallies.
 */
export async function stateForWrite(
  room: RoomContext,
  dir: Directory,
  card: CardV2,
  prev: CardState | undefined,
  authorId: string,
  topicId: string | null,
  now: number,
  /** The stored card's message (unset on send: `rememberRequester` stores the round's requester). */
  messageId?: string,
): Promise<CardState> {
  const state: CardState = { ...(prev ?? {}), answers: prev?.answers ?? [] };
  const ask = card.ask;
  if (!ask) {
    if (state.ask?.status === "open") state.ask = { ...state.ask, status: "closed" };
  } else {
    const id = ask.id ?? "a1";
    const poll = pollOf(card);
    const requester = roundRequester(room, dir, authorId, topicId, id, state.ask?.id === id, messageId, state);
    const from = poll ? pollVoters(room, dir, poll) : await resolveFrom(room, dir, ask.from, authorId, topicId, requester);
    const blocking = ask.blocking ?? card.intent === "ask";
    const expiresAt = ask.expiresAt ?? (poll ? poll.closesAt : undefined);
    if (state.ask && state.ask.id === id) {
      const next = { ...state.ask, from, blocking };
      if (expiresAt === undefined) delete next.expiresAt;
      else next.expiresAt = expiresAt;
      if (next.status === "expired" && expiresAt !== undefined && expiresAt > now) next.status = "open";
      state.ask = next;
    } else {
      state.ask = { id, status: expiresAt !== undefined && expiresAt <= now ? "expired" : "open", from, blocking };
      if (expiresAt !== undefined) state.ask.expiresAt = expiresAt;
    }
  }
  // emitBy "requester", resolved once and kept.
  const emitters = { ...(prev?.emitters ?? {}) };
  for (const b of card.blocks) {
    if (b.emitBy === "requester" && !emitters[b.id]) {
      const id = await requesterOf(room, dir, authorId, topicId);
      emitters[b.id] = id ? [id] : [];
    }
  }
  if (Object.keys(emitters).length > 0) state.emitters = emitters;
  else delete state.emitters;
  retally(state, card);
  return state;
}

/** Recomputes the poll tally from the answers (the newest live answer per voter). */
export function retally(state: CardState, card: CardV2): void {
  const poll = pollOf(card);
  if (!poll) {
    delete state.tally;
    delete state.tallyBy;
    return;
  }
  const latest = new Map<string, string>();
  for (const a of state.answers) {
    if (a.retractedAt !== undefined) continue;
    const v = a.blockId === poll.id ? a.value : a.inputs?.[poll.id];
    const x = v && typeof v === "object" && !Array.isArray(v) ? (v as { value?: unknown }).value : v;
    if (typeof x === "string") latest.set(a.actorId, x);
  }
  const tally: Record<string, number> = {};
  const by: Record<string, string[]> = {};
  for (const o of poll.options) {
    tally[o.id] = 0;
    by[o.id] = [];
  }
  for (const [actor, x] of latest) {
    if (!(x in tally)) continue;
    tally[x] += 1;
    by[x].push(actor);
  }
  state.tally = tally;
  if (poll.public) state.tallyBy = by;
  else delete state.tallyBy;
}

/** The timer of a card: its open ask's expiry (polls: closesAt). */
export function syncTimer(room: RoomContext, messageId: string, state: CardState): void {
  const at = state.ask?.status === "open" ? state.ask.expiresAt : undefined;
  if (at === undefined) {
    room.sql.exec("DELETE FROM card_timers WHERE message_id = ?", messageId);
    return;
  }
  room.sql.exec("INSERT OR REPLACE INTO card_timers (message_id, at) VALUES (?, ?)", messageId, at);
  room.wakeAt(at);
}

/**
 * Keeps the topic status in step with its open blocking asks: `waiting_answer` with
 * `waitingOn` = the union of their `from` while one is open; back to `in_progress` when the
 * last one closes (only from `waiting_answer`). Non-blocking asks change nothing.
 */
export function refreshTopicWaiting(room: RoomContext, tx: RoomTx, topicId: string, actorId: string): void {
  const topic = room.sql
    .exec<{ status: string; waiting_on: string }>("SELECT status, waiting_on FROM topics WHERE id = ?", topicId)
    .toArray()[0];
  if (!topic) return;
  const rows = room.sql
    .exec<{ from_ids: string }>(
      `SELECT json_extract(ext, '$.cardState.ask.from') AS from_ids FROM messages
       WHERE topic_id = ? AND deleted_at IS NULL AND json_extract(ext, '$.cardState.ask.status') = 'open'
         AND json_extract(ext, '$.cardState.ask.blocking') = 1`,
      topicId,
    )
    .toArray();
  const before = JSON.parse(topic.waiting_on ?? "[]") as string[];
  let status = topic.status;
  let waitingOn = before;
  if (rows.length > 0) {
    const ids = new Set<string>();
    for (const r of rows) for (const id of JSON.parse(r.from_ids ?? "[]") as string[]) ids.add(id);
    status = "waiting_answer";
    waitingOn = [...ids];
  } else if (topic.status === "waiting_answer") {
    status = "in_progress";
    waitingOn = [];
  } else return;
  const same = status === topic.status && waitingOn.length === before.length && waitingOn.every((id) => before.includes(id));
  if (same) return;
  const seq = tx.nextSeq();
  room.sql.exec("UPDATE topics SET status = ?, waiting_on = ?, last_seq = ? WHERE id = ?", status, JSON.stringify(waitingOn), seq, topicId);
  const waitingAdded = waitingOn.filter((id) => !before.includes(id));
  for (const id of waitingAdded) setFollowing(room.sql, id, topicId, true);
  tx.append({
    type: "topic.updated",
    actorId,
    topicId,
    data: { topicId, status, waitingOn },
    hints: { waitingAdded },
  });
}

/**
 * Checks of a card against the room: an `embed` names a 产出 version of this group, and its
 * `events` are page events that version declares (V11-INC R5).
 */
export function checkEmbeds(sql: SqlStorage, card: CardV2): void {
  for (const b of card.blocks) {
    if (b.type !== "embed") continue;
    const v = sql
      .exec<{ events: string | null }>("SELECT events FROM artifact_versions WHERE artifact_id = ? AND version = ?", b.artifactId, b.version)
      .toArray()[0];
    if (!v) throw new ApiError("invalid", `embed ${b.id}: no such 产出 version in this group`, "card.limits");
    const declared = new Set(((v.events ? JSON.parse(v.events) : []) as { name: string }[]).map((e) => e.name));
    for (const name of b.events ?? []) {
      if (!declared.has(name)) throw new ApiError("invalid", `embed ${b.id}: the page does not declare ${name}`, "card.undeclared_event");
    }
  }
}

// ---------------------------------------------------------------------------
// Derived v1 card
// ---------------------------------------------------------------------------

const V1_TYPES = new Set(["text", "fields", "link", "image", "code", "list", "table", "progress", "divider"]);
const PICK = "tc.pick";
const MAX_PICKS = 4;

function cut(s: string, max: number): string {
  const cps = [...s];
  return cps.length <= max ? s : `${cps.slice(0, max - 1).join("")}…`;
}

/** The single-choice block a v1 button can answer, with its options (CARD21 §7.4). */
function pickable(card: CardV2): { block: Block; options: { value: string | boolean; label: string }[] } | undefined {
  const ids = [...surfaceIds(card)];
  if (ids.length !== 1) return undefined;
  const b = card.blocks.find((x) => x.id === ids[0]);
  if (!b) return undefined;
  const commits = b.deliver === "immediate" || b.type === "compare" || b.type === "confirm";
  const submits = (card.actions ?? []).filter((a) => (a.kind ?? "submit") === "submit" && a.inputs?.length === 1 && a.inputs[0] === b.id);
  if (!commits && submits.length !== 1) return undefined;
  let options: { value: string | boolean; label: string }[];
  switch (b.type) {
    case "decision":
    case "imageChoice":
      if (b.multiple) return undefined;
      options = b.options.map((o) => ({ value: o.id, label: o.label }));
      break;
    case "compare":
      options = b.options.map((o) => ({ value: o.id, label: o.label }));
      break;
    case "confirm":
      return { block: b, options: [{ value: true, label: b.yes }, { value: false, label: b.no }] };
    default:
      return undefined;
  }
  return options.length <= MAX_PICKS ? { block: b, options } : undefined;
}

/** A pick a v1 button carries in its `value`. */
export type PickValue = { tc: typeof PICK; blockId: string; value: string | boolean };
export function isPick(v: unknown): v is PickValue {
  return !!v && typeof v === "object" && (v as { tc?: unknown }).tc === PICK;
}

function chosenLabel(card: CardV2, a: CardAnswer): string | undefined {
  const pickFrom = (b: Block, v: unknown): string | undefined => {
    const x = v && typeof v === "object" && !Array.isArray(v) ? (v as { value?: unknown }).value : v;
    if (b.type === "confirm") return x === true ? b.yes : x === false ? b.no : undefined;
    if (b.type === "decision" || b.type === "imageChoice" || b.type === "compare" || b.type === "poll") {
      return (b.options as { id: string; label: string }[]).find((o) => o.id === x)?.label;
    }
    return undefined;
  };
  if (a.blockId) {
    const b = card.blocks.find((x) => x.id === a.blockId);
    const l = b ? pickFrom(b, a.value) : undefined;
    if (l) return l;
  }
  for (const [id, v] of Object.entries(a.inputs ?? {})) {
    const b = card.blocks.find((x) => x.id === id);
    const l = b ? pickFrom(b, v) : undefined;
    if (l) return l;
  }
  return a.actionId ? card.actions?.find((x) => x.id === a.actionId)?.label : undefined;
}

/** The v1 status line from the card state: 等 {name} 拍板, {name} 选了 {option}, 已过期, 已作废. */
export function statusLabel(card: CardV2, state: CardState | undefined, dir: Directory): string | undefined {
  const ask = state?.ask;
  if (!ask) return undefined;
  const name = (id: string) => dir.get(id)?.name ?? "成员";
  switch (ask.status) {
    case "open":
      return ask.from.length === 1 ? `等 ${name(ask.from[0])} 拍板` : `等 ${ask.from.length} 位成员拍板`;
    case "answered": {
      // Private answers (./privacy): the shared v1 card names no voter and no choice.
      if (privacyOf(card)) return "已回答";
      const a = [...(state?.answers ?? [])].reverse().find((x) => x.askId === ask.id && x.retractedAt === undefined);
      if (!a) return undefined;
      const label = chosenLabel(card, a);
      return label ? `${name(a.actorId)} 选了 ${label}` : `${name(a.actorId)} 回答了`;
    }
    case "expired":
      return "已过期";
    case "superseded":
      return "已作废";
    default:
      return undefined;
  }
}

/**
 * The v1 card stored next to `cardV2` and refreshed on every rev and state change: the
 * protocol derivation (`cardV2ToV1`), "这部分需要新版本：" before the fallback when the card has
 * blocks v1 cannot show, a status line from the state, and v1 buttons: one per option of a
 * single-choice decision surface (<= 4 options) while the ask is open, else the plain
 * actions of a card with nothing to fill.
 */
export function deriveV1(card: CardV2, state: CardState | undefined, dir: Directory): Card {
  const v1 = cardV2ToV1(card);
  const allV1 = card.blocks.every((b) => V1_TYPES.has(b.type));
  if (!allV1 && card.fallback !== "" && v1.blocks[0]?.type === "text") {
    v1.blocks[0] = { type: "text", text: cut(`这部分需要新版本：${card.fallback}`, 8000) };
  }
  // Metrics read "label: value (delta)" (INTERACT §4.5).
  const fmt = (n: number) => (Number.isInteger(n) ? String(n) : String(Math.round(n * 100) / 100));
  const done = new Set<object>();
  for (const b of card.blocks) {
    if (b.type !== "metric" || b.delta === undefined) continue;
    const value = `${fmt(b.value)}${b.unit ?? ""}`;
    const hit = v1.blocks.find(
      (x) => x.type === "fields" && !done.has(x) && x.fields.length === 1 && x.fields[0].label === b.label && x.fields[0].value === value,
    );
    if (hit && hit.type === "fields") {
      done.add(hit);
      hit.fields[0] = { label: b.label, value: cut(`${value} (${b.delta > 0 ? "+" : ""}${fmt(b.delta)}${b.unit ?? ""})`, 1000) };
    }
  }
  const label = statusLabel(card, state, dir);
  if (label) v1.status = { label: cut(label, 40) };
  const open = state?.ask?.status === "open";
  delete v1.actions;
  if (card.ask && !open) return v1;
  const actions: CardAction[] = [];
  const pick = open ? pickable(card) : undefined;
  if (pick) {
    pick.options.forEach((o, i) => {
      const value: PickValue = { tc: PICK, blockId: pick.block.id, value: o.value };
      actions.push({ id: `${PICK}.${i + 1}`, label: cut(pick.block.type === "confirm" ? o.label : `选 ${o.label}`, 40), value });
    });
  } else if (pressableAsV1(card)) {
    for (const a of card.actions ?? []) {
      if (a.kind === "link") continue;
      const out: CardAction = { id: a.id, label: a.label };
      if (a.style) out.style = a.style;
      if (a.confirm) out.confirm = a.confirm.title;
      actions.push(out);
    }
  }
  if (actions.length > 0) v1.actions = actions.slice(0, 10);
  return v1;
}

/** The children of a block, for removal walks. */
export function descendants(card: CardV2, id: string): string[] {
  const byId = new Map(card.blocks.map((b) => [b.id, b] as const));
  const out: string[] = [];
  const walk = (x: string) => {
    const b = byId.get(x);
    if (!b) return;
    for (const c of childIds(b)) {
      out.push(c);
      walk(c);
    }
  };
  walk(id);
  return out;
}

export type { Cause };
