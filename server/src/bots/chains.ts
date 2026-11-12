// Usable (design-relay R2): a piece of work (chain) is one person's request and every hand-over
// (转交) and split-off (分出去) that came out of it. The room counts them in one place: depth,
// hand-overs by AI members, no cycles, stopped or not. A hand-over is two steps: `bots.chainStep`
// reserves a step, `bots.chainStepResult` confirms it once the other member took the work (or
// drops it when it refused: then neither the counts nor the routing owner change). When the run
// of a split-off step ends, its result goes back to the member that gave it out through the bots
// outbox (`part`); a person stopping a run, or a whole chain, stops the runs under it (`cancel`).
// AI members never start each other's runs by posting messages: only these steps and people do.
import { type RunStatus, isMainTopicId } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { type Actor, type RoomContext, type RoomTx, registerEventWriter, registerRoomMigration, registerRoomRpc } from "../core/registry";
import { deskStub } from "./env";
import { claimTopic, publishWorkState, routingOwner } from "./participants";
import type { Env } from "../core/env";
import type { Logger } from "../core/logger";

/** How deep hand-overs go: the root is 0, A → B → C → D is 3. */
export const MAX_DEPTH = 3;
/** Hand-overs and split-offs AI members start in one chain (retries and relay steps do not count). */
export const MAX_HANDOFFS = 8;
/** A relay (按顺序做, R3) has at most this many steps, retried at most this often. */
export const MAX_RELAY_STEPS = 6;
export const MAX_RELAY_RETRIES = 3;
/** The longest result a step keeps. */
export const MAX_STEP_RESULT = 4000;

/**
 * Room migration `chains-1`. Beyond the design's columns: `root_bot` (the member of the root
 * run) and the root as step 0 (kind `root`), so every run of a chain is a step with a path;
 * `parent_seq`: the step a split-off's result goes back to (its run may change on a retry).
 */
export const CHAINS_MIGRATION = `
  CREATE TABLE work_chains (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL,
    root_kind TEXT NOT NULL,
    root_id TEXT NOT NULL,
    root_bot TEXT,
    requester_id TEXT NOT NULL,
    handoffs INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    stopped_at INTEGER,
    stopped_by TEXT
  );
  CREATE INDEX work_chains_requester ON work_chains(requester_id, stopped_at);
  CREATE TABLE chain_steps (
    chain_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    from_bot TEXT,
    to_bot TEXT NOT NULL,
    from_run TEXT,
    run_id TEXT,
    topic_id TEXT NOT NULL,
    depth INTEGER NOT NULL,
    path TEXT NOT NULL,
    parent_run TEXT,
    parent_seq INTEGER,
    owner_before TEXT,
    state TEXT NOT NULL,
    result TEXT,
    at INTEGER NOT NULL,
    PRIMARY KEY (chain_id, seq)
  );
  CREATE INDEX chain_steps_run ON chain_steps(run_id);
  CREATE INDEX chain_steps_parent ON chain_steps(chain_id, parent_seq, state);
  CREATE INDEX chain_steps_topic ON chain_steps(topic_id);
`;

registerRoomMigration({ name: "chains-1", up: CHAINS_MIGRATION });

/** `relay`: a step of a relay (按顺序做, relay/engine.ts): depth 0, its path is its member. */
export type StepKind = "root" | "handoff" | "assign" | "relay";
/** Step states: reserved (not taken yet), working, then done | failed | cancelled | handed_over. */
const OPEN_STATES = "('reserved', 'working')";

export type ChainRow = {
  id: string;
  topic_id: string;
  root_kind: string;
  root_id: string;
  root_bot: string | null;
  requester_id: string;
  handoffs: number;
  created_at: number;
  stopped_at: number | null;
  stopped_by: string | null;
};

export type StepRow = {
  chain_id: string;
  seq: number;
  kind: StepKind;
  from_bot: string | null;
  to_bot: string;
  from_run: string | null;
  run_id: string | null;
  topic_id: string;
  depth: number;
  path: string;
  parent_run: string | null;
  parent_seq: number | null;
  /** 转交: the topic's routing owner when the step was reserved (a refusal puts it back). */
  owner_before: string | null;
  state: string;
  result: string | null;
  at: number;
};

/** A run's place in a chain, as its desk keeps it (meta `chain:<runId>`). */
export interface ChainRef {
  chainId: string;
  seq: number;
}
export interface ChainPos extends ChainRef {
  depth: number;
  path: string[];
}

