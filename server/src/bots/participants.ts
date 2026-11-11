// Usable (design-relay R1): the AI members that work in a topic. Several AI members may work in
// one topic at once or one after the other; `topic_bots` keeps one row per (topic, bot) with its
// latest run, whether it still works and how its latest run ended. The topic's status is rolled
// up from these rows, and `Topic.participants` lists them. `bot_topics` (and `topics.owner_id`)
// keep only the routing owner: the AI member that most recently started a run in the topic, which
// gets a follow-up without an @.
import { type ArchivedEvent, type RoomContext, type RoomTx, registerTopicFields } from "../core/registry";
import { approvalDecision, approvesWholeWork } from "./dispatch";
import { type BotRun, isMainTopicId, MAX_TOPIC_PARTICIPANTS, RUN_KIND_ASIDE, type TopicParticipant, type TopicStatus } from "@chat/protocol";

/** Room migration `participants-1` (registered after `bots-1` in room.ts, which it reads). */
export const PARTICIPANTS_MIGRATION = `
  CREATE TABLE topic_bots (
    topic_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    run_id TEXT,
    working INTEGER NOT NULL DEFAULT 0,
    report TEXT,
    waiting_on TEXT NOT NULL DEFAULT '[]',
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    PRIMARY KEY (topic_id, bot_id)
  );
  INSERT OR IGNORE INTO topic_bots (topic_id, bot_id, started_at, ended_at)
    SELECT topic_id, bot_id, claimed_at, claimed_at FROM bot_topics;
`;

export type TopicBotRow = {
  topic_id: string;
  bot_id: string;
  run_id: string | null;
  working: number;
  report: string | null;
  waiting_on: string;
  started_at: number;
  ended_at: number | null;
};

/** Events older than this are imports (restore from the archive): their runs are not working. */
const LIVE_WINDOW_MS = 10 * 60_000;

export function topicBots(sql: SqlStorage, topicId: string): TopicBotRow[] {
  return sql.exec<TopicBotRow>("SELECT * FROM topic_bots WHERE topic_id = ? ORDER BY started_at, bot_id", topicId).toArray() as TopicBotRow[];
}

export function isParticipant(sql: SqlStorage, topicId: string, botId: string): boolean {
  return sql.exec("SELECT 1 FROM topic_bots WHERE topic_id = ? AND bot_id = ?", topicId, botId).toArray().length > 0;
}

/** The routing owner: the AI member that most recently started a run in the topic. */
export function routingOwner(sql: SqlStorage, topicId: string): string | null {
  return sql.exec<{ bot_id: string }>("SELECT bot_id FROM bot_topics WHERE topic_id = ?", topicId).toArray()[0]?.bot_id ?? null;
}

/**
 * 接 1.6 with several members: the participants a message without an @ is addressed to. A quote
 * of a participant's message goes to that participant; a typed approval answer goes to the
 * working participants that wait for this person's answer. Empty: the routing owner gets it.
 */
export function addressedParticipants(sql: SqlStorage, topicId: string, authorId: string, body: string, replyToAuthorId?: string): string[] {
  if (isMainTopicId(topicId)) return [];
  if (replyToAuthorId && replyToAuthorId !== authorId && isParticipant(sql, topicId, replyToAuthorId)) return [replyToAuthorId];
  if (approvalDecision(body) === null && !approvesWholeWork(body)) return [];
  return topicBots(sql, topicId)
    .filter((r) => r.working === 1 && r.report === "waiting_answer" && (JSON.parse(r.waiting_on || "[]") as string[]).includes(authorId))
    .map((r) => r.bot_id);
}

/** `Topic.participants`: the newest `MAX_TOPIC_PARTICIPANTS` rows, oldest first. */
export function participants(sql: SqlStorage, topicId: string): TopicParticipant[] {
  const rows = topicBots(sql, topicId);
  return rows.slice(-MAX_TOPIC_PARTICIPANTS).map((r) => ({ memberId: r.bot_id, working: r.working === 1, lastRunAt: r.started_at }));
}

