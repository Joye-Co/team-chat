// Per-model prices for 用量 (docs/protocol.md "Team usage"). They come from models.dev
// (https://models.dev/api.json, which also lists cache prices): the daily cron fetches it and keeps
// the last good copy in D1 (`model_prices`, migration 0014). A small built-in list is used only
// while no fetch has ever worked. A model is priced only by its own entry: a model with no price
// is "价格未知" and never borrows another model's price.
import type { ModelUsage } from "@chat/protocol";
import type { UsageRun } from "../bots/desk/external";
import type { Env } from "../core/env";

export const PRICES_URL = "https://models.dev/api.json";
/** The prices are fetched again after this long (the cron runs daily; this covers a missed run). */
export const PRICES_MAX_AGE_MS = 26 * 3_600_000;

/** USD per million tokens, by kind. */
export interface ModelPrice {
  name: string;
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
}

export type PriceList = Map<string, ModelPrice>;

/**
 * Used only while no fetch from models.dev has ever worked. Published list prices; models.dev
 * replaces them on the first good sync.
 */
export const FALLBACK: Record<string, ModelPrice> = {
  "claude-opus-4-5": { name: "Claude Opus 4.5", input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
  "claude-sonnet-4-5": { name: "Claude Sonnet 4.5", input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 },
  "claude-haiku-4-5": { name: "Claude Haiku 4.5", input: 1, output: 5, cacheRead: 0.1, cacheWrite: 1.25 },
  "gpt-5-codex": { name: "GPT-5-Codex", input: 1.25, output: 10, cacheRead: 0.125 },
  "gpt-5.1-codex": { name: "GPT-5.1-Codex", input: 1.25, output: 10, cacheRead: 0.125 },
  "gpt-5": { name: "GPT-5", input: 1.25, output: 10, cacheRead: 0.125 },
  // The AI 服务's suggested DeepSeek model (用量's 前台和整理 line).
  "deepseek-chat": { name: "DeepSeek Chat", input: 0.28, output: 0.42, cacheRead: 0.028 },
};

/** Providers whose own entries win when two list the same model id. */
const FIRST_PARTY = ["anthropic", "openai", "google", "deepseek", "moonshotai", "zai", "xai", "mistral"];

/** models.dev `api.json` → id → price. Models without input and output prices are left out. */
export function parseModelsDev(json: unknown): PriceList {
  const out: PriceList = new Map();
  if (!json || typeof json !== "object") return out;
  const providers = Object.entries(json as Record<string, unknown>).sort(([a], [b]) => {
    const ia = FIRST_PARTY.indexOf(a);
    const ib = FIRST_PARTY.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });
  for (const [, p] of providers) {
    const models = (p as { models?: Record<string, unknown> })?.models;
    if (!models || typeof models !== "object") continue;
    for (const [key, raw] of Object.entries(models)) {
      const m = raw as { id?: unknown; name?: unknown; cost?: Record<string, unknown> };
      const c = m?.cost;
      const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : undefined);
      const input = num(c?.input);
      const output = num(c?.output);
      if (input === undefined || output === undefined) continue;
      const id = normalize(typeof m.id === "string" ? m.id : key);
      if (!id || out.has(id)) continue;
      out.set(id, {
        name: typeof m.name === "string" && m.name ? m.name.slice(0, 80) : id,
        input,
        output,
        ...(num(c?.cache_read) !== undefined ? { cacheRead: num(c?.cache_read) } : {}),
        ...(num(c?.cache_write) !== undefined ? { cacheWrite: num(c?.cache_write) } : {}),
      });
    }
  }
  return out;
}

/** Lower case, without a provider prefix ("openai/gpt-5" → "gpt-5"). */
function normalize(id: string): string {
  const s = id.trim().toLowerCase();
  const slash = s.lastIndexOf("/");
  return slash >= 0 ? s.slice(slash + 1) : s;
}

/**
 * The price of the model a CLI reported, or null. The id matches exactly, else without a date or
 * `-latest` suffix ("claude-sonnet-4-5-20250929" → "claude-sonnet-4-5"); never another model.
 */
export function matchModel(prices: PriceList, model: string): { id: string; price: ModelPrice } | null {
  const id = normalize(model);
  const tries = [id, id.replace(/[-@]\d{8}$/, ""), id.replace(/-latest$/, ""), id.replace(/[-@]\d{4}-\d{2}-\d{2}$/, "")];
  for (const t of tries) {
    const p = prices.get(t);
    if (p) return { id: t, price: p };
  }
  return null;
}

/** USD for one model's tokens. Cache tokens without their own price count at the input price. */
export function costOf(price: ModelPrice, m: Pick<ModelUsage, "inputTokens" | "outputTokens" | "cacheReadTokens" | "cacheWriteTokens">): number {
  return (
    (m.inputTokens * price.input +
      m.outputTokens * price.output +
      (m.cacheReadTokens ?? 0) * (price.cacheRead ?? price.input) +
      (m.cacheWriteTokens ?? 0) * (price.cacheWrite ?? price.input)) /
    1_000_000
  );
}

function fallbackList(): PriceList {
  return new Map(Object.entries(FALLBACK));
}

let cache: { list: PriceList; at: number; syncedAt: number } | null = null;
let syncing: Promise<unknown> | null = null;

