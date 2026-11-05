// One model call to the team's provider: Anthropic through the Messages API (SDK), DeepSeek and
// OpenAI-compatible providers through `POST <base>/chat/completions`. Never throws for a
// provider error: it comes back as `{ ok: false, error }` with the provider's status and message.
import Anthropic from "@anthropic-ai/sdk";
import type { AiProvider } from "@chat/protocol";

export const DEEPSEEK_BASE_URL = "https://api.deepseek.com";
const TIMEOUT_MS = 20_000;
const MAX_ERROR = 500;

export interface ModelConfig {
  provider: AiProvider;
  model: string;
  /** openai-compatible only. */
  baseUrl: string | null;
  apiKey: string;
}

export interface ModelRequest {
  system: string;
  prompt: string;
  maxTokens: number;
}

export type ModelResult = { ok: true; text: string } | { ok: false; error: string };

export interface CallOptions {
  /** Tests pass a fake; the Worker's own fetch otherwise. */
  fetch?: typeof fetch;
}

function short(text: string): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > MAX_ERROR ? `${one.slice(0, MAX_ERROR - 1)}…` : one;
}

/** The `/chat/completions` URL of an API root; a root that already ends with it is kept. */
export function completionsUrl(provider: AiProvider, baseUrl: string | null): string {
  const root = (provider === "deepseek" ? DEEPSEEK_BASE_URL : (baseUrl ?? "")).replace(/\/+$/, "");
  return root.endsWith("/chat/completions") ? root : `${root}/chat/completions`;
}

/** Claude models that take `output_config.effort` (Opus 4.5+, Sonnet 4.6+, every 5.x model). */
export function takesEffort(model: string): boolean {
  const m = /^claude-(opus|sonnet|haiku|fable|mythos)-(\d+)(?:-(\d+))?/.exec(model);
  if (!m) return false;
  const [major, minor] = [Number(m[2]), Number(m[3] ?? 0)];
  if (major >= 5) return true;
  if (major === 4 && m[1] === "opus") return minor >= 5;
  if (major === 4 && m[1] === "sonnet") return minor >= 6;
  return false;
}

async function callAnthropic(cfg: ModelConfig, req: ModelRequest, opts: CallOptions): Promise<ModelResult> {
  const client = new Anthropic({ apiKey: cfg.apiKey, maxRetries: 1, timeout: TIMEOUT_MS, fetch: opts.fetch ?? ((...a) => fetch(...a)) });
  try {
    const response = await client.messages.create({
      model: cfg.model,
      max_tokens: req.maxTokens,
      ...(takesEffort(cfg.model) ? { output_config: { effort: "low" as const } } : {}),
      system: req.system,
      messages: [{ role: "user", content: req.prompt }],
    });
    if (response.stop_reason === "refusal") return { ok: false, error: "refusal" };
    return { ok: true, text: response.content.map((b) => (b.type === "text" ? b.text : "")).join("") };
  } catch (err) {
    if (err instanceof Anthropic.APIError) return { ok: false, error: short(`${err.status ?? ""} ${err.message}`) };
    return { ok: false, error: short(String((err as Error)?.message ?? err)) };
  }
}

type Completion = { choices?: { message?: { content?: string | null } }[]; error?: { message?: string } | string };

async function callCompletions(cfg: ModelConfig, req: ModelRequest, opts: CallOptions): Promise<ModelResult> {
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
  let body: Completion | null = null;
  try {
    body = JSON.parse(text) as Completion;
  } catch {
    // not JSON
  }
  if (!res.ok) {
    const e = body?.error;
    const message = typeof e === "string" ? e : (e?.message ?? text);
    return { ok: false, error: short(`${res.status} ${message || res.statusText}`) };
  }
  const content = body?.choices?.[0]?.message?.content;
  if (typeof content !== "string") return { ok: false, error: short(`unexpected response: ${text}`) };
  return { ok: true, text: content };
}

export function callModel(cfg: ModelConfig, req: ModelRequest, opts: CallOptions = {}): Promise<ModelResult> {
  return cfg.provider === "anthropic" ? callAnthropic(cfg, req, opts) : callCompletions(cfg, req, opts);
}

/** 测试连接: the smallest call that proves the key, model and URL work. */
export const PING: ModelRequest = { system: "Reply with the single word OK.", prompt: "ping", maxTokens: 16 };