registerTopicFields((sql, topicId) => {
  if (isMainTopicId(topicId)) return {};
  const list = participants(sql, topicId);
  return list.length > 0 ? { participants: list } : {};
});

/**
 * Makes `botId` the topic's routing owner (`bot_topics` and `topics.owner_id`), appending
 * `topic.updated` with the new `ownerId` when it changes. Not in the 群聊.
 */
export function claimTopic(room: RoomContext, tx: RoomTx, botId: string, topicId: string): void {
  if (isMainTopicId(topicId)) return;
  const now = Date.now();
  room.sql.exec(
    `INSERT INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)
     ON CONFLICT (topic_id) DO UPDATE SET bot_id = excluded.bot_id, claimed_at = excluded.claimed_at`,
    topicId,
    botId,
    now,
  );
  const owner = room.sql.exec<{ owner_id: string | null }>("SELECT owner_id FROM topics WHERE id = ?", topicId).toArray()[0];
  if (owner && owner.owner_id !== botId) room.updateTopic(tx, botId, topicId, { ownerId: botId });
}

/** Statuses of ended rows, most urgent first (a failed run reports `blocked`). */
const ENDED_ORDER: TopicStatus[] = ["waiting_answer", "waiting_review", "blocked", "open", "done"];

/**
 * The topic's status from its rows (design-relay 3.4): a working row makes it in progress (or
 * waiting for an answer when that row waits for one); otherwise only the rows that ended after
 * the newest run started count, the most urgent of them wins and their waitingOn are joined.
 * Null: no row reports anything (rows from before participants-1).
 */
export function rollUp(rows: TopicBotRow[]): { status: TopicStatus; waitingOn: string[] } | null {
  const waiting = (xs: TopicBotRow[]) => [...new Set(xs.flatMap((r) => JSON.parse(r.waiting_on || "[]") as string[]))];
  const working = rows.filter((r) => r.working === 1);
  if (working.length > 0) {
    // A working run that waits for an answer or is stuck (盯活) still shows; otherwise it works.
    for (const status of ["waiting_answer", "waiting_review", "blocked"] as const) {
      const at = working.filter((r) => r.report === status);
      if (at.length > 0) return { status, waitingOn: waiting(at) };
    }
    return { status: "in_progress", waitingOn: [] };
  }
  const latest = Math.max(...rows.map((r) => r.started_at));
  const current = rows.filter((r) => r.report !== null && r.ended_at !== null && r.ended_at >= latest);
  for (const status of ENDED_ORDER) {
    const at = current.filter((r) => r.report === status);
    if (at.length > 0) return { status, waitingOn: status === "done" || status === "open" ? [] : waiting(at) };
  }
  return null;
}

/** Writes the rolled-up status and the participants as one `topic.updated`. */
export function publishWorkState(room: RoomContext, tx: RoomTx, actorId: string, topicId: string): void {
  const rows = topicBots(room.sql, topicId);
  if (rows.length === 0) return;
  const next = rollUp(rows);
  room.updateTopic(tx, actorId, topicId, { ...(next ?? {}), extra: { participants: participants(room.sql, topicId) } });
}

/** What `bots.workState` gets from a desk: the run's latest report. */
export interface WorkState {
  topicId: string;
  runId: string;
  status: TopicStatus;
  waitingOn: string[];
  /** The run has ended (its last report). */
  ended: boolean;
}

