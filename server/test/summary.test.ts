// Rolling topic summaries (summary/): a long topic gets a summary from the team's model, and a
// run's prompt carries it in front of the lines when its budget leaves messages out.
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { withSummary } from "../src/context/render";
import { SUMMARY_FROM } from "../src/summary";
import { Daemon, downViolations, world } from "./daemon";
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

async function send(c: Client, gid: string, topicId: string | undefined, body: string, extra: Record<string, unknown> = {}) {
  const frame = { t: "send", groupId: gid, ...(topicId ? { topicId } : {}), clientMsgId: clientMsgId(), body, ...extra };
  for (;;) {
    const r = await c.request(frame);
    if (r.ok) return r.result;
    expect(r.error?.code, JSON.stringify(r)).toBe("rate_limited");
    await new Promise((x) => setTimeout(x, (r.error.retryAfterMs ?? 100) + 20));
  }
}

describe("topic summary", () => {
  it("puts the summary in front only when messages were left out", () => {
    expect(withSummary({ text: "L", omitted: 0 }, { text: "S", uptoSeq: 9 })).toBe("L");
    expect(withSummary({ text: "L", omitted: 3 }, { text: "S", uptoSeq: 9 })).toBe("Summary of the topic up to #9 (the messages left out are in it):\nS\n\nL");
    expect(withSummary({ text: "L", omitted: 3 })).toBe("L");
  });

  it("summarizes a long topic and gives the summary to the run", async () => {
    const w = await world();
    expect((await api("/v1/settings/ai", { method: "PUT", token: w.owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-test-summary" } })).status).toBe(200);
    const seen: string[] = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
      const req = new Request(input, init);
      if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
      const body = (await req.json()) as any;
      // The front desk is not under test here: it always dispatches.
      if (body.tools) {
        const args = { action: "dispatch", reason: "work", confidence: 0.9, brief: { goal: "按需求做" } };
        return Response.json({ choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: "decide", arguments: JSON.stringify(args) } }] } }] });
      }
      const system = String(body.messages[0].content);
      if (system.includes("running summary")) {
        seen.push(String(body.messages[1].content));
        return Response.json({ choices: [{ message: { role: "assistant", content: "需求：登录页支持手机号（m 第一条）。" } }] });
      }
      return Response.json({ choices: [{ message: { role: "assistant", content: "登录改造" } }] });
    });
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const first = await send(c, w.gid, undefined, "需求：登录页要支持手机号");
    const topicId = first.message.topicId as string;
    for (let i = 2; i <= SUMMARY_FROM + 5; i++) await send(c, w.gid, topicId, `第 ${i} 条 ${"讨论".repeat(300)}`);
    await waitUntil(() => seen.length > 0, 10_000, "summary call");
    expect(seen[0]).toContain("需求：登录页要支持手机号");
    await new Promise((r) => setTimeout(r, 200));
    await send(c, w.gid, topicId, "@dev 按需求做", { mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign", 10_000);
    expect(a.prompt).toMatch(/Summary of the topic up to #\d+/);
    expect(a.prompt).toContain("需求：登录页支持手机号（m 第一条）。");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });
});