/** A `part` outbox row: a split-off's result for the member that gave it out. */
export interface PartRow {
  botId: string;
  chainId: string;
  seq: number;
  parentSeq: number;
  /** The giver's run when it gave the part out, or a newer run of that step. */
  parentRun: string | null;
  /** The giver's topic. */
  topicId: string;
  /** The topic the part was done in. */
  partTopicId: string;
  fromBot: string;
  requesterId: string;
  /** done | failed | cancelled */
  state: string;
  result: string;
  stoppedBy?: string;
  /** Every part that step gave out has ended. */
  last: boolean;
  /** The run of the part's step that ended: a retried part comes back again with its new run. */
  runId?: string;
  /** The member that gave the part out, when it handed its work over since: `botId` continues the work. */
  givenBy?: string;
  /** Only the giver's split-off card is updated: the work went on with another member. */
  cardOnly?: boolean;
}

/** A `cancel` outbox row: stops a run for a person. */
export interface CancelRow {
  botId: string;
  runId: string;
  actorId: string;
  chainId: string;
}

export function chainRow(sql: SqlStorage, chainId: string): ChainRow | undefined {
  return sql.exec<ChainRow>("SELECT * FROM work_chains WHERE id = ?", chainId).toArray()[0] as ChainRow | undefined;
}

export function stepRow(sql: SqlStorage, chainId: string, seq: number): StepRow | undefined {
  return sql.exec<StepRow>("SELECT * FROM chain_steps WHERE chain_id = ? AND seq = ?", chainId, seq).toArray()[0] as StepRow | undefined;
}

function enqueue(sql: SqlStorage, kind: "part" | "cancel", topicId: string, payload: PartRow | CancelRow): void {
  sql.exec("INSERT INTO bot_outbox (kind, topic_id, payload, created_at) VALUES (?, ?, ?, ?)", kind, topicId, JSON.stringify(payload), Date.now());
}

function cut(s: string, n: number): string {
  const cs = [...s];
  return cs.length <= n ? s : cs.slice(0, n).join("");
}

// ---------------------------------------------------------------------------
// Two-step hand-over
// ---------------------------------------------------------------------------

/** What `bots.chainStep` gets from a desk. */
export interface ChainStepIn {
  fromRun: string;
  /** The from-run's step; absent: the run starts a new chain as its root. */
  chain?: ChainRef;
  requesterId: string;
  kind: "handoff" | "assign";
  to: string;
  topicId: string;
}
export interface ChainStepOut extends ChainPos {
  /** Set when this call made the chain: the from-run's own place (step 0). */
  root?: ChainPos;
}

/**
 * Reserves a hand-over step (the first of two). Refuses, in this order: a stopped chain
 * (`delegation.stopped`), too many hand-overs (`delegation.too_many`), too deep
 * (`delegation.too_deep`), back to a member on the path (`delegation.cycle`).
 */
