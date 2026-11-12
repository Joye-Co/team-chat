// One forced tool call to the team's provider: the model must answer through `tool`, so the
// answer has a fixed shape (the front desk's decision, docs/design/agent-harness.zh.md 3.1).
// Anthropic: Messages tool use with `tool_choice`. DeepSeek and OpenAI-compatible: function
// calling. A provider that answers with text instead gets one chance: a JSON object in the text.
// Retries, the prompt cache mark and `track` work as in providers.ts.
import type Anthropic from "@anthropic-ai/sdk";
import {
  type CallOptions,
  type CallUsage,
  type ModelConfig,
  anthropicClient,
  anthropicFailure,
  anthropicUsage,
  cachedSystem,
  completionUsage,
  postCompletions,
  short,
} from "./providers";
import { withRetry } from "./retry";

const TIMEOUT_MS = 30_000;

export interface ToolSpec {
  name: string;
  description: string;
  /** JSON Schema of the tool's input (an object). */
  schema: Record<string, unknown>;
}

export interface ToolRequest {
  system: string;
  prompt: string;
  tool: ToolSpec;
  maxTokens: number;
}

export type ToolResult = { ok: true; input: unknown; usage?: CallUsage } | { ok: false; error: string };

/** The first JSON object in a text (a fenced block or a bare object), or undefined. */
export function jsonIn(text: string): unknown {
  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(text);
  const candidates = [fenced?.[1], text.slice(text.indexOf("{"), text.lastIndexOf("}") + 1)];
  for (const c of candidates) {
    if (!c || !c.trim().startsWith("{")) continue;
    try {
      return JSON.parse(c);
    } catch {
      // try the next
    }
  }
  return undefined;
}

async function anthropicTool(cfg: ModelConfig, req: ToolRequest, opts: CallOptions): Promise<ToolResult> {
  const client = anthropicClient(cfg, TIMEOUT_MS, opts);
  return withRetry<ToolResult>(async () => {
    try {
      const response = await client.messages.create({
        model: cfg.model,
        max_tokens: req.maxTokens,
        system: cachedSystem(req.system),
        tools: [{ name: req.tool.name, description: req.tool.description, input_schema: req.tool.schema as Anthropic.Tool.InputSchema }],
        tool_choice: { type: "tool", name: req.tool.name },
        messages: [{ role: "user", content: req.prompt }],
      });
      const usage = anthropicUsage(response.usage);
      if (response.stop_reason === "refusal") return { result: { ok: false, error: "refusal" }, retry: false };
      const use = response.content.find((b) => b.type === "tool_use");
      if (use && use.type === "tool_use") return { result: { ok: true, input: use.input, usage }, retry: false };
      const text = response.content.map((b) => (b.type === "text" ? b.text : "")).join("");
      const parsed = jsonIn(text);
      return { result: parsed === undefined ? { ok: false, error: "no tool call" } : { ok: true, input: parsed, usage }, retry: false };
    } catch (err) {
      return anthropicFailure<ToolResult & { ok: false }>(err);
    }
  }, opts.sleep, opts.retries);
}

type ToolCompletion = {
  choices?: { message?: { content?: string | null; tool_calls?: { function?: { name?: string; arguments?: string } }[] } }[];
  usage?: Parameters<typeof completionUsage>[0];
};

async function completionsTool(cfg: ModelConfig, req: ToolRequest, opts: CallOptions): Promise<ToolResult> {
  const res = await postCompletions<ToolCompletion>(
    cfg,
    {
      max_tokens: req.maxTokens,
      tools: [{ type: "function", function: { name: req.tool.name, description: req.tool.description, parameters: req.tool.schema } }],
      tool_choice: { type: "function", function: { name: req.tool.name } },
      messages: [
        { role: "system", content: req.system },
        { role: "user", content: req.prompt },
      ],
    },
    TIMEOUT_MS,
    opts,
  );
  if (!res.ok) return res;
  const usage = completionUsage(res.body?.usage);
  const message = res.body?.choices?.[0]?.message;
  const args = message?.tool_calls?.find((c) => c.function?.name === req.tool.name)?.function?.arguments;
  const parsed = args !== undefined ? jsonIn(args) : jsonIn(message?.content ?? "");
  if (parsed === undefined) return { ok: false, error: short(`no tool call: ${res.text}`) };
  return usage ? { ok: true, input: parsed, usage } : { ok: true, input: parsed };
}

export async function callTool(cfg: ModelConfig, req: ToolRequest, opts: CallOptions = {}): Promise<ToolResult> {
  const result = cfg.provider === "anthropic" ? await anthropicTool(cfg, req, opts) : await completionsTool(cfg, req, opts);
  await opts.track?.(result.ok ? { ok: true, ...(result.usage ? { usage: result.usage } : {}) } : result);
  return result;
}

/** 测试连接's second check (模 1.7): a forced tool call, as the front desk, the review and the tidy make. */
export const PING_TOOL: ToolRequest = {
  system: "Call the tool `answer` with ok set to true.",
  prompt: "ping",
  tool: {
    name: "answer",
    description: "Answer the ping.",
    schema: { type: "object", properties: { ok: { type: "boolean" } }, required: ["ok"] },
  },
  maxTokens: 64,
};
