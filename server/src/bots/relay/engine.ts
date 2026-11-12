// Usable (design-relay R3): relays (按顺序做). A person writes the order in a topic
// (`@A 写 → @B 审 → @A 改`, parse.ts); the room checks that every member can take the work, makes
// a chain (chains.ts, root kind `message`) and wakes one step after the other: each step is a
// chain step of kind `relay`, handed to its member's desk through the bots outbox (`relay` rows,
// desk/relay.ts `takeRelay`). A step ends when its run ends (`bots.workState`), after any
// hand-over (转交) inside it went on with the other member and every part it gave out came back.
// The room then wakes the next member with the previous step's result, 产出 and code location,
// or stops: a failed or blocked step, a run a person stopped, code left unpushed before another
// member, or a step that could not be handed over. AI members never wake each other by
// messages: only the room does, from the states the desks report.
import { type BotRun, isMainTopicId, type RunCode, type TopicRelay, type TopicStatus } from "@chat/protocol";
import { ApiError, unwrap } from "../../core/errors";
import { roomStub } from "../../core/env";
import { ulid } from "../../core/ids";
import type { Logger } from "../../core/logger";
import type { Env } from "../../core/env";
import {
  type Actor,
  type ArchivedEvent,
  type RoomContext,
  type RoomTx,
  registerEventWriter,
  registerMessageWriteHook,
  registerRoomMigration,
  registerRoomRpc,
  registerTopicFields,
} from "../../core/registry";
import { parseExt, requestExt, type RequestExt } from "../../context/render";
import { MAX_DEPTH, MAX_RELAY_RETRIES, chainRow, onChainStop, stepRow, stopChain, type StepRow } from "../chains";
import { deskStub } from "../env";
import { type RolledUp, type WorkState, publishWorkState, registerWorkStateOverlay, topicBots } from "../participants";
import { RelayParseError, looksLikeRelay, parseRelay } from "./parse";

/** Room migration `relay-1` (design-relay 3.2), plus an index to find a chain's relay. */
export const RELAY_MIGRATION = `
  CREATE TABLE topic_relays (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    chain_id TEXT NOT NULL,
    started_by TEXT NOT NULL,
    goal TEXT NOT NULL,
    steps TEXT NOT NULL,
    current INTEGER NOT NULL,
    state TEXT NOT NULL,
    reason TEXT,
    stopped_by TEXT,
    retries INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
  );
  CREATE INDEX topic_relays_topic ON topic_relays(topic_id, state);
  CREATE UNIQUE INDEX topic_relays_message ON topic_relays(message_id);
  CREATE INDEX topic_relays_chain ON topic_relays(chain_id);
`;

registerRoomMigration({ name: "relay-1", up: RELAY_MIGRATION });

/** The previous step's result a next member gets, at most. */
export const MAX_RELAY_RESULT = 1500;
/** Events older than this are imports (restore from the archive): they wake no step. */
const LIVE_WINDOW_MS = 10 * 60_000;

/** One step as the room keeps it: the wire step plus what the engine needs. */
interface StoredStep {
  memberId: string;
  note?: string;
  state: string;
  runId?: string;
  /** The chain step of the current try (a retry makes a new one). */
  seq: number;
  /** Every chain step of kind `relay` this step had (its tries). */
  seqs: number[];
  /** Every run of this step (tries, follow-ups, the run a hand-over went on with). */
  runIds: string[];
  /** How it ended: its result (<= 1500) and code location, for the next member. */
  result?: string;
  code?: RunCode | null;
}

type RelayRow = {
  id: string;
  topic_id: string;
  message_id: string;
  chain_id: string;
  started_by: string;
  goal: string;
  steps: string;
  current: number;
  state: string;
  reason: string | null;
  stopped_by: string | null;
  retries: number;
  updated_at: number;
};

interface Relay {
  id: string;
  topicId: string;
  messageId: string;
  chainId: string;
  startedBy: string;
  goal: string;
  steps: StoredStep[];
  current: number;
  state: string;
  reason: string | null;
  stoppedBy: string | null;
  retries: number;
  updatedAt: number;
}

