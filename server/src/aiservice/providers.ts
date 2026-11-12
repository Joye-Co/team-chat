// One model call to the team's provider: Anthropic through the Messages API (SDK), DeepSeek and
// OpenAI-compatible providers through `POST <base>/chat/completions`. Never throws for a
// provider error: it comes back as `{ ok: false, error }` with the provider's status and message.
// Both paths share one retry policy (retry.ts) and report the tokens they used; with `track` the
// call is recorded under its use (uses.ts: health, the failing alert and 用量).
import Anthropic from "@anthropic-ai/sdk";
import type { AiProvider } from "@chat/protocol";
import { type Attempt, type Sleep, retryAfterMs, retryable, withRetry } from "./retry";

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

/** What one call used. `inputTokens` leaves out cached input, as in `ModelUsage`. */
export interface CallUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
}

export type ModelResult = { ok: true; text: string; usage?: CallUsage } | { ok: false; error: string };

/** The uses of the team's model (`AiServiceUseHealth.use`). */
export type AiUse = "front_desk" | "title" | "summary" | "retro" | "tidy";

/** Records a finished call under its use (uses.ts `recordCall`); set by the callers that track. */
export type Tracker = (r: { ok: true; usage?: CallUsage } | { ok: false; error: string }) => Promise<void>;

export interface CallOptions {
  /** Tests pass a fake; the Worker's own fetch otherwise. */
  fetch?: typeof fetch;
  /** Tests: the pause between retries. */
  sleep?: Sleep;
  /** Records the call under its use: health, the failing alert and tokens (uses.ts `tracker`). */
  track?: Tracker;
}

