// The front desk (前台, docs/design/agent-harness.zh.md 3.1-3.2): before a new request becomes a
// run on a machine, one model call decides how to handle it: answer at once, hand a work order
// (交代单) to the executor, ask one question, or let it be. Rules come first and need no model;
// anything uncertain falls back to today's behavior (a run).
import { z } from "zod";
import type { ModelConfig } from "../aiservice/providers";
import { type ToolSpec, callTool } from "../aiservice/tools";
import type { CallOptions } from "../aiservice/providers";

/** Below this the front desk does not trust its own answer: the request goes to the executor. */
export const MIN_CONFIDENCE = 0.6;
/** Topic characters the front desk reads (the executor gets more). */
export const FRONT_DESK_CHARS = 12_000;
export const MAX_REPLY_CHARS = 4000;

export const Brief = z.object({
  goal: z.string().min(1).max(1000),
  doneWhen: z.array(z.string().min(1).max(300)).max(10).default([]),
  notes: z.string().max(2000).optional(),
});
export type Brief = z.infer<typeof Brief>;

export const Decision = z.object({
  action: z.enum(["reply", "dispatch", "ask", "skip"]),
  reason: z.string().max(500).default(""),
  confidence: z.number().min(0).max(1).default(0),
  reply: z.string().max(MAX_REPLY_CHARS).optional(),
  brief: Brief.optional(),
  /** 接 3.2, dispatch in a DM: the request is new work, so the executor starts a fresh session. */
  newTask: z.boolean().optional(),
});
export type Decision = z.infer<typeof Decision>;

export const DECIDE_TOOL: ToolSpec = {
  name: "decide",
  description: "How this member handles the new request.",
  schema: {
    type: "object",
    properties: {
      action: {
        type: "string",
        enum: ["reply", "dispatch", "ask", "skip"],
        description:
          "reply: answer now in `reply`. dispatch: the executor does the work on a computer; fill `brief`. ask: one short question in `reply` because the request cannot be started as it is. skip: nothing for this member to do.",
      },
      reason: { type: "string", description: "One sentence: why this action." },
      confidence: { type: "number", minimum: 0, maximum: 1, description: "How sure you are that this action is right." },
      reply: { type: "string", description: "For reply and ask: the message posted in the topic, in the language of the request." },
      brief: {
        type: "object",
        description: "For dispatch: the work order for the executor.",
        properties: {
          goal: { type: "string", description: "What to achieve, in one or two sentences, with the requester's intent." },
          doneWhen: { type: "array", items: { type: "string" }, description: "Checks that show the work is done (tests pass, PR open, file delivered…)." },
          notes: { type: "string", description: "What the executor must know: constraints, decisions, message ids (m …) and file ids (f …) worth reading." },
        },
        required: ["goal"],
      },
      newTask: {
        type: "boolean",
        description:
          "For dispatch in a direct conversation: true when the request is new work unrelated to the earlier work in this conversation (the executor starts a fresh session); false when it continues that work.",
      },
    },
    required: ["action", "reason", "confidence"],
  },
};

export interface DecideInput {
  botName: string;
  role?: string | null;
  title: string;
  /** Topic lines (context/render.ts), oldest first. */
  context: string;
  requester: string;
  request: string;
  /** The topic is a DM with this member. */
  dm: boolean;
  /** 接 1.4: the message does not @ this member; it came from the requester after its last answer. */
  followUp?: boolean;
  /** 沉淀: the memory index (memory/inject.ts), reference material in the fixed prefix. */
  memory?: string;
}

