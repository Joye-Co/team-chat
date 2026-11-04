// One-time backfills of rooms written before the cards2 tables existed (1.1 development builds
// on staging): private cards (cards2-5), the topic base of open blocking asks (cards2-6) and the
// group decisions of answers stored before cards2-1. New code reads old rows; these steps make
// the old rows complete. Each step is idempotent (INSERT OR IGNORE / ON CONFLICT).
import type { CardV2 } from "@chat/protocol";
import type { ArchivedEvent } from "../core/registry";
import { registerRoomMigration } from "../core/registry";
import { recordAnswerDecision } from "./decisions";
import { markPrivate } from "./privacy";

type EventRow = { seq: number; type: string; at: number; actor_id: string; topic_id: string | null; message_id: string | null; data: string };

/** Every rev of every card in the log, by message id and rev. */
function cardRevs(sql: SqlStorage): Map<string, Map<number, CardV2>> {
  const out = new Map<string, Map<number, CardV2>>();
  const rows = sql
    .exec<{ message_id: string | null; type: string; data: string }>(
      `SELECT message_id, type, data FROM events
       WHERE type IN ('message.created', 'message.updated') AND data LIKE '%"cardV2"%' ORDER BY seq`,
    )
    .toArray();
  for (const r of rows) {
    const d = JSON.parse(r.data) as { message?: { id?: string; cardV2?: CardV2 }; messageId?: string; cardV2?: CardV2 };
    const id = r.type === "message.created" ? d.message?.id : d.messageId;
    const card = r.type === "message.created" ? d.message?.cardV2 : d.cardV2;
    if (!id || !card) continue;
    let revs = out.get(id);
    if (!revs) out.set(id, (revs = new Map()));
    revs.set(card.rev, card);
  }
  return out;
}

registerRoomMigration({
  name: "cards2-8-backfill",
  up: (sql) => {
    const revs = cardRevs(sql);
    // A card that was private at any rev stays private.
    for (const [id, byRev] of revs) for (const card of byRev.values()) markPrivate(sql, id, card);
    // A topic that waits on an open blocking ask but has no base comes back to in_progress.
    sql.exec(
      `INSERT OR IGNORE INTO card_topic_base (topic_id, status, waiting_on)
       SELECT t.id, 'in_progress', '[]' FROM topics t
       WHERE t.status = 'waiting_answer' AND EXISTS (
         SELECT 1 FROM messages m WHERE m.topic_id = t.id AND m.deleted_at IS NULL
           AND json_extract(m.ext, '$.cardState.ask.status') = 'open'
           AND json_extract(m.ext, '$.cardState.ask.blocking') = 1)`,
    );
    // Group decisions of answers stored before card_decisions existed.
    const answers = sql
      .exec<EventRow>("SELECT * FROM events WHERE type = 'card.answered' AND seq NOT IN (SELECT seq FROM card_decisions) ORDER BY seq")
      .toArray();
    for (const r of answers) {
      const data = JSON.parse(r.data) as { messageId?: string; rev?: number };
      if (!data.messageId || data.rev === undefined) continue;
      const msg = sql
        .exec<{ topic_id: string; deleted_at: number | null }>("SELECT topic_id, deleted_at FROM messages WHERE id = ?", data.messageId)
        .toArray()[0];
      const card = revs.get(data.messageId)?.get(data.rev);
      if (!msg || msg.deleted_at !== null || !card) continue;
      const event: ArchivedEvent = { seq: r.seq, type: r.type, at: r.at, actorId: r.actor_id, topicId: r.topic_id, messageId: r.message_id, data };
      recordAnswerDecision(sql, event, { card, topicId: msg.topic_id });
    }
  },
});