/** The stored price list (the built-in one before any good sync); fetches again when stale. */
export async function loadPrices(env: Env, waitUntil?: (p: Promise<unknown>) => void): Promise<PriceList> {
  const now = Date.now();
  if (!cache || now - cache.at > 10 * 60_000) {
    const rows = await env.DB.prepare("SELECT model, name, input, output, cache_read, cache_write, synced_at FROM model_prices")
      .all<{ model: string; name: string; input: number; output: number; cache_read: number | null; cache_write: number | null; synced_at: number }>()
      .catch(() => ({ results: [] }));
    const list: PriceList = new Map();
    let syncedAt = 0;
    for (const r of rows.results) {
      list.set(r.model, {
        name: r.name,
        input: r.input,
        output: r.output,
        ...(r.cache_read !== null ? { cacheRead: r.cache_read } : {}),
        ...(r.cache_write !== null ? { cacheWrite: r.cache_write } : {}),
      });
      syncedAt = Math.max(syncedAt, r.synced_at);
    }
    cache = { list: list.size > 0 ? list : fallbackList(), at: now, syncedAt };
  }
  if (waitUntil && now - cache.syncedAt > PRICES_MAX_AGE_MS && !syncing) {
    const p = syncPrices(env)
      .catch(() => 0)
      .finally(() => {
        syncing = null;
      });
    syncing = p;
    waitUntil(p);
  }
  return cache.list;
}

/**
 * Fetches models.dev and replaces the stored list. A failed fetch or an empty answer keeps the
 * last good copy. Returns how many models were stored.
 */
export async function syncPrices(env: Env, fetcher: typeof fetch = fetch): Promise<number> {
  const res = await fetcher(PRICES_URL, { headers: { accept: "application/json" } });
  if (!res.ok) throw new Error(`models.dev answered ${res.status}`);
  const list = parseModelsDev(await res.json());
  if (list.size < 10) throw new Error(`models.dev listed only ${list.size} priced models`);
  const now = Date.now();
  const stmts = [env.DB.prepare("DELETE FROM model_prices")];
  for (const [model, p] of list) {
    stmts.push(
      env.DB.prepare("INSERT INTO model_prices (model, name, input, output, cache_read, cache_write, synced_at) VALUES (?, ?, ?, ?, ?, ?, ?)").bind(
        model,
        p.name,
        p.input,
        p.output,
        p.cacheRead ?? null,
        p.cacheWrite ?? null,
        now,
      ),
    );
  }
  await env.DB.batch(stmts);
  cache = { list, at: now, syncedAt: now };
  return list.size;
}

/** Forgets the in-memory copy (tests). */
export function resetPriceCache(): void {
  cache = null;
}

/** What one model of a run cost: reported by the CLI, estimated from the list, or unknown (null). */
export interface PricedModel {
  model: string;
  name: string;
  costUsd: number | null;
  estimated: boolean;
  /** Input with cache reads and writes, and output. */
  inputTokens: number;
  outputTokens: number;
}

/** A run with its cost settled: `estimated` is the part priced from tokens, `unpricedTokens` have no price. */
export type Priced = UsageRun & { estimated: number; unpricedTokens: number; priced: PricedModel[] };

/**
 * A run's cost by model. With per-model usage (daemons from 1.3), each model is the CLI's own
 * cost or its tokens at that model's list price, else unknown. An older run without it keeps a
 * reported total; with none, it is priced only by the model the AI member is set to run, if any.
 */
export function priceRun(run: UsageRun, prices: PriceList, runner?: { runtime: string | null; model: string | null }): Priced {
  const priced: PricedModel[] = [];
  let cost = 0;
  let estimated = 0;
  let unpriced = 0;
  const one = (m: ModelUsage) => {
    const input = m.inputTokens + (m.cacheReadTokens ?? 0) + (m.cacheWriteTokens ?? 0);
    const tokens = input + m.outputTokens;
    const io = { inputTokens: input, outputTokens: m.outputTokens };
    const hit = matchModel(prices, m.model);
    const name = hit?.price.name ?? m.model;
    if (typeof m.costUsd === "number") {
      cost += m.costUsd;
      priced.push({ model: hit?.id ?? normalize(m.model), name, costUsd: m.costUsd, estimated: false, ...io });
    } else if (hit) {
      const c = costOf(hit.price, m);
      cost += c;
      estimated += c;
      priced.push({ model: hit.id, name, costUsd: c, estimated: true, ...io });
    } else {
      unpriced += tokens;
      priced.push({ model: normalize(m.model), name, costUsd: null, estimated: false, ...io });
    }
  };
  if (run.models && run.models.length > 0) {
    for (const m of run.models) one(m);
    // The CLI's total wins over the sum of its models when it reports one.
    if (run.costUsd > 0 && estimated === 0 && unpriced === 0) cost = run.costUsd;
  } else if (run.costUsd > 0) {
    cost = run.costUsd;
  } else if (run.inputTokens + run.outputTokens > 0) {
    const sameCli = runner && (!run.runtime || run.runtime === runner.runtime);
    const model = sameCli ? runner?.model : null;
    if (model) one({ model, inputTokens: run.inputTokens, outputTokens: run.outputTokens });
    else unpriced += run.inputTokens + run.outputTokens;
  }
  return { ...run, costUsd: cost, estimated, unpricedTokens: unpriced, priced };
}