export function reserveStep(sql: SqlStorage, botId: string, a: ChainStepIn, now = Date.now()): ChainStepOut {
  let chainId = a.chain?.chainId;
  let root: ChainPos | undefined;
  if (!chainId) {
    chainId = ulid(now);
    sql.exec(
      "INSERT INTO work_chains (id, topic_id, root_kind, root_id, root_bot, requester_id, handoffs, created_at) VALUES (?, ?, 'run', ?, ?, ?, 0, ?)",
      chainId,
      a.topicId,
      a.fromRun,
      botId,
      a.requesterId,
      now,
    );
    sql.exec(
      `INSERT INTO chain_steps (chain_id, seq, kind, from_bot, to_bot, from_run, run_id, topic_id, depth, path, state, at)
       VALUES (?, 0, 'root', NULL, ?, NULL, ?, ?, 0, ?, 'working', ?)`,
      chainId,
      botId,
      a.fromRun,
      a.topicId,
      JSON.stringify([botId]),
      now,
    );
    root = { chainId, seq: 0, depth: 0, path: [botId] };
  }
  const chain = chainRow(sql, chainId);
  if (!chain) throw new ApiError("not_found", "chain not found");
  const from = stepRow(sql, chainId, a.chain?.seq ?? 0);
  if (!from || from.to_bot !== botId) throw new ApiError("forbidden", "not your step of this chain");
  if (chain.stopped_at !== null) throw new ApiError("conflict", "a person stopped this piece of work", "delegation.stopped");
  if (chain.handoffs >= MAX_HANDOFFS) throw new ApiError("conflict", `one piece of work is handed over or split at most ${MAX_HANDOFFS} times`, "delegation.too_many");
  if (from.depth >= MAX_DEPTH) throw new ApiError("conflict", `hand-overs go at most ${MAX_DEPTH} deep`, "delegation.too_deep");
  const path = JSON.parse(from.path) as string[];
  if (path.includes(a.to)) throw new ApiError("conflict", "that member is already on this piece of work's path; do not hand work back", "delegation.cycle");
  const seq = (sql.exec<{ n: number | null }>("SELECT max(seq) AS n FROM chain_steps WHERE chain_id = ?", chainId).one().n ?? 0) + 1;
  // A split-off's result goes back to the step that gave it out; a hand-over inherits where the
  // step it continues reports to.
  const parentSeq = a.kind === "assign" ? from.seq : from.parent_seq;
  const parentRun = a.kind === "assign" ? a.fromRun : from.parent_run;
  sql.exec(
    `INSERT INTO chain_steps (chain_id, seq, kind, from_bot, to_bot, from_run, run_id, topic_id, depth, path, parent_run, parent_seq, owner_before, state, at)
     VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, 'reserved', ?)`,
    chainId,
    seq,
    a.kind,
    botId,
    a.to,
    a.fromRun,
    a.topicId,
    from.depth + 1,
    JSON.stringify([...path, a.to]),
    parentRun,
    parentSeq,
    a.kind === "handoff" ? routingOwner(sql, a.topicId) : null,
    now,
  );
  sql.exec("UPDATE work_chains SET handoffs = handoffs + 1 WHERE id = ?", chainId);
  return { chainId, seq, depth: from.depth + 1, path: [...path, a.to], ...(root ? { root } : {}) };
}

/**
 * The second step: the other member took the work (`runId`), or refused (null: the step goes and
 * the count with it). A taken hand-over makes the other member the routing owner; a refused one
 * leaves the routing owner as it was. A step taken after the chain was stopped is stopped too.
 */
export function confirmStep(room: RoomContext, tx: RoomTx, botId: string, a: { chainId: string; seq: number; runId: string | null; topicId?: string }): void {
  const sql = room.sql;
  const step = stepRow(sql, a.chainId, a.seq);
  if (!step || step.from_bot !== botId) throw new ApiError("not_found", "step not found");
  if (a.runId === null) {
    if (step.state !== "reserved") return;
    sql.exec("DELETE FROM chain_steps WHERE chain_id = ? AND seq = ?", a.chainId, a.seq);
    sql.exec("UPDATE work_chains SET handoffs = max(0, handoffs - 1) WHERE id = ?", a.chainId);
    // The other member's start may have claimed the topic before it refused (a race): the
    // routing owner goes back to what it was.
    if (step.kind === "handoff" && step.owner_before && step.owner_before !== step.to_bot && routingOwner(sql, step.topic_id) === step.to_bot) {
      claimTopic(room, tx, step.owner_before, step.topic_id);
    }
    return;
  }
  // The receiving run's first report may come first: it only names the run, the step stays
  // reserved until here (and a run that already ended keeps how it ended).
  if (step.state !== "reserved" && step.run_id !== a.runId) return;
  const open = step.state === "reserved" || step.state === "working";
  const topicId = a.topicId ?? step.topic_id;
  sql.exec(
    "UPDATE chain_steps SET state = CASE WHEN state = 'reserved' THEN 'working' ELSE state END, run_id = ?, topic_id = ? WHERE chain_id = ? AND seq = ?",
    a.runId,
    topicId,
    a.chainId,
    a.seq,
  );
  if (!open) return;
  if (step.kind === "handoff") claimTopic(room, tx, step.to_bot, topicId);
  const chain = chainRow(sql, a.chainId);
  if (chain?.stopped_at) {
    enqueue(sql, "cancel", topicId, { botId: step.to_bot, runId: a.runId, actorId: chain.stopped_by ?? chain.requester_id, chainId: a.chainId });
    room.wakeAt(Date.now());
  }
}

// ---------------------------------------------------------------------------
// Reports of a step's run (from bots.workState)
// ---------------------------------------------------------------------------

