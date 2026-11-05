// Per-member state inside one group room: per-topic read positions, follow and mute flags and group
// notification preferences (docs/protocol.md, "Read positions per topic", "Topics: follow",
// "Members and preferences"). All functions run on the room's SQLite.
import type { NotifyLevel, TopicState } from "@chat/protocol";
import type { MigrationStep } from "./migrations";

/** Room migration for the V1 messaging extensions. Old rows keep working: every column has a default. */
export const ROOM_MESSAGING_MIGRATION: MigrationStep = {
  name: "messaging-1",
  up: `
    ALTER TABLE topics ADD COLUMN waiting_on TEXT NOT NULL DEFAULT '[]';
    ALTER TABLE topics ADD COLUMN owner_id TEXT;
    ALTER TABLE topics ADD COLUMN last_message_at INTEGER;
    ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'text';
    UPDATE topics SET owner_id = created_by;
    UPDATE topics SET last_message_at =
      (SELECT max(created_at) FROM messages m WHERE m.topic_id = topics.id AND m.deleted_at IS NULL);
    CREATE TABLE topic_states (
      member_id TEXT NOT NULL,
      topic_id TEXT NOT NULL,
      read_seq INTEGER NOT NULL DEFAULT 0,
      following INTEGER NOT NULL DEFAULT 0,
      PRIMARY KEY (member_id, topic_id)
    );
    CREATE TABLE member_prefs (
      member_id TEXT PRIMARY KEY,
      muted INTEGER NOT NULL DEFAULT 0,
      notify TEXT
    );
  `,
};

/** Room migration: my per-topic mute (topic.mute). Old rows read as 0: not muted. */
export const ROOM_TOPIC_MUTE_MIGRATION: MigrationStep = {
  name: "messaging-2",
  up: `ALTER TABLE topic_states ADD COLUMN muted INTEGER NOT NULL DEFAULT 0;`,
};

export interface ReadTotals {
  readSeq: number;
  unread: number;
  mentions: number;
}

type StateRow = {
  topic_id: string;
  unread: number;
  mentions: number;
  read_seq: number;
  following: number;
  muted: number;
};

function toState(r: StateRow, groupRead: number): TopicState {
  const s: TopicState = {
    topicId: r.topic_id,
    readSeq: Math.max(groupRead, r.read_seq),
    unread: r.unread,
    mentions: r.mentions,
  };
  if (r.following) s.following = true;
  if (r.muted) s.muted = true;
  return s;
}

const UNREAD_CTE = `
  WITH u AS (
    SELECT m.topic_id AS topic_id, count(*) AS unread,
      sum(EXISTS (SELECT 1 FROM mentions mn WHERE mn.member_id = ? AND mn.message_id = m.id)) AS mentions
    FROM messages m LEFT JOIN topic_states ts ON ts.member_id = ? AND ts.topic_id = m.topic_id
    WHERE m.created_seq > ? AND m.created_seq > coalesce(ts.read_seq, 0)
      AND m.author_id != ? AND m.deleted_at IS NULL AND m.kind != 'system'
    GROUP BY m.topic_id
  )`;

/** The group cursor, raised to the seq at which the member joined (older history is read). */
function fromJoin(sql: SqlStorage, memberId: string, groupRead: number): number {
  const joined =
    sql.exec<{ joined_seq: number }>("SELECT joined_seq FROM room_members WHERE member_id = ?", memberId).toArray()[0]
      ?.joined_seq ?? 0;
  return Math.max(groupRead, joined);
}

/**
 * Every topic with unread > 0, mentions > 0, following or muted, plus the group totals.
 * `groupRead` is the member's group cursor, already cut to lastSeq.
 */
export function groupReadState(
  sql: SqlStorage,
  memberId: string,
  cursor: number,
): ReadTotals & { topics: TopicState[] } {
  const groupRead = fromJoin(sql, memberId, cursor);
  const rows = sql
    .exec<StateRow>(
      `${UNREAD_CTE}
       SELECT t.id AS topic_id, coalesce(u.unread, 0) AS unread, coalesce(u.mentions, 0) AS mentions,
              coalesce(ts.read_seq, 0) AS read_seq, coalesce(ts.following, 0) AS following,
              coalesce(ts.muted, 0) AS muted
       FROM topics t LEFT JOIN u ON u.topic_id = t.id
       LEFT JOIN topic_states ts ON ts.member_id = ? AND ts.topic_id = t.id
       WHERE u.topic_id IS NOT NULL OR ts.following = 1 OR ts.muted = 1
       ORDER BY t.last_seq DESC`,
      memberId,
      memberId,
      groupRead,
      memberId,
      memberId,
    )
    .toArray() as StateRow[];
  const topics = rows.map((r) => toState(r, groupRead));
  let unread = 0;
  let mentions = 0;
  for (const t of topics) {
    unread += t.unread;
    mentions += t.mentions;
  }
  return { readSeq: groupRead, unread, mentions, topics };
}