function fromRow(r: RelayRow): Relay {
  return {
    id: r.id,
    topicId: r.topic_id,
    messageId: r.message_id,
    chainId: r.chain_id,
    startedBy: r.started_by,
    goal: r.goal,
    steps: JSON.parse(r.steps) as StoredStep[],
    current: r.current,
    state: r.state,
    reason: r.reason,
    stoppedBy: r.stopped_by,
    retries: r.retries,
    updatedAt: r.updated_at,
  };
}

function one(sql: SqlStorage, where: string, ...params: unknown[]): Relay | undefined {
  const r = sql.exec<RelayRow>(`SELECT * FROM topic_relays WHERE ${where} ORDER BY id DESC LIMIT 1`, ...params).toArray()[0] as RelayRow | undefined;
  return r ? fromRow(r) : undefined;
}

export function relayById(sql: SqlStorage, id: string): Relay | undefined {
  return one(sql, "id = ?", id);
}
/** The topic's latest relay (`Topic.relay`). */
export function latestRelay(sql: SqlStorage, topicId: string): Relay | undefined {
  return one(sql, "topic_id = ?", topicId);
}
/** The topic's relay that is running or paused. */
export function activeRelay(sql: SqlStorage, topicId: string): Relay | undefined {
  return one(sql, "topic_id = ? AND state IN ('running', 'paused')", topicId);
}
/** True when the message started a relay: it goes to no member by its @ (room.ts `deliver`). */
export function isRelayMessage(sql: SqlStorage, messageId: string): boolean {
  return sql.exec("SELECT 1 FROM topic_relays WHERE message_id = ?", messageId).toArray().length > 0;
}

function save(sql: SqlStorage, r: Relay, now = Date.now()): void {
  r.updatedAt = Math.max(now, r.updatedAt + 1);
  sql.exec(
    "UPDATE topic_relays SET steps = ?, current = ?, state = ?, reason = ?, stopped_by = ?, retries = ?, updated_at = ? WHERE id = ?",
    JSON.stringify(r.steps),
    r.current,
    r.state,
    r.reason,
    r.stoppedBy,
    r.retries,
    r.updatedAt,
    r.id,
  );
}

/** The relay as clients get it. */
export function toTopicRelay(r: Relay): TopicRelay {
  return {
    id: r.id,
    startedBy: r.startedBy,
    goal: r.goal,
    current: r.current,
    state: r.state,
    ...(r.reason ? { reason: r.reason } : {}),
    ...(r.stoppedBy ? { stoppedBy: r.stoppedBy } : {}),
    updatedAt: r.updatedAt,
    steps: r.steps.map((s) => ({ memberId: s.memberId, ...(s.note ? { note: s.note } : {}), state: s.state, ...(s.runId ? { runId: s.runId } : {}) })),
  };
}

registerTopicFields((sql, topicId) => {
  if (isMainTopicId(topicId)) return {};
  const r = latestRelay(sql, topicId);
  return r ? { relay: toTopicRelay(r) } : {};
});

/**
 * The topic's status with its relay (design-relay 3.4, rule 3): while a relay runs and no AI
 * member works, the topic is in progress (a step that ended waiting for review is not the
 * topic's state while more steps come); a paused relay is stuck and waits for whoever started it.
 * A working member's own wait (an approval, 盯活) still shows. Every publish carries the relay.
 */
export function relayOverlay(sql: SqlStorage, topicId: string, next: RolledUp): { next: RolledUp; extra?: Record<string, unknown> } {
  if (isMainTopicId(topicId)) return { next };
  const r = latestRelay(sql, topicId);
  if (!r) return { next };
  const extra = { relay: toTopicRelay(r) };
  if (r.state === "running") {
    const working = topicBots(sql, topicId).some((x) => x.working === 1);
    return { next: working && next ? next : { status: "in_progress", waitingOn: [] }, extra };
  }
  if (r.state === "paused") return { next: { status: "blocked", waitingOn: [r.startedBy] }, extra };
  return { next, extra };
}

registerWorkStateOverlay(relayOverlay);

// ---------------------------------------------------------------------------
// Starting a relay: the message write hook
// ---------------------------------------------------------------------------

type MemberRow = { id: string; name: string; kind: string; deactivated: number; runner: number };

