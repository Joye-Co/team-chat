// The front desk (agent/frontdesk.ts, bots/desk/front.ts): with an AI service, a new request is
// decided by one forced tool call before it becomes a run. Provider calls go to a fake fetch.
import { afterEach, describe, expect, it, vi } from "vitest";
import { jsonIn } from "../src/aiservice/tools";
import { DECIDE_TOOL, MIN_CONFIDENCE, settle, systemPrompt } from "../src/agent/frontdesk";
import { Daemon, mention, world } from "./daemon";
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { FOLLOW_UP_WINDOW_MS } from "../src/bots/desk/input";
import { BASE, Client, api, clientMsgId, waitUntil } from "./helpers";

afterEach(() => {
  vi.restoreAllMocks();
});

const benv = env as unknown as { BOT_DESK: DurableObjectNamespace };
/** Moves a run's end back in time, in the bot's desk. */
const endEarlier = (botId: string, runId: string, ms: number) =>
  runInDurableObject(benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId)), (_o, state: DurableObjectState) => {
    state.storage.sql.exec("UPDATE runs SET ended_at = ended_at - ? WHERE id = ?", ms, runId);
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
  it("names the form of the result in the work order and knows that a mermaid fence is drawn", () => {
    const brief = (DECIDE_TOOL.schema as any).properties.brief.properties;
    expect(brief.notes.description).toContain("the form the result should take when the request implies one (a choice card, a data card, a 产出 page, a page people fill in together)");
    const system = systemPrompt({ botName: "dev" });
    expect(system).toContain("a `mermaid` fence in a reply is drawn as a diagram");
    // The reply rule is unchanged: what moves a request from reply to dispatch is decided by the replay.
    expect(system).toContain("- reply: conversation, thanks, or a question you can answer well from the topic and general knowledge alone.");
  });

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

  it("knows a direct chat and reads the request's files", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk([{ action: "reply", reason: "a look", confidence: 0.95, reply: "收到图了。" }]);
    const { c } = await Client.connect(w.human.token);
    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    expect(dm.status).toBe(200);
    const bytes = new TextEncoder().encode("png");
    const sum = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
    const f = await api("/v1/files", { token: w.human.token, body: { groupId: dm.body.group.id, name: "图.png", size: 3, mime: "image/png", sha256: sum } });
    expect(f.status, JSON.stringify(f.body)).toBe(200);
    const put = await SELF.fetch(`${BASE}/v1/files/${f.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${w.human.token}` }, body: bytes });
    expect(put.status).toBe(200);
    await c.ok({
      t: "send",
      groupId: dm.body.group.id,
      clientMsgId: clientMsgId(),
      body: "看下这张",
      attachments: [{ fileId: f.body.file.id, name: "图.png", size: bytes.length, mime: "image/png", sha256: "0".repeat(64) }],
    });
    await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body === "收到图了。");
    const input = seen[0].body.messages[1].content as string;
    expect(input).toContain("(a direct conversation with you)");
    expect(input).toContain(`[file f ${f.body.file.id} · 图.png`);
    c.close();
  });

  it("takes its 👀 back when it skips, and decides a follow-up without an @ quietly", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk([
      { action: "skip", reason: "for someone else", confidence: 0.9 },
      { action: "dispatch", reason: "work", confidence: 0.9, brief: { goal: "修设置页" } },
      { action: "skip", reason: "small talk", confidence: 0.9 },
    ]);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 这个给 Bob 看");
    const request = (await c.waitEvent((e) => e.type === "message.created" && e.data.message.topicId === topicId)).data.message;
    // 接 1.11: the 👀 it put while reading goes away on skip.
    await c.waitEvent((e) => e.type === "reaction.changed" && e.data.messageId === request.id && e.data.on === false);
    expect(c.events().filter((e) => e.type === "reaction.changed" && e.data.messageId === request.id).map((e) => e.data.on)).toEqual([true, false]);

    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 修设置页", mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // 接 1.4: the requester's message without an @ right after the answer goes to the front desk, which is told so.
    const thanks = await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "谢谢" });
    await waitUntil(() => seen.length === 3, 5000, "third decision");
    expect(seen[2].body.messages[1].content).toContain("This message does not @ you");
    await new Promise((r) => setTimeout(r, 300));
    // Small talk: no 👀 at all, no run.
    expect(c.events().some((e) => e.type === "reaction.changed" && e.data.messageId === thanks.message.id)).toBe(false);
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    d.close();
    c.close();
  });

  it("hands the requester's message without an @ to the front desk hours after the last answer", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk([
      { action: "dispatch", reason: "work", confidence: 0.9, brief: { goal: "修设置页" } },
      { action: "dispatch", reason: "continues the work", confidence: 0.9, brief: { goal: "设置页再加个开关" } },
    ]);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 修设置页");
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // 接 1.4 with a front desk: no time window; the front desk is told the message has no @.
    await endEarlier(w.bot.id, a.runId, 3 * 3600_000);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "再加个开关" });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(seen).toHaveLength(2);
    expect(seen[1].body.messages[1].content).toContain("This message does not @ you");
    expect(b.prompt).toContain("Goal: 设置页再加个开关");
    d.close();
    c.close();
  });

  it("without a front desk, takes a message without an @ as a follow-up only within the window", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 修设置页");
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    await endEarlier(w.bot.id, a.runId, FOLLOW_UP_WINDOW_MS + 60_000);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "再加个开关" });
    // Messages reach the desk in order: the next run is the @ request's, so the late one started nothing.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再看看", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt).toContain("再看看");
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
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
