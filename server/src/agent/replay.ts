// M6 replay (docs/design/agent-harness.zh.md 9): front desk cases run again on a model, offline,
// to compare models and prompt changes. A case is what the desk read and the action a person
// says is right. Real cases hold topic text: keep them in the private ops checkout.
import type { CallOptions, ModelConfig } from "../aiservice/providers";
import { callTool } from "../aiservice/tools";
import { DECIDE_TOOL, type Decision, settle } from "./frontdesk";

export interface ReplayCase {
  id: string;
  system: string;
  prompt: string;
  /** The right action, as a person judged it. */
  expect: Decision["action"];
}

export interface ReplayReport {
  cases: number;
  right: number;
  accuracy: number;
  /** expected → got → count */
  confusion: Record<string, Record<string, number>>;
  p50Ms: number;
  p95Ms: number;
  inputTokens: number;
  outputTokens: number;
  wrong: { id: string; expect: string; got: string; reason: string }[];
}

export function parseCases(jsonl: string): ReplayCase[] {
  return jsonl
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l, i) => {
      const c = JSON.parse(l) as Partial<ReplayCase>;
      if (!c.system || !c.prompt || !c.expect || !["reply", "dispatch", "ask", "skip"].includes(c.expect)) {
        throw new Error(`case ${i + 1}: needs system, prompt and expect (reply|dispatch|ask|skip)`);
      }
      return { id: c.id ?? String(i + 1), system: c.system, prompt: c.prompt, expect: c.expect };
    });
}

export async function replay(cfg: ModelConfig, cases: ReplayCase[], opts: CallOptions = {}): Promise<ReplayReport> {
  const confusion: ReplayReport["confusion"] = {};
  const wrong: ReplayReport["wrong"] = [];
  const ms: number[] = [];
  let right = 0;
  let inputTokens = 0;
  let outputTokens = 0;
  for (const c of cases) {
    const t0 = Date.now();
    const res = await callTool(cfg, { system: c.system, prompt: c.prompt, tool: DECIDE_TOOL, maxTokens: 1500 }, opts);
    ms.push(Date.now() - t0);
    const got = res.ok ? settle(res.input).decision : { action: "dispatch" as const, reason: `model error: ${res.ok ? "" : res.error}` };
    if (res.ok && res.usage) {
      inputTokens += res.usage.inputTokens;
      outputTokens += res.usage.outputTokens;
    }
    confusion[c.expect] ??= {};
    confusion[c.expect][got.action] = (confusion[c.expect][got.action] ?? 0) + 1;
    if (got.action === c.expect) right++;
    else wrong.push({ id: c.id, expect: c.expect, got: got.action, reason: got.reason ?? "" });
  }
  ms.sort((a, b) => a - b);
  const pct = (p: number) => (ms.length === 0 ? 0 : ms[Math.min(ms.length - 1, Math.floor((p / 100) * ms.length))]);
  return { cases: cases.length, right, accuracy: cases.length === 0 ? 0 : right / cases.length, confusion, p50Ms: pct(50), p95Ms: pct(95), inputTokens, outputTokens, wrong };
}