async function membersOf(db: D1Database, ids: string[]): Promise<MemberRow[]> {
  const uniq = [...new Set(ids)];
  if (uniq.length === 0) return [];
  const rows = await db
    .prepare(
      `SELECT m.id, m.name, m.kind, m.deactivated, r.member_id IS NOT NULL AS runner FROM members m
       LEFT JOIN bot_runners r ON r.member_id = m.id AND r.enabled = 1
       WHERE m.id IN (${uniq.map(() => "?").join(",")})`,
    )
    .bind(...uniq)
    .all<MemberRow>();
  return rows.results;
}

/**
 * Parses a person's arrow message in a topic and refuses it when it cannot run: a broken arrow
 * line (`relay.bad_steps`), a member that cannot take the work (`relay.member_unavailable` with
 * `memberIds`), a relay already running or paused in the topic (`relay.running`). A message of
 * an AI member, in a 群聊 or in a DM is never a relay.
 */
registerMessageWriteHook({
  async send(frame, room, actor, draft) {
    if (actor.kind !== "human" || room.groupKind !== "group") return undefined;
    const topicId = typeof frame.topicId === "string" ? frame.topicId : undefined;
    if (topicId && isMainTopicId(topicId)) return undefined;
    const mentions = Array.isArray(frame.mentions) ? [...new Set(frame.mentions.filter((x): x is string => typeof x === "string"))] : [];
    // Only an arrow line costs a member lookup: other messages never wait on D1 here.
    if (mentions.length === 0 || !looksLikeRelay(draft.body)) return undefined;
    const members = await membersOf(room.env.DB, mentions);
    let parsed;
    try {
      parsed = parseRelay(draft.body, members);
    } catch (err) {
      if (err instanceof RelayParseError) throw new ApiError("invalid", err.message, err.reason);
      throw err;
    }
    if (!parsed) return undefined;
    const byId = new Map(members.map((m) => [m.id, m]));
    const cannot = [
      ...new Set(
        parsed.steps
          .map((s) => s.memberId)
          .filter((id) => {
            const m = byId.get(id);
            return !m || m.kind !== "bot" || m.deactivated || !m.runner || !room.isMember(id);
          }),
      ),
    ];
    if (cannot.length > 0) throw new ApiError("invalid", "some members cannot take this work now", "relay.member_unavailable", undefined, cannot);
    if (topicId && activeRelay(room.sql, topicId)) throw new ApiError("conflict", "this topic already has a relay; stop it first", "relay.running");
    const draftRelay = parsed;
    return {
      inTx(tx, message) {
        // Two arrow messages raced into the same topic: the second one stays a plain message.
        if (activeRelay(room.sql, message.topicId)) return;
        startRelay(room, tx, actor.id, message.topicId, message.id, draftRelay);
      },
    };
  },
});

/** Makes the chain and the relay, and hands step 0 to its member. In the message's transaction. */
export function startRelay(room: RoomContext, tx: RoomTx, startedBy: string, topicId: string, messageId: string, d: { steps: { memberId: string; note?: string }[]; goal: string }, now = Date.now()): Relay {
  const sql = room.sql;
  const chainId = ulid(now);
  sql.exec(
    "INSERT INTO work_chains (id, topic_id, root_kind, root_id, root_bot, requester_id, handoffs, created_at) VALUES (?, ?, 'message', ?, NULL, ?, 0, ?)",
    chainId,
    topicId,
    messageId,
    startedBy,
    now,
  );
  const r: Relay = {
    id: ulid(now),
    topicId,
    messageId,
    chainId,
    startedBy,
    goal: d.goal,
    steps: d.steps.map((s) => ({ memberId: s.memberId, ...(s.note ? { note: s.note } : {}), state: "pending", seq: -1, seqs: [], runIds: [] })),
    current: 0,
    state: "running",
    reason: null,
    stoppedBy: null,
    retries: 0,
    updatedAt: now,
  };
  sql.exec(
    `INSERT INTO topic_relays (id, topic_id, message_id, chain_id, started_by, goal, steps, current, state, retries, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, 0, 'running', 0, ?)`,
    r.id,
    topicId,
    messageId,
    chainId,
    startedBy,
    r.goal,
    JSON.stringify(r.steps),
    now,
  );
  enqueueStep(sql, r, 0, now);
  save(sql, r, now);
  publishWorkState(room, tx, startedBy, topicId);
  room.wakeAt(now);
  return r;
}