/** Stores one run's report on its row and publishes the topic's state. */
export function workState(room: RoomContext, tx: RoomTx, botId: string, s: WorkState): void {
  if (isMainTopicId(s.topicId)) return;
  const now = Date.now();
  const row = room.sql.exec<TopicBotRow>("SELECT * FROM topic_bots WHERE topic_id = ? AND bot_id = ?", s.topicId, botId).toArray()[0] as TopicBotRow | undefined;
  const waitingOn = JSON.stringify([...new Set(s.waitingOn)]);
  if (s.ended) {
    // A late report of an earlier run does not end the bot's newer run.
    if (row && row.run_id !== null && row.run_id !== s.runId && row.working === 1) return;
    room.sql.exec(
      `INSERT INTO topic_bots (topic_id, bot_id, run_id, working, report, waiting_on, started_at, ended_at) VALUES (?, ?, ?, 0, ?, ?, ?, ?)
       ON CONFLICT (topic_id, bot_id) DO UPDATE SET run_id = excluded.run_id, working = 0, report = excluded.report,
         waiting_on = excluded.waiting_on,
         ended_at = CASE WHEN topic_bots.run_id = excluded.run_id THEN coalesce(topic_bots.ended_at, excluded.ended_at) ELSE excluded.ended_at END`,
      s.topicId,
      botId,
      s.runId,
      s.status,
      waitingOn,
      now,
      now,
    );
  } else if (!row || row.run_id !== s.runId) {
    startRow(room.sql, s.topicId, botId, s.runId, now, s.status, waitingOn);
  } else {
    room.sql.exec("UPDATE topic_bots SET working = 1, report = ?, waiting_on = ? WHERE topic_id = ? AND bot_id = ?", s.status, waitingOn, s.topicId, botId);
  }
  publishWorkState(room, tx, botId, s.topicId);
}

function startRow(sql: SqlStorage, topicId: string, botId: string, runId: string, at: number, report: string | null, waitingOn = "[]"): void {
  sql.exec(
    `INSERT INTO topic_bots (topic_id, bot_id, run_id, working, report, waiting_on, started_at, ended_at) VALUES (?, ?, ?, 1, ?, ?, ?, NULL)
     ON CONFLICT (topic_id, bot_id) DO UPDATE SET run_id = excluded.run_id, working = 1, report = excluded.report,
       waiting_on = excluded.waiting_on, started_at = excluded.started_at, ended_at = NULL`,
    topicId,
    botId,
    runId,
    report,
    waitingOn,
    at,
  );
}

/**
 * Event writer for `run.updated` (live and imported): a new run makes its bot a working
 * participant, an ended run stops it working. This keeps rows right when a desk never reports
 * (it failed, or the machine lost the run); it appends no event, the desk's report does.
 */
export function onRunUpdated(event: ArchivedEvent, sql: SqlStorage): void {
  const run = (event.data as { run?: BotRun }).run;
  if (!run?.topicId || !run.botId || isMainTopicId(run.topicId) || run.kind === RUN_KIND_ASIDE) return;
  const row = sql.exec<TopicBotRow>("SELECT * FROM topic_bots WHERE topic_id = ? AND bot_id = ?", run.topicId, run.botId).toArray()[0] as TopicBotRow | undefined;
  const ended = run.endedAt !== undefined;
  const live = event.at >= Date.now() - LIVE_WINDOW_MS;
  if (!ended && live) {
    if (!row || row.run_id !== run.id) startRow(sql, run.topicId, run.botId, run.id, event.at, null);
    return;
  }
  const at = run.endedAt ?? event.at;
  if (!row) {
    sql.exec(
      "INSERT INTO topic_bots (topic_id, bot_id, run_id, working, started_at, ended_at) VALUES (?, ?, ?, 0, ?, ?)",
      run.topicId,
      run.botId,
      run.id,
      run.startedAt ?? run.createdAt,
      at,
    );
    return;
  }
  if (row.run_id === run.id || row.run_id === null || (!live && run.createdAt >= row.started_at)) {
    sql.exec(
      "UPDATE topic_bots SET run_id = ?, working = 0, ended_at = coalesce(ended_at, ?), started_at = max(started_at, ?) WHERE topic_id = ? AND bot_id = ?",
      run.id,
      at,
      row.run_id === run.id ? row.started_at : (run.startedAt ?? run.createdAt),
      run.topicId,
      run.botId,
    );
  }
}
