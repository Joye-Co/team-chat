#!/usr/bin/env bun
// Replays front desk cases on a model and prints accuracy, confusion, latency and tokens (M6).
//
// Usage: bun scripts/replay-frontdesk.ts --cases <file.jsonl> --provider anthropic|deepseek|openai-compatible
//          --model <id> --key-file <path> [--base-url <url>] [--json]
// Cases: one JSON per line { id?, system, prompt, expect: reply|dispatch|ask|skip }. Export real
// ones with GET /v1/bots/:id/decisions/export and fix `expect` by hand; keep them out of this
// repository (they hold topic text). test/fixtures/frontdesk-cases.jsonl has synthetic ones.
import { readFileSync } from "node:fs";
import type { AiProvider } from "@chat/protocol";
import { parseCases, replay } from "../src/agent/replay";

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const casesPath = arg("cases");
const provider = arg("provider") as AiProvider | undefined;
const model = arg("model");
const keyFile = arg("key-file");
if (!casesPath || !provider || !model || !keyFile) {
  console.error("usage: bun scripts/replay-frontdesk.ts --cases <file> --provider <p> --model <m> --key-file <path> [--base-url <u>] [--json]");
  process.exit(2);
}
const apiKey = readFileSync(keyFile, "utf8").trim();
const cases = parseCases(readFileSync(casesPath, "utf8"));
const report = await replay({ provider, model, baseUrl: arg("base-url") ?? null, apiKey }, cases);
if (process.argv.includes("--json")) {
  console.log(JSON.stringify(report, null, 2));
} else {
  console.log(`${model}: ${report.right}/${report.cases} right (${(report.accuracy * 100).toFixed(1)}%), p50 ${report.p50Ms} ms, p95 ${report.p95Ms} ms, tokens ${report.inputTokens}+${report.outputTokens}`);
  for (const [expect, got] of Object.entries(report.confusion)) console.log(`  ${expect} → ${JSON.stringify(got)}`);
  for (const w of report.wrong) console.log(`  wrong ${w.id}: expected ${w.expect}, got ${w.got} (${w.reason})`);
}
