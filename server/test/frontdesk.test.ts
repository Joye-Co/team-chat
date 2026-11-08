// The front desk (agent/frontdesk.ts, bots/desk/front.ts): with an AI service, a new request is
// decided by one forced tool call before it becomes a run. Provider calls go to a fake fetch.
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { jsonIn } from "../src/aiservice/tools";
import { MIN_CONFIDENCE, settle } from "../src/agent/frontdesk";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const realFetch = globalThis.fetch;
type Seen = { body: any };

/** DeepSeek answers each call with the next decision (a function call). */
function fakeDesk(decisions: (unknown | Error)[]): Seen[] {
  const seen: Seen[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
    const body = (await req.json()) as any;
    // Other model calls (topic titles) get a plain answer.
    if (!body.tools) return Response.json({ choices: [{ message: { role: "assistant", content: "标题" } }] });
    // The review after a run (memory/retro.ts) finds nothing to keep.
    if (body.tool_choice?.function?.name !== "decide") return Response.json({ choices: [{ message: { role: "assistant", content: '{"lessons":[]}' } }] });
    seen.push({ body });
    const d = decisions.shift();
    if (d instanceof Error) return new Response(JSON.stringify({ error: { message: d.message } }), { status: 500 });
    return Response.json({
      choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: "decide", arguments: JSON.stringify(d) } }] } }],
      usage: { prompt_tokens: 900, completion_tokens: 60 },
    });
  });
  return seen;
}

async function aiOn(token: string): Promise<void> {
  const r = await api("/v1/settings/ai", { method: "PUT", token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-test-front-desk" } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
}

describe("front desk rules", () => {
  it("falls back to a run when the answer is invalid, empty or unsure", () => {
    expect(settle({ action: "reply", reason: "hi", confidence: 0.9, reply: "你好" }).decision.action).toBe("reply");
    expect(settle({ action: "reply", reason: "x", confidence: MIN_CONFIDENCE - 0.1, reply: "maybe" })).toMatchObject({ decision: { action: "dispatch" }, fallback: "low confidence" });
    expect(settle({ action: "ask", confidence: 1 })).toMatchObject({ decision: { action: "dispatch" }, fallback: "no reply text" });
    expect(settle({ action: "explode" })).toMatchObject({ decision: { action: "dispatch" }, fallback: "invalid decision" });
    expect(jsonIn('Here:\n```json\n{"action":"skip"}\n```')).toEqual({ action: "skip" });
    expect(jsonIn("no json")).toBeUndefined();
  });
});

describe("front desk", () => {
  it("answers simple questions itself and hands work to the executor with a work order", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk([
      { action: "reply", reason: "a greeting", confidence: 0.95, reply: "你好，我在。" },
      { action: "dispatch", reason: "needs the repo", confidence: 0.9, brief: { goal: "修好登录页的测试", doneWhen: ["cargo test 通过"], notes: "看 m 01X" } },
    ]);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 你在吗");
    const reply = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === topicId);
    expect(reply.data.message.body).toBe("你好，我在。");
    expect(seen[0].body.tool_choice).toEqual({ type: "function", function: { name: "decide" } });
    expect(seen[0].body.messages[1].content).toContain("你在吗");
    expect(d.frames.some((f) => f.t === "run.assign")).toBe(false);

    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 登录页测试挂了，修一下", mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("Work order from your front desk");
    expect(a.prompt).toContain("Goal: 修好登录页的测试");
    expect(a.prompt).toContain("- cargo test 通过");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));

    const list = await api(`/v1/bots/${w.bot.id}/decisions?groupId=${w.gid}&topicId=${topicId}`, { token: w.owner.token });
    expect(list.status).toBe(200);
    expect(list.body.decisions.map((x: any) => x.action)).toEqual(["dispatch", "reply"]);
    expect(list.body.decisions[1]).toMatchObject({ reason: "a greeting", inputTokens: 900, model: "deepseek-chat" });
    expect((await api(`/v1/bots/${w.bot.id}/decisions?groupId=${w.gid}&topicId=${topicId}`, { token: w.human.token })).status).toBe(403);
    d.close();
    c.close();
  });

  it("asks one question, takes the answer without an @, and runs on a model error", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    fakeDesk([
      { action: "ask", reason: "which page", confidence: 0.9, reply: "是哪个页面？" },
      { action: "dispatch", reason: "clear now", confidence: 0.9, brief: { goal: "修设置页" } },
      new Error("overloaded"),
    ]);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 页面坏了");
    await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body === "是哪个页面？");
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "设置页" });
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("Goal: 修设置页");
    expect(a.prompt).toContain("设置页");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // The provider fails: the request becomes a run as without a front desk.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再看看", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt).not.toContain("Work order");
    await waitUntil(() => true);
    d.close();
    c.close();
  });
});