/** What a desk adds to `bots.workState` for the chain (design-relay R2). */
export interface StepReport {
  runId: string;
  ended: boolean;
  chain?: ChainRef;
  /** How the run ended: done | failed | cancelled. */
  outcome?: RunStatus;
  /** The person who stopped the run. */
  stoppedBy?: string;
  /** The member the run handed the work over to: the step goes on there. */
  handedOver?: string;
  /** The run's result (<= 4000). */
  result?: string;
}

/**
 * A step's run started or ended. Ended: the step keeps its outcome and result; a split-off's
 * result goes back to its giver (`part`); a run a person stopped stops the parts it gave out.
 * Nothing goes back on a stopped chain: the giver was stopped too.
 */
export function stepReport(room: RoomContext, botId: string, s: StepReport): void {
  if (!s.chain) return;
  const sql = room.sql;
  const step = stepRow(sql, s.chain.chainId, s.chain.seq);
  if (!step || step.to_bot !== botId) return;
  if (!s.ended) {
    // Not taken yet: `confirmStep` moves the step on (its topic, the routing owner, a stop
    // that came in between); the run is only named here. A relay step has no confirmation: its
    // run's first report starts it.
    if (step.state === "reserved" && step.kind !== "relay") {
      sql.exec("UPDATE chain_steps SET run_id = ? WHERE chain_id = ? AND seq = ?", s.runId, step.chain_id, step.seq);
      return;
    }
    // A retry, restart or follow-up of an ended step is the same step working again.
    const again = (step.state === "done" || step.state === "failed" || step.state === "cancelled") && step.run_id !== s.runId;
    // (A run a person starts again after a stop is that person's choice: it is not stopped here.)
    if (step.state === "working" || step.state === "reserved" || step.kind === "root" || again) {
      sql.exec("UPDATE chain_steps SET run_id = ?, state = 'working' WHERE chain_id = ? AND seq = ?", s.runId, step.chain_id, step.seq);
    }
    return;
  }
  // A late report of an older run does not end the step's newer run.
  if (step.run_id !== null && step.run_id !== s.runId && step.state === "working") return;
  if (s.handedOver) {
    sql.exec("UPDATE chain_steps SET state = 'handed_over', run_id = ? WHERE chain_id = ? AND seq = ?", s.runId, step.chain_id, step.seq);
    return;
  }
  const state = s.outcome === "cancelled" ? "cancelled" : s.outcome === "failed" ? "failed" : "done";
  const result = cut(s.result ?? "", MAX_STEP_RESULT);
  sql.exec("UPDATE chain_steps SET state = ?, result = ?, run_id = ? WHERE chain_id = ? AND seq = ?", state, result, s.runId, step.chain_id, step.seq);
  const chain = chainRow(sql, step.chain_id);
  if (!chain || chain.stopped_at !== null) return;
  let queued = false;
  if (s.stoppedBy) {
    // A person stopped this run: the parts it gave out do not come back to wake it.
    const parts = sql
      .exec<StepRow>(`SELECT * FROM chain_steps WHERE chain_id = ? AND parent_seq = ? AND state IN ${OPEN_STATES} AND run_id IS NOT NULL`, step.chain_id, step.seq)
      .toArray() as StepRow[];
    for (const p of parts) {
      enqueue(sql, "cancel", p.topic_id, { botId: p.to_bot, runId: p.run_id as string, actorId: s.stoppedBy, chainId: p.chain_id });
      queued = true;
    }
  }
  if (step.parent_seq !== null) {
    const giver = stepRow(sql, step.chain_id, step.parent_seq);
    // A giver that handed its work over since is not woken: the part goes to the member that
    // continued the work (its card in the giver's topic is still updated).
    const parent = giver ? continuer(sql, giver) : undefined;
    // A giver a person stopped is not woken by what comes back.
    if (giver && parent && parent.state !== "cancelled" && parent.state !== "handed_over") {
      const open = sql
        .exec<{ n: number }>(`SELECT count(*) AS n FROM chain_steps WHERE chain_id = ? AND parent_seq = ? AND state IN ${OPEN_STATES}`, step.chain_id, step.parent_seq)
        .one().n;
      const part: PartRow = {
        botId: parent.to_bot,
        chainId: step.chain_id,
        seq: step.seq,
        parentSeq: parent.seq,
        parentRun: parent.run_id ?? step.parent_run,
        topicId: parent.topic_id,
        partTopicId: step.topic_id,
        fromBot: botId,
        requesterId: chain.requester_id,
        state,
        result,
        ...(s.stoppedBy ? { stoppedBy: s.stoppedBy } : {}),
        last: open === 0,
        runId: s.runId,
        ...(parent.seq !== giver.seq ? { givenBy: giver.to_bot } : {}),
      };
      enqueue(sql, "part", parent.topic_id, part);
      if (parent.seq !== giver.seq) enqueue(sql, "part", giver.topic_id, { ...part, botId: giver.to_bot, parentSeq: giver.seq, parentRun: giver.run_id, topicId: giver.topic_id, cardOnly: true });
      queued = true;
    }
  }
  if (queued) room.wakeAt(Date.now());
}

