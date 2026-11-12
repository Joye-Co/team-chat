// AI topic titles: a topic started without a title gets one from the team's model, written
// after the first message is in. The first line stays the title until then, and a title someone
// set in the meantime is never replaced. Needs the team's AI 服务 (src/aiservice, set by an owner
// in 设置); without it topics keep the first line.
import { MAX_TITLE_CHARS, isMainTopicId } from "@chat/protocol";
import { type CallOptions, type ModelConfig, callModel } from "../aiservice/providers";
import { tracker, useConfig } from "../aiservice/uses";
import type { Env } from "../core/env";
import { registerAfterCommit, registerRoomRpc } from "../core/registry";
import { roomStub } from "../core/env";
import { defaultTitle, startTitle } from "../core/room";

const APPLY_RPC = "autotitle.apply";

export interface TitleInput {
  /** The first message of the topic. */
  body: string;
  /** The 群聊 message the topic was opened from (开个话题), if any. */
  quote?: string;
}

const SYSTEM = [
  "You name topics in a team chat. A topic is one piece of work or one discussion.",
  "Reply with the title only: no quotes, no trailing punctuation, no explanation.",
  "Write it in the language of the messages. Chinese: at most 16 characters. Other languages: at most 8 words.",
  "Say what the topic is about or what has to be done, not who said it.",
].join("\n");

/** The text the model reads: the quoted message, then the first message. */
export function titlePrompt(input: TitleInput): string {
  const parts: string[] = [];
  if (input.quote) parts.push(`The topic was opened from this chat message:\n${input.quote}`);
  if (input.body.trim()) parts.push(`First message of the topic:\n${input.body}`);
  return parts.join("\n\n");
}

/** A title from model text: one line, no wrapping quotes or end punctuation, within the limit. */
export function cleanTitle(text: string): string | null {
  const line = text.split(/\r?\n/).map((l) => l.trim()).find((l) => l !== "") ?? "";
  let t = line.replace(/^(标题|title)\s*[:：]\s*/i, "");
  let before = "";
  while (before !== t) {
    before = t;
    t = t
      .replace(/^["'“”‘’「」『』《》]+|["'“”‘’「」『』《》]+$/g, "")
      .replace(/[。.!！?？,，;；:：]+$/u, "")
      .trim();
  }
  if (!t) return null;
  return [...t].slice(0, MAX_TITLE_CHARS).join("");
}

/** Asks the team's model for a title; null when it is not set up or the call fails or refuses. */
export async function generateTitle(env: Env, input: TitleInput, cfg: ModelConfig | null, opts: CallOptions = {}): Promise<string | null> {
  // Tests: a body with the AUTOTITLE_FAKE marker gets a made-up title, no network.
  if (env.AUTOTITLE_FAKE && `${input.body}${input.quote ?? ""}`.includes(env.AUTOTITLE_FAKE)) return cleanTitle(`AI ${defaultTitle(input.quote ?? input.body)}`);
  if (!cfg) return null;
  const result = await callModel(cfg, { system: SYSTEM, prompt: titlePrompt(input), maxTokens: 1024 }, opts);
  if (!result.ok) throw new Error(result.error);
  return cleanTitle(result.text);
}

registerAfterCommit(async (event, h) => {
  if (event.type !== "topic.created") return;
  const topic = (event.data as { topic: { id: string; title: string; main?: boolean; rootMessageId: string } }).topic;
  if (topic.main || isMainTopicId(topic.id)) return;
  const root = h.sql
    .exec<{ body: string; ext: string | null }>("SELECT body, ext FROM messages WHERE id = ?", topic.rootMessageId)
    .toArray()[0];
  if (!root) return;
  const ext = root.ext ? (JSON.parse(root.ext) as { replyTo?: { excerpt?: string } }) : {};
  // A title given at creation stays: only the first-line default is replaced.
  if (topic.title !== startTitle(root.body, ext.replyTo?.excerpt)) return;
  const cfg: ModelConfig | null = await useConfig(h.env, "title", h.log);
  if (!cfg && !h.env.AUTOTITLE_FAKE) return;
  let title: string | null = null;
  try {
    title = await generateTitle(h.env, { body: root.body, quote: ext.replyTo?.excerpt }, cfg, cfg ? { track: tracker(h.env, "title", cfg, h.log) } : {});
  } catch (err) {
    h.log.warn("topic title failed", { topicId: topic.id, err });
    return;
  }
  if (!title || title === topic.title) return;
  await roomStub(h.env, h.groupId).rpc(h.groupId, APPLY_RPC, null, { topicId: topic.id, from: topic.title, title });
});

/** Sets the AI title when the topic still has the title it was made with. */
registerRoomRpc(APPLY_RPC, (room, actor, args) => {
  if (actor) throw new Error("internal only");
  const { topicId, from, title } = args as { topicId: string; from: string; title: string };
  return room.commit((tx) => {
    const row = room.sql
      .exec<{ title: string; created_by: string }>("SELECT title, created_by FROM topics WHERE id = ?", topicId)
      .toArray()[0];
    if (!row || row.title !== from) return { applied: false };
    const seq = tx.nextSeq();
    room.sql.exec("UPDATE topics SET title = ?, last_seq = ? WHERE id = ?", title, seq, topicId);
    tx.append({ type: "topic.updated", actorId: row.created_by, topicId, data: { topicId, title } });
    return { applied: true };
  });
});