// ---------------------------------------------------------------------------
// Handing a step to its member: `relay` outbox rows
// ---------------------------------------------------------------------------

/** What a step's member gets (desk/relay.ts `takeRelay`): written by code, never by a model. */
export interface RelayOrder {
  botId: string;
  relayId: string;
  chainId: string;
  /** The chain step of this try: the run's place in the chain, and the try's identity. */
  seq: number;
  index: number;
  topicId: string;
  /** The person's arrow message. */
  messageId: string;
  startedBy: string;
  goal: string;
  note?: string;
  /** Every step's member, in order. */
  members: string[];
  /** The arrow message as the person wrote it, with its files, quotes and 产出. */
  root: { body: string; ext?: RequestExt };
  /** The step before, when there is one. */
  prev?: {
    memberId: string;
    result: string;
    code: RunCode | null;
    artifacts: { artifactId: string; version: number; title: string; parentVersion?: number }[];
  };
}

/** 产出 versions the runs made (the newest of each), with their parent version. */
function runArtifacts(sql: SqlStorage, runIds: string[]): NonNullable<RelayOrder["prev"]>["artifacts"] {
  if (runIds.length === 0) return [];
  try {
    const rows = sql
      .exec<{ artifact_id: string; version: number; parent_version: number | null; title: string }>(
        `SELECT v.artifact_id, v.version, v.parent_version, a.title FROM artifact_versions v JOIN artifacts a ON a.id = v.artifact_id
         WHERE v.run_id IN (${runIds.map(() => "?").join(", ")}) ORDER BY v.created_at, v.version LIMIT 50`,
        ...runIds,
      )
      .toArray();
    const newest = new Map<string, NonNullable<RelayOrder["prev"]>["artifacts"][number]>();
    for (const x of rows) {
      const parent = x.parent_version ?? (x.version > 1 ? x.version - 1 : null);
      newest.set(x.artifact_id, { artifactId: x.artifact_id, version: x.version, title: x.title, ...(parent !== null ? { parentVersion: parent } : {}) });
    }
    return [...newest.values()].slice(0, 20);
  } catch {
    // A room that never had 产出 has no tables for them.
    return [];
  }
}

function orderOf(sql: SqlStorage, r: Relay, index: number): RelayOrder {
  const st = r.steps[index];
  const msg = sql.exec<{ body: string; ext: string | null }>("SELECT body, ext FROM messages WHERE id = ?", r.messageId).toArray()[0];
  const ext = msg ? requestExt(parseExt(msg.ext)) : undefined;
  const prevStep = index > 0 ? r.steps[index - 1] : undefined;
  return {
    botId: st.memberId,
    relayId: r.id,
    chainId: r.chainId,
    seq: st.seq,
    index,
    topicId: r.topicId,
    messageId: r.messageId,
    startedBy: r.startedBy,
    goal: r.goal,
    ...(st.note ? { note: st.note } : {}),
    members: r.steps.map((s) => s.memberId),
    root: { body: msg?.body ?? "", ...(ext ? { ext } : {}) },
    ...(prevStep
      ? { prev: { memberId: prevStep.memberId, result: prevStep.result ?? "", code: prevStep.code ?? null, artifacts: runArtifacts(sql, prevStep.runIds) } }
      : {}),
  };
}

/**
 * Starts a try of step `index`: a new chain step of kind `relay` (it counts no hand-over; its path
 * is its member, so the relay may come back to a member) and a `relay` row for its member.
 */
function enqueueStep(sql: SqlStorage, r: Relay, index: number, now = Date.now()): void {
  const st = r.steps[index];
  const prev = index > 0 ? r.steps[index - 1] : undefined;
  const seq = (sql.exec<{ n: number | null }>("SELECT max(seq) AS n FROM chain_steps WHERE chain_id = ?", r.chainId).one().n ?? -1) + 1;
  sql.exec(
    `INSERT INTO chain_steps (chain_id, seq, kind, from_bot, to_bot, from_run, run_id, topic_id, depth, path, state, at)
     VALUES (?, ?, 'relay', ?, ?, ?, NULL, ?, 0, ?, 'reserved', ?)`,
    r.chainId,
    seq,
    prev?.memberId ?? null,
    st.memberId,
    prev?.runId ?? null,
    r.topicId,
    JSON.stringify([st.memberId]),
    now,
  );
  st.seq = seq;
  st.seqs = [...st.seqs, seq];
  st.state = "pending";
  delete st.runId;
  delete st.result;
  delete st.code;
  r.current = index;
  queueRow(sql, r, index, now);
}