/** One topic's state for this member (also when it has nothing unread and is not followed). */
export function topicState(sql: SqlStorage, memberId: string, topicId: string, cursor: number): TopicState {
  const groupRead = fromJoin(sql, memberId, cursor);
  const r = sql
    .exec<StateRow>(
      `${UNREAD_CTE}
       SELECT t.id AS topic_id, coalesce(u.unread, 0) AS unread, coalesce(u.mentions, 0) AS mentions,
              coalesce(ts.read_seq, 0) AS read_seq, coalesce(ts.following, 0) AS following,
              coalesce(ts.muted, 0) AS muted
       FROM topics t LEFT JOIN u ON u.topic_id = t.id
       LEFT JOIN topic_states ts ON ts.member_id = ? AND ts.topic_id = t.id
       WHERE t.id = ?`,
      memberId,
      memberId,
      groupRead,
      memberId,
      memberId,
      topicId,
    )
    .toArray()[0] as StateRow | undefined;
  return toState(r ?? { topic_id: topicId, unread: 0, mentions: 0, read_seq: 0, following: 0, muted: 0 }, groupRead);
}

export function setFollowing(sql: SqlStorage, memberId: string, topicId: string, following: boolean): void {
  sql.exec(
    `INSERT INTO topic_states (member_id, topic_id, following) VALUES (?, ?, ?)
     ON CONFLICT (member_id, topic_id) DO UPDATE SET following = excluded.following`,
    memberId,
    topicId,
    following ? 1 : 0,
  );
}

export function isFollowing(sql: SqlStorage, memberId: string, topicId: string): boolean {
  return (
    sql
      .exec("SELECT 1 FROM topic_states WHERE member_id = ? AND topic_id = ? AND following = 1", memberId, topicId)
      .toArray().length > 0
  );
}

export function setTopicMuted(sql: SqlStorage, memberId: string, topicId: string, muted: boolean): void {
  sql.exec(
    `INSERT INTO topic_states (member_id, topic_id, muted) VALUES (?, ?, ?)
     ON CONFLICT (member_id, topic_id) DO UPDATE SET muted = excluded.muted`,
    memberId,
    topicId,
    muted ? 1 : 0,
  );
}

export function isTopicMuted(sql: SqlStorage, memberId: string, topicId: string): boolean {
  return (
    sql
      .exec("SELECT 1 FROM topic_states WHERE member_id = ? AND topic_id = ? AND muted = 1", memberId, topicId)
      .toArray().length > 0
  );
}

/** Moves one topic's read position forward. Returns true when it moved. */
export function advanceTopicRead(sql: SqlStorage, memberId: string, topicId: string, seq: number): boolean {
  const prev =
    sql
      .exec<{ read_seq: number }>(
        "SELECT read_seq FROM topic_states WHERE member_id = ? AND topic_id = ?",
        memberId,
        topicId,
      )
      .toArray()[0]?.read_seq ?? 0;
  if (seq <= prev) return false;
  sql.exec(
    `INSERT INTO topic_states (member_id, topic_id, read_seq) VALUES (?, ?, ?)
     ON CONFLICT (member_id, topic_id) DO UPDATE SET read_seq = max(read_seq, excluded.read_seq)`,
    memberId,
    topicId,
    seq,
  );
  return true;
}

export interface Prefs {
  muted: boolean;
  notify: NotifyLevel;
}

export function defaultNotify(kind: "group" | "dm" | null): NotifyLevel {
  return kind === "dm" ? "all" : "mentions";
}

export function getPrefs(sql: SqlStorage, memberId: string, kind: "group" | "dm" | null): Prefs {
  const r = sql
    .exec<{ muted: number; notify: string | null }>("SELECT muted, notify FROM member_prefs WHERE member_id = ?", memberId)
    .toArray()[0] as { muted: number; notify: string | null } | undefined;
  return { muted: r?.muted === 1, notify: (r?.notify as NotifyLevel | null) ?? defaultNotify(kind) };
}

export function setPrefs(sql: SqlStorage, memberId: string, change: { muted?: boolean; notify?: NotifyLevel }): void {
  sql.exec("INSERT OR IGNORE INTO member_prefs (member_id) VALUES (?)", memberId);
  if (change.muted !== undefined) sql.exec("UPDATE member_prefs SET muted = ? WHERE member_id = ?", change.muted ? 1 : 0, memberId);
  if (change.notify !== undefined) sql.exec("UPDATE member_prefs SET notify = ? WHERE member_id = ?", change.notify, memberId);
}
