// 复盘 (agent-harness.zh.md 7.3): when a run of a person's request ends, one forced tool call
// writes at most 3 lessons with evidence and when-to-apply, pending in the member's scope until
// the daily consolidation, readable only in the group (or direct conversation) of the run.
// Best effort: it never fails or delays the run.
import type { RunStatus } from "@chat/protocol";
import { z } from "zod";
import { tracker, useConfig } from "../aiservice/uses";
import type { CallOptions } from "../aiservice/providers";
import { type ToolSpec, callTool } from "../aiservice/tools";
import type { Env } from "../core/env";
import { runReadableSql } from "./access";
import { insertStatements, type MemoryRow } from "./store";

export const MAX_LESSONS = 3;
/** Characters of each part of the run the review reads. */
const PART_CHARS = 6000;

const Lessons = z.object({
  lessons: z
    .array(z.object({ text: z.string().min(1), evidence: z.string().optional(), when: z.string().optional() }))
    .max(10),
});

export const RETRO_TOOL: ToolSpec = {
  name: "remember_lessons",
  description: "Record what is worth remembering from this piece of work. An empty list is a good answer.",
  schema: {
    type: "object",
    properties: {
      lessons: {
        type: "array",
        maxItems: MAX_LESSONS,
        items: {
          type: "object",
          properties: {
            text: { type: "string", description: "The lesson as one short instruction to yourself, in the language of the request. At most 300 characters." },
            evidence: { type: "string", description: "What in the request, the progress or the result shows it. At most 300 characters." },
            when: { type: "string", description: "When the lesson applies. At most 150 characters." },
          },
          required: ["text", "evidence", "when"],
        },
      },
    },
    required: ["lessons"],
  },
};

export interface RetroInput {
  botId: string;
  botName?: string;
  runId: string;
  /** Where the lessons come from: the run's group, or the other group it read (`unknown` for two or more). */
  groupId: string;
  /** The groups the run may read (`readableGroups`): lessons known there are not repeated. */
  groups: string[];
  status: RunStatus;
  requesterId: string;
  request: string;
  brief?: string;
  progress?: string;
  result: string;
}

function cut(text: string, max: number): string {
  const cps = [...text];
  return cps.length <= max ? text : `${cps.slice(0, max - 1).join("")}…`;
}

function lastPart(text: string, max: number): string {
  const cps = [...text];
  return cps.length <= max ? text : `…${cps.slice(cps.length - max + 1).join("")}`;
}

/** Writes the run's lessons as pending member memories; returns how many. */
export async function retroAfterRun(env: Env, i: RetroInput, opts: CallOptions = {}): Promise<number> {
  // Sources are people's messages and run results: a bot's or an integration's request gets no review.
  const requester = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(i.requesterId).first<{ kind: string }>();
  if (requester?.kind !== "human") return 0;
  const cfg = await useConfig(env, "retro");
  if (!cfg) return 0;
  const known = (
    await env.DB.prepare(`SELECT text FROM memories m WHERE scope = ?1 AND status IN ('active', 'pending') AND ${runReadableSql("m", 2)} ORDER BY updated_at DESC LIMIT 30`)
      .bind(`member:${i.botId}`, JSON.stringify(i.groups))
      .all<Pick<MemoryRow, "text">>()
  ).results;
  const system = [
    `You review a finished piece of work of ${i.botName ?? "an AI member"} of a team chat, to help it do better next time.`,
    `Write at most ${MAX_LESSONS} lessons with the \`remember_lessons\` tool: things that will matter again (how this team or repository works, what went wrong and how to avoid it, what the requester cared about).`,
    "Only lessons the request, the progress or the result show. Each with its evidence and when it applies. No one-off details, no secrets, no lessons already known. An empty list is the usual answer when the work went as expected.",
  ].join("\n");
  const prompt = [
    `Run status: ${i.status}`,
    "",
    "Request (from a person):",
    cut(i.request, PART_CHARS),
    ...(i.brief ? ["", cut(i.brief, 2000)] : []),
    ...(i.progress ? ["", "Progress, the end of it:", lastPart(i.progress, PART_CHARS)] : []),
    "",
    "Result:",
    cut(i.result, PART_CHARS),
    ...(known.length > 0 ? ["", "Lessons already known (do not repeat them):", ...known.map((k) => `- ${k.text}`)] : []),
  ].join("\n");
  const res = await callTool(cfg, { system, prompt, tool: RETRO_TOOL, maxTokens: 1200 }, { track: tracker(env, "retro", cfg), ...opts });
  if (!res.ok) return 0;
  const parsed = Lessons.safeParse(res.input);
  if (!parsed.success) return 0;
  const seen = new Set(known.map((k) => k.text.trim()));
  const now = Date.now();
  const stmts: D1PreparedStatement[] = [];
  let n = 0;
  for (const l of parsed.data.lessons.slice(0, MAX_LESSONS)) {
    if (l.text.trim() === "" || seen.has(l.text.trim())) continue;
    const { stmts: s } = insertStatements(
      env.DB,
      {
        scope: `member:${i.botId}`,
        kind: "lesson",
        text: l.text,
        when: l.when ?? null,
        note: l.evidence ?? null,
        source: { kind: "run", ids: [i.runId] },
        authorId: i.botId,
        human: false,
        status: "pending",
        confidence: 0.5,
        fromGroup: i.groupId,
      },
      i.botId,
      now + n,
    );
    stmts.push(...s);
    n++;
  }
  if (stmts.length > 0) await env.DB.batch(stmts);
  return n;
}