function queueRow(sql: SqlStorage, r: Relay, index: number, now = Date.now()): void {
  sql.exec("INSERT INTO bot_outbox (kind, topic_id, payload, created_at) VALUES ('relay', ?, ?, ?)", r.topicId, JSON.stringify(orderOf(sql, r, index)), now);
}

/** True while a `relay` row is still the relay's current try (a stop or a retry makes it stale). */
function current(sql: SqlStorage, o: Pick<RelayOrder, "relayId" | "index" | "seq">): Relay | undefined {
  const r = relayById(sql, o.relayId);
  if (!r || r.state !== "running" || r.current !== o.index || r.steps[o.index]?.seq !== o.seq) return undefined;
  return r;
}

/** What `takeRelay` answers: the run, or why the member did not start one. */
export type RelayTaken = { runId: string } | { busy: true } | { unavailable: true };

/**
 * Hands one `relay` row to its member's desk. A stale row is dropped. Busy in the topic: the step
 * waits (`queued`) and starts when that member's run there ends. A member that takes no work now:
 * the relay stops there. Throws when it must be tried again.
 */
export async function sendRelayRow(env: Env, groupId: string, sql: SqlStorage, payload: string): Promise<void> {
  const o = JSON.parse(payload) as RelayOrder;
  if (!current(sql, o)) return;
  const taken = (await deskStub(env, o.botId).takeRelay(o.botId, { ...o, groupId })) as RelayTaken;
  if ("runId" in taken) return;
  // The room records it in a transaction of its own (a hook after commit cannot write events).
  unwrap(await roomStub(env, groupId).rpc(groupId, "bots.relayStepState", null, { relayId: o.relayId, index: o.index, seq: o.seq, state: "busy" in taken ? "queued" : "unavailable" }));
}

/** A `relay` row that could not be handed over in time: the relay stops there (`undelivered`). */
export function giveUpRelayRow(room: RoomContext, log: Logger, payload: string): void {
  const o = JSON.parse(payload) as RelayOrder;
  log.error("relay step dropped", { relayId: o.relayId, index: o.index });
  room.commit((tx) => {
    const r = current(room.sql, o);
    if (!r) return;
    pause(room.sql, r, "undelivered", "failed");
    publishWorkState(room, tx, r.startedBy, r.topicId);
  });
}

function pause(sql: SqlStorage, r: Relay, reason: string, stepState: string, stoppedBy?: string): void {
  r.state = "paused";
  r.reason = reason;
  r.stoppedBy = stoppedBy ?? null;
  r.steps[r.current].state = stepState;
  save(sql, r);
}

/** Internal: the member of a step was busy in the topic (`queued`) or takes no work (`unavailable`). */
registerRoomRpc("bots.relayStepState", (room, actor, args) => {
  const a = args as { relayId: string; index: number; seq: number; state: "queued" | "unavailable" };
  if (actor) throw new ApiError("forbidden", "internal only");
  room.commit((tx) => {
    const r = current(room.sql, a);
    if (!r) return;
    if (a.state === "queued") {
      r.steps[r.current].state = "queued";
      save(room.sql, r);
    } else {
      room.sql.exec("UPDATE chain_steps SET state = 'failed' WHERE chain_id = ? AND seq = ? AND state = 'reserved'", r.chainId, a.seq);
      pause(room.sql, r, "member_unavailable", "failed");
    }
    publishWorkState(room, tx, r.startedBy, r.topicId);
  });
  return { ok: true };
});

// A step waits (`queued`) while its member works on something else in the topic: when that run
// ends, the step is handed over again.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type !== "run.updated" || event.at < Date.now() - LIVE_WINDOW_MS) return;
  const run = (event.data as { run?: BotRun }).run;
  if (!run?.topicId || run.endedAt === undefined || isMainTopicId(run.topicId)) return;
  const r = activeRelay(sql, run.topicId);
  if (!r || r.state !== "running") return;
  const st = r.steps[r.current];
  if (st.state !== "queued" || st.memberId !== run.botId) return;
  st.state = "pending";
  save(sql, r);
  queueRow(sql, r, r.current);
});