/**
 * The step that carries on a giver's work: the giver itself, or (it handed the work over) the
 * hand-over step its run started, followed along later hand-overs.
 */
function continuer(sql: SqlStorage, giver: StepRow): StepRow | undefined {
  let cur: StepRow | undefined = giver;
  for (let i = 0; cur && cur.state === "handed_over" && i <= MAX_DEPTH + 1; i++) {
    cur = sql
      .exec<StepRow>("SELECT * FROM chain_steps WHERE chain_id = ? AND kind = 'handoff' AND from_bot = ? AND from_run = ? ORDER BY seq DESC LIMIT 1", cur.chain_id, cur.to_bot, cur.run_id)
      .toArray()[0] as StepRow | undefined;
  }
  return cur;
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

/**
 * design-relay R3: what else stops with a chain (a relay, relay/engine.ts), in the same
 * transaction. Returns the topics whose state it changed (published by the caller when it can).
 */
export type ChainStopHook = (sql: SqlStorage, chainId: string, by: string, reason?: string) => string[];
const chainStopHooks: ChainStopHook[] = [];
export function onChainStop(h: ChainStopHook): void {
  chainStopHooks.push(h);
}

/**
 * Marks the chain stopped and queues a cancel for each of its runs that has not ended. `touched`
 * collects the topics whose relay stopped with it; `reason` is why (a relay's `requester_left`).
 */
export function stopChain(sql: SqlStorage, chainId: string, by: string, now = Date.now(), touched?: Set<string>, reason?: string): number {
  sql.exec("UPDATE work_chains SET stopped_at = coalesce(stopped_at, ?), stopped_by = coalesce(stopped_by, ?) WHERE id = ?", now, by, chainId);
  const open = sql.exec<StepRow>("SELECT * FROM chain_steps WHERE chain_id = ? AND state = 'working' AND run_id IS NOT NULL", chainId).toArray() as StepRow[];
  for (const s of open) enqueue(sql, "cancel", s.topic_id, { botId: s.to_bot, runId: s.run_id as string, actorId: by, chainId });
  for (const h of chainStopHooks) for (const t of h(sql, chainId, by, reason)) touched?.add(t);
  return open.length;
}

/** Publishes the topics a stop changed (their relay). */
function publishStopped(room: RoomContext, actorId: string, touched: Set<string>): void {
  if (touched.size === 0) return;
  room.commit((tx) => {
    for (const t of touched) publishWorkState(room, tx, actorId, t);
  });
}

/** The person a stop is for: a person in the group, or (a card press through a desk) the one who pressed. */
async function stopper(room: RoomContext, actor: Actor | null, byActorId: unknown): Promise<string> {
  if (actor?.kind === "human") {
    if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
    return actor.id;
  }
  if (typeof byActorId !== "string" || !room.isMember(byActorId)) throw new ApiError("forbidden", "only a person in this group stops a piece of work");
  // Checked again here: a desk passes on who pressed, the room decides.
  const who = await room.env.DB.prepare("SELECT kind, deactivated FROM members WHERE id = ?").bind(byActorId).first<{ kind: string; deactivated: number }>();
  if (!who || who.kind !== "human" || who.deactivated) throw new ApiError("forbidden", "only a person in this group stops a piece of work");
  return byActorId;
}

registerRoomRpc("bots.chainStep", (room, actor, args): ChainStepOut => {
  const a = args as ChainStepIn;
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  if (!room.isMember(a.to)) throw new ApiError("forbidden", "that member is not in this group");
  if (a.kind !== "handoff" && a.kind !== "assign") throw new ApiError("invalid", "kind is handoff or assign");
  if (a.to === actor.id) throw new ApiError("invalid", "that is you");
  return reserveStep(room.sql, actor.id, a);
});

registerRoomRpc("bots.chainStepResult", (room, actor, args) => {
  const a = args as { chainId: string; seq: number; runId: string | null; topicId?: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  room.commit((tx) => confirmStep(room, tx, actor.id, a));
  return { ok: true };
});

/** `POST /v1/groups/:id/chains/:chainId/stop` and the card's 全部停止: any person in the group. */
registerRoomRpc("bots.chainStop", async (room, actor, args) => {
  const { chainId, byActorId } = args as { chainId: string; byActorId?: string };
  if (actor && actor.kind !== "human" && actor.kind !== "bot") throw new ApiError("forbidden", "people only");
  const by = actor ? await stopper(room, actor, byActorId) : String(byActorId ?? "");
  if (!by) throw new ApiError("forbidden", "people only");
  if (!chainRow(room.sql, chainId)) throw new ApiError("not_found", "piece of work not found");
  const touched = new Set<string>();
  const stoppedRuns = stopChain(room.sql, chainId, by, Date.now(), touched);
  publishStopped(room, by, touched);
  room.wakeAt(Date.now());
  return { stoppedRuns };
});

/** Stops every chain a person asked for (he was deactivated or left the group). */
export function requesterLeft(sql: SqlStorage, memberId: string, touched?: Set<string>): number {
  const chains = sql.exec<{ id: string }>("SELECT id FROM work_chains WHERE requester_id = ? AND stopped_at IS NULL", memberId).toArray();
  let n = 0;
  for (const c of chains) n += stopChain(sql, c.id, memberId, Date.now(), touched, "requester_left");
  return n;
}

/** The requester was deactivated: internal only. */
registerRoomRpc("bots.requesterLeft", (room, actor, args) => {
  const { memberId } = args as { memberId: string };
  if (actor) throw new ApiError("forbidden", "internal only");
  const touched = new Set<string>();
  const n = requesterLeft(room.sql, memberId, touched);
  publishStopped(room, memberId, touched);
  if (n > 0) room.wakeAt(Date.now());
  return { stoppedRuns: n };
});

// The requester left the group: his chains stop (the cancels go after commit).
registerEventWriter((event, sql) => {
  if (event.type !== "group.memberRemoved") return;
  const { memberId } = event.data as { memberId?: string };
  if (memberId) requesterLeft(sql, memberId);
});

// ---------------------------------------------------------------------------
// Outbox rows: part and cancel
// ---------------------------------------------------------------------------

/** Hands one `part` or `cancel` row to its desk. Throws when it must be tried again. */
export async function sendChainRow(env: Env, groupId: string, kind: string, payload: string): Promise<void> {
  if (kind === "cancel") {
    const c = JSON.parse(payload) as CancelRow;
    // An ended or unknown run is stopped already: nothing to retry.
    await deskStub(env, c.botId).cancel(c.botId, c.runId, c.actorId);
    return;
  }
  const p = JSON.parse(payload) as PartRow;
  await deskStub(env, p.botId).partDone(p.botId, { ...p, groupId });
}

/**
 * A `part` row that could not be handed over in time: the giver's topic is stuck and waits for the
 * requester (the part's result is kept on its step). A `cancel` is only logged.
 */
export function giveUpChainRow(room: RoomContext, log: Logger, kind: string, payload: string): void {
  if (kind !== "part") {
    log.error("chain cancel dropped", { kind });
    return;
  }
  const p = JSON.parse(payload) as PartRow;
  log.error("chain part dropped", { chainId: p.chainId, seq: p.seq });
  if (isMainTopicId(p.topicId)) return;
  room.commit((tx) => room.updateTopic(tx, p.botId, p.topicId, { status: "blocked", waitingOn: [p.requesterId] }));
}

// ---------------------------------------------------------------------------
// The state card's 交接 section
// ---------------------------------------------------------------------------

export interface HandoverLine {
  kind: StepKind;
  from: string | null;
  to: string;
  state: string;
  topicId: string;
  at: number;
}

/** Hand-overs and split-offs of the chains that touch this topic, oldest first (at most 10). */
export function topicHandovers(sql: SqlStorage, topicId: string): HandoverLine[] {
  return sql
    .exec<StepRow>(
      `SELECT * FROM chain_steps WHERE kind != 'root' AND chain_id IN (SELECT DISTINCT chain_id FROM chain_steps WHERE topic_id = ?)
       ORDER BY at, seq LIMIT 10`,
      topicId,
    )
    .toArray()
    .map((s) => ({ kind: s.kind as StepKind, from: s.from_bot, to: s.to_bot, state: s.state, topicId: s.topic_id, at: s.at }));
}
