// One forced tool call to the team's provider: the model must answer through `tool`, so the
// answer has a fixed shape (the front desk's decision, docs/design/agent-harness.zh.md 3.1).
// Anthropic: Messages tool use with `tool_choice`. DeepSeek and OpenAI-compatible: function
// calling. A provider that answers with text instead gets one chance: a JSON object in the text.
import Anthropic from "@anthropic-ai/sdk";
import { type CallOptions, type ModelConfig, completionsUrl } from "./providers";

const TIMEOUT_MS = 30_000;
const MAX_ERROR = 500;

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

export type ToolResult = { ok: true; input: unknown; usage?: { inputTokens: number; outputTokens: number } } | { ok: false; error: string };

function short(text: string): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > MAX_ERROR ? `${one.slice(0, MAX_ERROR - 1)}…` : one;
}

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
  const client = new Anthropic({ apiKey: cfg.apiKey, maxRetries: 1, timeout: TIMEOUT_MS, fetch: opts.fetch ?? ((...a) => fetch(...a)) });
  try {
    const response = await client.messages.create({
      model: cfg.model,
      max_tokens: req.maxTokens,
      system: req.system,
      tools: [{ name: req.tool.name, description: req.tool.description, input_schema: req.tool.schema as Anthropic.Tool.InputSchema }],
      tool_choice: { type: "tool", name: req.tool.name },
      messages: [{ role: "user", content: req.prompt }],
    });
    const usage = { inputTokens: response.usage.input_tokens, outputTokens: response.usage.output_tokens };
    if (response.stop_reason === "refusal") return { ok: false, error: "refusal" };
    const use = response.content.find((b) => b.type === "tool_use");
    if (use && use.type === "tool_use") return { ok: true, input: use.input, usage };
    const text = response.content.map((b) => (b.type === "text" ? b.text : "")).join("");
    const parsed = jsonIn(text);
    return parsed === undefined ? { ok: false, error: "no tool call" } : { ok: true, input: parsed, usage };
  } catch (err) {
    if (err instanceof Anthropic.APIError) return { ok: false, error: short(`${err.status ?? ""} ${err.message}`) };
    return { ok: false, error: short(String((err as Error)?.message ?? err)) };
  }
}

type ToolCompletion = {
  choices?: { message?: { content?: string | null; tool_calls?: { function?: { name?: string; arguments?: string } }[] } }[];
  usage?: { prompt_tokens?: number; completion_tokens?: number };
  error?: { message?: string } | string;
};

async function completionsTool(cfg: ModelConfig, req: ToolRequest, opts: CallOptions): Promise<ToolResult> {
  const doFetch = opts.fetch ?? fetch;
  let res: Response;
  try {
    res = await doFetch(completionsUrl(cfg.provider, cfg.baseUrl), {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${cfg.apiKey}` },
      body: JSON.stringify({
        model: cfg.model,
        max_tokens: req.maxTokens,
        stream: false,
        tools: [{ type: "function", function: { name: req.tool.name, description: req.tool.description, parameters: req.tool.schema } }],
        tool_choice: { type: "function", function: { name: req.tool.name } },
        messages: [
          { role: "system", content: req.system },
          { role: "user", content: req.prompt },
        ],
      }),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch (err) {
    return { ok: false, error: short(String((err as Error)?.message ?? err)) };
  }
  const text = await res.text();
  let body: ToolCompletion | null = null;
  try {
    body = JSON.parse(text) as ToolCompletion;
  } catch {
    // not JSON
  }
  if (!res.ok) {
    const e = body?.error;
    const message = typeof e === "string" ? e : (e?.message ?? text);
    return { ok: false, error: short(`${res.status} ${message || res.statusText}`) };
  }
  const usage = body?.usage ? { inputTokens: body.usage.prompt_tokens ?? 0, outputTokens: body.usage.completion_tokens ?? 0 } : undefined;
  const message = body?.choices?.[0]?.message;
  const args = message?.tool_calls?.find((c) => c.function?.name === req.tool.name)?.function?.arguments;
  const parsed = args !== undefined ? jsonIn(args) : jsonIn(message?.content ?? "");
  if (parsed === undefined) return { ok: false, error: short(`no tool call: ${text}`) };
  return usage ? { ok: true, input: parsed, usage } : { ok: true, input: parsed };
}

export function callTool(cfg: ModelConfig, req: ToolRequest, opts: CallOptions = {}): Promise<ToolResult> {
  return cfg.provider === "anthropic" ? anthropicTool(cfg, req, opts) : completionsTool(cfg, req, opts);
}
