// List prices for estimating a run's cost from its tokens, for CLIs that report no cost (Codex).
// USD per million tokens. The one place to update when a provider changes its prices: a model id
// matches the longest key it starts with; a runtime with no model uses its default.
// Prices as published by OpenAI and Anthropic (standard tier, no cache discount).

export interface Price {
  /** USD per million input tokens. */
  input: number;
  /** USD per million output tokens. */
  output: number;
}

export const PRICES: Record<string, Price> = {
  // OpenAI (Codex)
  "gpt-5-codex": { input: 1.25, output: 10 },
  "gpt-5.1-codex-mini": { input: 0.25, output: 2 },
  "gpt-5.1-codex": { input: 1.25, output: 10 },
  "gpt-5-mini": { input: 0.25, output: 2 },
  "gpt-5-nano": { input: 0.05, output: 0.4 },
  "gpt-5": { input: 1.25, output: 10 },
  "codex-mini": { input: 1.5, output: 6 },
  "o4-mini": { input: 1.1, output: 4.4 },
  "o3": { input: 2, output: 8 },
  "gpt-4.1-mini": { input: 0.4, output: 1.6 },
  "gpt-4.1": { input: 2, output: 8 },
  // Anthropic (Claude Code reports its own cost; these cover runs that did not)
  "claude-opus-4": { input: 15, output: 75 },
  "claude-sonnet-4": { input: 3, output: 15 },
  "claude-haiku-4": { input: 1, output: 5 },
  opus: { input: 15, output: 75 },
  sonnet: { input: 3, output: 15 },
  haiku: { input: 1, output: 5 },
};

/** The model a runtime runs when its AI member names none. */
export const RUNTIME_DEFAULT: Record<string, string> = {
  codex: "gpt-5-codex",
  claude: "sonnet",
};

/** The price of a model (or of the runtime's default), or null when unknown. */
export function priceOf(runtime: string | null, model: string | null): Price | null {
  const id = (model ?? (runtime ? RUNTIME_DEFAULT[runtime] : undefined) ?? "").toLowerCase();
  if (!id) return null;
  let best: string | null = null;
  for (const key of Object.keys(PRICES)) {
    if (id.startsWith(key) && (best === null || key.length > best.length)) best = key;
  }
  return best ? PRICES[best] : null;
}

/** The estimated cost of tokens at a price, in USD. */
export function estimate(price: Price, inputTokens: number, outputTokens: number): number {
  return (inputTokens * price.input + outputTokens * price.output) / 1_000_000;
}
