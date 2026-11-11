// Rolling topic summaries (滚动摘要, docs/design/agent-harness.zh.md 5.5): a long topic keeps one
// summary that the team's model updates as messages come in. Runs and the front desk read it in
// place of the messages their budget leaves out (docs/protocol.md, "Run context"). Needs the AI
// 服务; without it there is no summary and prompts say how many messages were left out, as before.
import { type ModelConfig, callModel } from "../aiservice/providers";
import { loadModelConfig } from "../aiservice/store";
import { type ContextRow, HISTORY_BODY_CHARS, renderMessage } from "../context/render";
import { teamZone } from "../memory/consolidate";
import type { Env } from "../core/env";
import { registerAfterCommit, registerRoomMigration } from "../core/registry";
import { contextRows } from "../bots/room";

/** A topic gets a summary once it has this many messages (shorter ones fit the budget). */
export const SUMMARY_FROM = 60;
/** The summary is updated after this many new messages. */
export const SUMMARY_EVERY = 40;
/** Characters of new messages one update reads. */
export const SUMMARY_INPUT_CHARS = 24_000;
export const MAX_SUMMARY_CHARS = 2000;
/** An update that has not finished after this long may be tried again. */
const IN_FLIGHT_MS = 5 * 60_000;

registerRoomMigration({
  name: "summary-1",
  up: `
    CREATE TABLE topic_summaries (
      topic_id TEXT PRIMARY KEY,
      text TEXT NOT NULL DEFAULT '',
      upto_seq INTEGER NOT NULL DEFAULT 0,
      updated_at INTEGER NOT NULL DEFAULT 0,
      running_since INTEGER
    );
  `,
});

export interface TopicSummary {
  text: string;
  uptoSeq: number;
}

/** The topic's summary, or undefined. Room side. */
export function readSummary(sql: SqlStorage, topicId: string): TopicSummary | undefined {
  const r = sql.exec<{ text: string; upto_seq: number }>("SELECT text, upto_seq FROM topic_summaries WHERE topic_id = ? AND text != ''", topicId).toArray()[0];
  return r ? { text: r.text, uptoSeq: r.upto_seq } : undefined;
}

const SYSTEM = [
  "You keep the running summary of one topic of a team chat, for an AI member who joins the work later.",
  "Update the summary with the new messages. Keep: what is asked and by whom, decisions and their reasons, constraints, open questions, who does what, and ids worth reading again (m … for messages, f … for files, 产出 a … for outputs).",
  "Drop chit-chat and anything later messages replaced. Write in the language of the topic, as short plain lines, at most 2000 characters. Reply with the summary only.",
].join("\n");

export function summaryPrompt(previous: string, lines: string): string {
  return [`Summary so far:\n${previous || "(none yet)"}`, `New messages, oldest first:\n${lines}`].join("\n\n");
}

/** Asks the model for the next summary; null when the call fails. */
export async function nextSummary(cfg: ModelConfig, previous: string, lines: string): Promise<string | null> {
  const res = await callModel(cfg, { system: SYSTEM, prompt: summaryPrompt(previous, lines), maxTokens: 1500 });
  if (!res.ok) return null;
  const text = res.text.trim();
  return text === "" ? null : [...text].slice(0, MAX_SUMMARY_CHARS).join("");
}

async function names(env: Env, ids: string[]): Promise<(id: string) => string> {
  const uniq = [...new Set(ids)];
  if (uniq.length === 0) return (id) => id;
  const rows = await env.DB.prepare(`SELECT id, name FROM members WHERE id IN (${uniq.map(() => "?").join(",")})`)
    .bind(...uniq)
    .all<{ id: string; name: string }>();
  const m = new Map(rows.results.map((r) => [r.id, r.name]));
  return (id) => m.get(id) ?? id;
}

registerAfterCommit(async (event, h) => {
  if (event.type !== "message.created") return;
  const topicId = (event.data as { message?: { topicId?: string } }).message?.topicId;
  if (!topicId) return;
  const total = h.sql.exec<{ n: number }>("SELECT count(*) AS n FROM messages WHERE topic_id = ? AND deleted_at IS NULL", topicId).one().n;
  if (total < SUMMARY_FROM) return;
  const now = Date.now();
  const row = h.sql
    .exec<{ upto_seq: number; text: string; running_since: number | null }>("SELECT upto_seq, text, running_since FROM topic_summaries WHERE topic_id = ?", topicId)
    .toArray()[0];
  const upto = row?.upto_seq ?? 0;
  const fresh = h.sql
    .exec<{ n: number }>("SELECT count(*) AS n FROM messages WHERE topic_id = ? AND deleted_at IS NULL AND created_seq > ?", topicId, upto)
    .one().n;
  // The first summary covers the start of the topic; later ones every SUMMARY_EVERY messages.
  if (row && fresh < SUMMARY_EVERY) return;
  if (row?.running_since && now - row.running_since < IN_FLIGHT_MS) return;
  let cfg: ModelConfig | null = null;
  try {
    cfg = await loadModelConfig(h.env);
  } catch (err) {
    h.log.warn("ai service unreadable", { err });
  }
  if (!cfg) return;
  h.sql.exec(
    "INSERT INTO topic_summaries (topic_id, running_since) VALUES (?, ?) ON CONFLICT (topic_id) DO UPDATE SET running_since = excluded.running_since",
    topicId,
    now,
  );
  try {
    // The new messages, oldest first, within the input budget (the oldest first: the summary grows in order).
    const rows: ContextRow[] = contextRows(h.sql, "", topicId, "AND created_seq > ?", [upto], "ASC", 400);
    const name = await names(h.env, rows.map((r) => r.authorId));
    const parts: string[] = [];
    let used = 0;
    let last = upto;
    for (const r of rows) {
      const line = renderMessage(r, name, Math.min(1200, HISTORY_BODY_CHARS), "", teamZone(h.env));
      if (parts.length > 0 && used + line.length > SUMMARY_INPUT_CHARS) break;
      parts.push(line);
      used += line.length + 1;
      last = r.seq;
    }
    const text = await nextSummary(cfg, row?.text ?? "", parts.join("\n"));
    if (text === null) {
      h.sql.exec("UPDATE topic_summaries SET running_since = NULL WHERE topic_id = ?", topicId);
      return;
    }
    h.sql.exec("UPDATE topic_summaries SET text = ?, upto_seq = ?, updated_at = ?, running_since = NULL WHERE topic_id = ?", text, last, Date.now(), topicId);
  } catch (err) {
    h.log.warn("topic summary failed", { topicId, err });
    h.sql.exec("UPDATE topic_summaries SET running_since = NULL WHERE topic_id = ?", topicId);
  }
});