// ---------------------------------------------------------------------------
// Moving on: the reports of a step's runs (bots.workState)
// ---------------------------------------------------------------------------

/**
 * The relay step a chain step belongs to: itself (kind `relay`), or the relay step a hand-over
 * (转交) went on from. Null for a split-off part and the steps under it.
 */
function relaySeqOf(sql: SqlStorage, chainId: string, seq: number): number | null {
  let cur: StepRow | undefined = stepRow(sql, chainId, seq);
  for (let i = 0; cur && i <= MAX_DEPTH + 1; i++) {
    if (cur.kind === "relay") return cur.seq;
    if (cur.kind !== "handoff") return null;
    cur = sql
      .exec<StepRow>(
        "SELECT * FROM chain_steps WHERE chain_id = ? AND to_bot = ? AND seq < ? ORDER BY (run_id IS ?) DESC, seq DESC LIMIT 1",
        chainId,
        cur.from_bot,
        cur.seq,
        cur.from_run,
      )
      .toArray()[0] as StepRow | undefined;
  }
  return null;
}

/** Parts a step gave out (分出去) that have not come back. */
function openParts(sql: SqlStorage, chainId: string, seq: number): number {
  return sql.exec<{ n: number }>("SELECT count(*) AS n FROM chain_steps WHERE chain_id = ? AND parent_seq = ? AND state IN ('reserved', 'working')", chainId, seq).one().n;
}

/**
 * A run of a relay step reported (before `workState` writes its row, so the topic's state shows
 * where the relay is). Started: the step works (with the member a hand-over went to; a run a
 * person restarted takes a paused relay up again). Ended: the step is done and the next member is
 * woken, or the relay pauses with a reason. A hand-over waits for the other member's run; parts
 * that have not come back wait for the follow-up run that gets them.
 */
export function relayReport(room: RoomContext, botId: string, s: WorkState): void {
  if (!s.chain || isMainTopicId(s.topicId)) return;
  const sql = room.sql;
  const r = one(sql, "chain_id = ? AND state IN ('running', 'paused')", s.chain.chainId);
  if (!r) return;
  const seq = relaySeqOf(sql, s.chain.chainId, s.chain.seq);
  if (seq === null) return;
  const index = r.steps.findIndex((x) => x.seqs.includes(seq));
  if (index !== r.current) return;
  const st = r.steps[index];
  if (!s.ended) {
    const chain = chainRow(sql, r.chainId);
    if (chain?.stopped_at) {
      // Stopped while it was being handed over: the run stops too.
      sql.exec(
        "INSERT INTO bot_outbox (kind, topic_id, payload, created_at) VALUES ('cancel', ?, ?, ?)",
        s.topicId,
        JSON.stringify({ botId, runId: s.runId, actorId: chain.stopped_by ?? r.startedBy, chainId: r.chainId }),
        Date.now(),
      );
      room.wakeAt(Date.now());
      return;
    }
    if (st.runId === s.runId && st.state === "working" && r.state === "running") return;
    if (r.state === "paused") {
      r.state = "running";
      r.reason = null;
      r.stoppedBy = null;
    }
    st.memberId = botId;
    st.runId = s.runId;
    st.state = "working";
    if (!st.runIds.includes(s.runId)) st.runIds = [...st.runIds, s.runId].slice(-20);
    save(sql, r);
    return;
  }
  if (r.state !== "running") return;
  // A late report of an older run does not end the step's newer run.
  if (st.runId !== undefined && st.runId !== s.runId) return;
  // 转交: the step goes on with the other member's run.
  if (s.handedOver) return;
  // 分出去: the parts come back to a follow-up run, whose end ends the step.
  if (openParts(sql, r.chainId, s.chain.seq) > 0) return;
  st.result = [...(s.result ?? "")].slice(0, MAX_RELAY_RESULT).join("");
  st.code = s.code ?? null;
  if (s.outcome === "cancelled") return pause(sql, r, "step_cancelled", "stopped", s.stoppedBy);
  if (s.outcome === "failed") return pause(sql, r, "step_failed", "failed");
  const ok: TopicStatus[] = ["done", "waiting_review", "open"];
  if (!ok.includes(s.status)) return pause(sql, r, "step_blocked", "failed");
  st.state = "done";
  const next = index + 1;
  if (next >= r.steps.length) {
    r.state = "done";
    r.reason = null;
    save(sql, r);
    return;
  }
  // Code a later member needs and cannot read: only what is pushed goes on to someone else.
  if (s.code && !s.code.pushed && r.steps[next].memberId !== st.memberId) {
    r.state = "paused";
    r.reason = "not_pushed";
    save(sql, r);
    return;
  }
  enqueueStep(sql, r, next);
  save(sql, r);
  room.wakeAt(Date.now());
}

