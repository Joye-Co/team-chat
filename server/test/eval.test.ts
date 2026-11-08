// M6 (docs/design/agent-harness.zh.md 9): replay of front desk cases, the export that builds a
// replay set, metrics, and the prompt patch register. Provider calls go to a fake fetch.
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import cases from "./fixtures/frontdesk-cases.jsonl?raw";
import { PATCHES } from "../src/agent/patches";
import { parseCases, replay } from "../src/agent/replay";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const realFetch = globalThis.fetch;
/** DeepSeek answers `decide` with `pick(prompt)`; other calls (titles) with text. */
function fakeModel(pick: (prompt: string) => string): void {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
    const body = (await req.json()) as any;
    if (!body.tools) return Response.json({ choices: [{ message: { role: "assistant", content: "标题" } }] });
    const action = pick(body.messages[1].content as string);
    const args = { action, reason: "r", confidence: 0.9, ...(action === "reply" || action === "ask" ? { reply: "好" } : {}), ...(action === "dispatch" ? { brief: { goal: "g" } } : {}) };
    return Response.json({
      choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: "decide", arguments: JSON.stringify(args) } }] } }],
      usage: { prompt_tokens: 100, completion_tokens: 10 },
    });
  });
}

const cfg = { provider: "deepseek" as const, model: "deepseek-chat", baseUrl: null, apiKey: "sk-replay" };

describe("replay", () => {
  it("scores a model on the cases and lists what it got wrong", async () => {
    const list = parseCases(cases);
    expect(list.length).toBeGreaterThanOrEqual(12);
    // A model that always dispatches gets exactly the dispatch cases right.
    fakeModel(() => "dispatch");
    const report = await replay(cfg, list);
    const dispatches = list.filter((c) => c.expect === "dispatch").length;
    expect(report.right).toBe(dispatches);
    expect(report.confusion.reply).toEqual({ dispatch: list.filter((c) => c.expect === "reply").length });
    expect(report.wrong.every((w) => w.got === "dispatch")).toBe(true);
    expect(report.inputTokens).toBe(100 * list.length);
    expect(() => parseCases('{"system":"s","prompt":"p","expect":"maybe"}')).toThrow(/expect/);
  });

  it("keeps a register of prompt patches with why and the failure they fixed", () => {
    const ids = new Set(PATCHES.map((p) => p.id));
    expect(ids.size).toBe(PATCHES.length);
    for (const p of PATCHES) expect(p.why && p.failure && p.where && p.addedWith).toBeTruthy();
  });
});

describe("export and metrics", () => {
  it("exports decisions with what the desk read, and counts what happened", async () => {
    const w = await world();
    const ai = await api("/v1/settings/ai", { method: "PUT", token: w.owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-test-eval" } });
    expect(ai.status).toBe(200);
    fakeModel((p) => (p.includes("你在吗") ? "reply" : "dispatch"));
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w, "@dev 你在吗");
    await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id);
    await mention(c, w, "@dev 修一下测试");
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    const res = await api(`/v1/bots/${w.bot.id}/decisions/export?since=0`, { token: w.owner.token });
    expect(res.status).toBe(200);
    const lines = parseCases(res.body as string);
    expect(lines.map((l) => l.expect)).toEqual(["reply", "dispatch"]);
    expect(lines[0].prompt).toContain("你在吗");
    expect(lines[0].system).toContain("front desk");
    expect((await api(`/v1/bots/${w.bot.id}/decisions/export`, { token: w.human.token })).status).toBe(403);

    const m = await api(`/v1/bots/${w.bot.id}/metrics?days=7`, { token: w.owner.token });
    expect(m.status).toBe(200);
    expect(m.body.frontDesk).toMatchObject({ decisions: 2, byAction: { reply: 1, dispatch: 1, ask: 0, skip: 0 }, inputTokens: 200 });
    expect(m.body.runs).toMatchObject({ total: 1, done: 1, restarted: 0 });
    expect((await api(`/v1/bots/${w.bot.id}/metrics?days=0`, { token: w.owner.token })).status).toBe(400);
    d.close();
    c.close();
  });
});