export function systemPrompt(i: Pick<DecideInput, "botName" | "role" | "memory">): string {
  return [
    `You are the front desk of ${i.botName}${i.role ? ` (${i.role})` : ""}, an AI member of a team chat. ${i.botName} works on the team's computers through a coding agent (the executor) that can read and change the code repository, run commands, use the web and deliver files.`,
    "A new request just came in. Decide how to handle it with the `decide` tool:",
    "- reply: conversation, thanks, or a question you can answer well from the topic and general knowledge alone. Never claim work was done that the topic does not show. Never invent facts about the code, files or systems.",
    "- dispatch: anything that needs the repository, files, commands, the web, a change, a check or a deliverable. Write the brief so the executor can start without rereading the whole topic.",
    "- ask: the request cannot be started as it is (what or where is unclear) and the topic does not answer it. Ask one short question.",
    "- skip: nothing for this member to do (the message is for someone else, or needs no answer).",
    "When unsure between reply and dispatch, dispatch. Write replies short, in the language of the request, as the member (not as a front desk).",
    ...(i.memory ? ["", i.memory] : []),
  ].join("\n");
}

export function userPrompt(i: DecideInput): string {
  return [
    `Topic: "${i.title}"${i.dm ? " (a direct conversation with you)" : ""}`,
    "The topic, oldest first:",
    i.context === "" ? "(empty)" : i.context,
    "",
    `New request from ${i.requester}:`,
    i.request,
    ...(i.followUp
      ? ["", "(This message does not @ you. It came in this topic after your last answer. Skip it when it is small talk or meant for someone else; handle it only when it continues the work with you.)"]
      : []),
  ].join("\n");
}

/** A decision the desk can act on: invalid or unsure answers become a dispatch. */
export function settle(raw: unknown): { decision: Decision; fallback?: string } {
  const parsed = Decision.safeParse(raw);
  if (!parsed.success) return { decision: { action: "dispatch", reason: "", confidence: 0 }, fallback: "invalid decision" };
  const d = parsed.data;
  if ((d.action === "reply" || d.action === "ask") && !d.reply?.trim()) {
    return { decision: { ...d, action: "dispatch" }, fallback: "no reply text" };
  }
  if (d.action !== "dispatch" && d.confidence < MIN_CONFIDENCE) {
    return { decision: { ...d, action: "dispatch" }, fallback: "low confidence" };
  }
  return { decision: d };
}

export interface DecideResult {
  decision: Decision;
  /** What the model read, kept for replay (M6). */
  system: string;
  prompt: string;
  /** Why the desk did not take the model's answer as it was. */
  fallback?: string;
  ms: number;
  usage?: { inputTokens: number; outputTokens: number };
}

export async function decide(cfg: ModelConfig, input: DecideInput, opts: CallOptions = {}): Promise<DecideResult> {
  const t0 = Date.now();
  const system = systemPrompt(input);
  const prompt = userPrompt(input);
  const res = await callTool(cfg, { system, prompt, tool: DECIDE_TOOL, maxTokens: 1500 }, opts);
  const ms = Date.now() - t0;
  if (!res.ok) return { decision: { action: "dispatch", reason: "", confidence: 0 }, fallback: `model error: ${res.error}`, ms, system, prompt };
  const s = settle(res.input);
  return { ...s, ms, system, prompt, ...(res.usage ? { usage: res.usage } : {}) };
}

/**
 * 接 3.2: words that start a new piece of work in a DM's chat area (without a front desk), at the
 * start of the message: the next request there otherwise continues the member's last session.
 */
const NEW_TASK = /^(?:新任务|新的任务|另一个任务|换个任务|开个新任务|重新开始|从头开始|new task\b)/iu;

export function isNewTask(body: string): boolean {
  return NEW_TASK.test(body.replace(/@\S+/g, "").trim());
}

/** The work order as the executor's prompt shows it. */
export function briefText(b: Brief): string {
  const lines = ["Work order from your front desk (it read the topic and the request):", `Goal: ${b.goal}`];
  if (b.doneWhen.length > 0) lines.push("Done when:", ...b.doneWhen.map((x) => `- ${x}`));
  if (b.notes) lines.push(`Notes: ${b.notes}`);
  return lines.join("\n");
}