export function short(text: string): string {
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

/** An Anthropic client that leaves retrying to retry.ts. */
export function anthropicClient(cfg: ModelConfig, timeout: number, opts: CallOptions): Anthropic {
  return new Anthropic({ apiKey: cfg.apiKey, maxRetries: 0, timeout, fetch: opts.fetch ?? ((...a) => fetch(...a)) });
}

/**
 * The system prompt as one block marked for Anthropic's prompt cache (模 1.11): the tools and the
 * system prompt are the stable prefix of every call of a use. DeepSeek and OpenAI-compatible
 * providers cache a repeated prefix on their own and need no mark.
 */
export function cachedSystem(system: string): Anthropic.TextBlockParam[] {
  return [{ type: "text", text: system, cache_control: { type: "ephemeral" } }];
}

export function anthropicUsage(u: Anthropic.Usage): CallUsage {
  return {
    inputTokens: u.input_tokens,
    outputTokens: u.output_tokens,
    ...(u.cache_read_input_tokens ? { cacheReadTokens: u.cache_read_input_tokens } : {}),
    ...(u.cache_creation_input_tokens ? { cacheWriteTokens: u.cache_creation_input_tokens } : {}),
  };
}

/** An Anthropic error as a final result or a retry. */
export function anthropicFailure<T extends { ok: false; error: string }>(err: unknown): Attempt<T> {
  if (err instanceof Anthropic.APIError) {
    const after = err.headers instanceof Headers ? retryAfterMs(err.headers.get("retry-after")) : undefined;
    return { result: { ok: false, error: short(`${err.status ?? ""} ${err.message}`) } as T, retry: retryable(err.status), ...(after !== undefined ? { afterMs: after } : {}) };
  }
  return { result: { ok: false, error: short(String((err as Error)?.message ?? err)) } as T, retry: false };
}

async function callAnthropic(cfg: ModelConfig, req: ModelRequest, opts: CallOptions): Promise<ModelResult> {
  const client = anthropicClient(cfg, TIMEOUT_MS, opts);
  return withRetry<ModelResult>(async () => {
    try {
      const response = await client.messages.create({
        model: cfg.model,
        max_tokens: req.maxTokens,
        ...(takesEffort(cfg.model) ? { output_config: { effort: "low" as const } } : {}),
        system: cachedSystem(req.system),
        messages: [{ role: "user", content: req.prompt }],
      });
      if (response.stop_reason === "refusal") return { result: { ok: false, error: "refusal" }, retry: false };
      const text = response.content.map((b) => (b.type === "text" ? b.text : "")).join("");
      return { result: { ok: true, text, usage: anthropicUsage(response.usage) }, retry: false };
    } catch (err) {
      return anthropicFailure<ModelResult & { ok: false }>(err);
    }
  }, opts.sleep);
}

type CompletionUsage = {
  prompt_tokens?: number;
  completion_tokens?: number;
  /** DeepSeek. */
  prompt_cache_hit_tokens?: number;
  /** OpenAI. */
  prompt_tokens_details?: { cached_tokens?: number };
};

/** Tokens of a `/chat/completions` answer; cached input is counted apart, as in `ModelUsage`. */
export function completionUsage(u: CompletionUsage | undefined): CallUsage | undefined {
  if (!u) return undefined;
  const cached = u.prompt_cache_hit_tokens ?? u.prompt_tokens_details?.cached_tokens ?? 0;
  return {
    inputTokens: Math.max(0, (u.prompt_tokens ?? 0) - cached),
    outputTokens: u.completion_tokens ?? 0,
    ...(cached > 0 ? { cacheReadTokens: cached } : {}),
  };
}

/**
 * One `/chat/completions` POST under the retry policy: the parsed JSON body (null when not JSON)
 * and its text, or the final error.
 */
export async function postCompletions<B>(
  cfg: ModelConfig,
  payload: Record<string, unknown>,
  timeout: number,
  opts: CallOptions,
): Promise<{ ok: true; body: B | null; text: string } | { ok: false; error: string }> {
  const doFetch = opts.fetch ?? fetch;
  type R = { ok: true; body: B | null; text: string } | { ok: false; error: string };
  return withRetry<R>(async () => {
    let res: Response;
    try {
      res = await doFetch(completionsUrl(cfg.provider, cfg.baseUrl), {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Bearer ${cfg.apiKey}` },
        body: JSON.stringify({ model: cfg.model, stream: false, ...payload }),
        signal: AbortSignal.timeout(timeout),
      });
    } catch (err) {
      // A timeout or a network error: no answer, worth another try.
      return { result: { ok: false, error: short(String((err as Error)?.message ?? err)) }, retry: true };
    }
    const text = await res.text();
    let body: (B & { error?: { message?: string } | string }) | null = null;
    try {
      body = JSON.parse(text) as B & { error?: { message?: string } | string };
    } catch {
      // not JSON
    }
    if (!res.ok) {
      const e = body?.error;
      const message = typeof e === "string" ? e : (e?.message ?? text);
      const after = retryAfterMs(res.headers.get("retry-after"));
      return { result: { ok: false, error: short(`${res.status} ${message || res.statusText}`) }, retry: retryable(res.status), ...(after !== undefined ? { afterMs: after } : {}) };
    }
    return { result: { ok: true, body, text }, retry: false };
  }, opts.sleep);
}

type Completion = { choices?: { message?: { content?: string | null } }[]; usage?: CompletionUsage };

async function callCompletions(cfg: ModelConfig, req: ModelRequest, opts: CallOptions): Promise<ModelResult> {
  const res = await postCompletions<Completion>(
    cfg,
    {
      max_tokens: req.maxTokens,
      messages: [
        { role: "system", content: req.system },
        { role: "user", content: req.prompt },
      ],
    },
    TIMEOUT_MS,
    opts,
  );
  if (!res.ok) return res;
  const content = res.body?.choices?.[0]?.message?.content;
  if (typeof content !== "string") return { ok: false, error: short(`unexpected response: ${res.text}`) };
  const usage = completionUsage(res.body?.usage);
  return usage ? { ok: true, text: content, usage } : { ok: true, text: content };
}

export async function callModel(cfg: ModelConfig, req: ModelRequest, opts: CallOptions = {}): Promise<ModelResult> {
  const result = cfg.provider === "anthropic" ? await callAnthropic(cfg, req, opts) : await callCompletions(cfg, req, opts);
  await opts.track?.(result.ok ? { ok: true, ...(result.usage ? { usage: result.usage } : {}) } : result);
  return result;
}

/** 测试连接: the smallest call that proves the key, model and URL work. */
export const PING: ModelRequest = { system: "Reply with the single word OK.", prompt: "ping", maxTokens: 16 };