// ---------------------------------------------------------------------------
// People steer it: stop and retry (routes.ts), and stops of the whole chain
// ---------------------------------------------------------------------------

function person(room: RoomContext, actor: Actor | null): string {
  if (!actor || actor.kind !== "human") throw new ApiError("forbidden", "only people stop or retry a relay");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  return actor.id;
}

/** `POST /v1/groups/:id/relays/:relayId/stop`: any person in the group; stops the current step's run. */
registerRoomRpc("bots.relayStop", (room, actor, args): TopicRelay => {
  const by = person(room, actor);
  const { relayId } = args as { relayId: string };
  const r = relayById(room.sql, relayId);
  if (!r) throw new ApiError("not_found", "relay not found");
  if (r.state !== "running" && r.state !== "paused") throw new ApiError("conflict", "the relay has ended");
  room.commit((tx) => {
    // Stopping the chain stops the step's run and what it gave out, and (onChainStop) the relay.
    stopChain(room.sql, r.chainId, by);
    publishWorkState(room, tx, by, r.topicId);
  });
  room.wakeAt(Date.now());
  return toTopicRelay(relayById(room.sql, relayId) as Relay);
});

/** `POST /v1/groups/:id/relays/:relayId/retry`: any person in the group; the paused step again. */
registerRoomRpc("bots.relayRetry", (room, actor, args): TopicRelay => {
  const by = person(room, actor);
  const { relayId } = args as { relayId: string };
  const r = relayById(room.sql, relayId);
  if (!r) throw new ApiError("not_found", "relay not found");
  if (r.state !== "paused") throw new ApiError("conflict", "only a relay that stopped at a step can be retried");
  if (r.retries >= MAX_RELAY_RETRIES) throw new ApiError("conflict", `a relay is retried at most ${MAX_RELAY_RETRIES} times`, "relay.retry_limit");
  room.commit((tx) => {
    r.retries += 1;
    r.state = "running";
    r.reason = null;
    r.stoppedBy = null;
    enqueueStep(room.sql, r, r.current);
    save(room.sql, r);
    publishWorkState(room, tx, by, r.topicId);
  });
  room.wakeAt(Date.now());
  return toTopicRelay(relayById(room.sql, relayId) as Relay);
});

// A chain stops (全部停止, a relay's stop, the requester left): its relay stops with it.
onChainStop((sql, chainId, by, reason) => {
  const r = one(sql, "chain_id = ? AND state IN ('running', 'paused')", chainId);
  if (!r) return [];
  r.state = "stopped";
  r.reason = reason ?? null;
  r.stoppedBy = reason === "requester_left" ? null : by;
  const st = r.steps[r.current];
  if (st.state !== "done") st.state = "stopped";
  save(sql, r);
  return [r.topicId];
});

// ---------------------------------------------------------------------------
// The state card (chat-run topic state)
// ---------------------------------------------------------------------------

/** The relay line of a topic's state card: who does what in which order, and where it is. */
export function relayLine(sql: SqlStorage, topicId: string): { steps: { memberId: string; note?: string; state: string }[]; current: number; state: string; reason?: string } | undefined {
  const r = latestRelay(sql, topicId);
  if (!r) return undefined;
  return {
    steps: r.steps.map((s) => ({ memberId: s.memberId, ...(s.note ? { note: s.note } : {}), state: s.state })),
    current: r.current,
    state: r.state,
    ...(r.reason ? { reason: r.reason } : {}),
  };
}
