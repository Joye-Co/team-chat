// Usable work line 1, acceptance path 5: conversations go on where people expect.
// 接 3.2: a DM's chat area continues the member's last session; 新任务 starts a fresh one.
// 接 1.5: in a chat area a second request next to a running one gets its own run.
// 接 1.8: the front desk's question is a Card v2 ask; the next message of the person answers it.
// 接 1.9 / 1.13: hand-offs run per (topic, member); a decision on an old topic is made again.
import { CardV2, mainTopicId } from "@chat/protocol";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { isNewTask } from "../src/agent/frontdesk";
import { askCard } from "../src/bots/desk/front";
import { Daemon, downViolations, world, type World } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const realFetch = globalThis.fetch;
type Answer = { d: unknown; delayMs?: number };
type Seen = { bot: string; body: any; at: number };

/** DeepSeek answers each front desk call of a member with that member's next decision. */
function fakeDesk(byBot: Record<string, Answer[]>): Seen[] {
  const seen: Seen[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
    const body = (await req.json()) as any;
    if (!body.tools) return Response.json({ choices: [{ message: { role: "assistant", content: "标题" } }] });
    if (body.tool_choice?.function?.name !== "decide") return Response.json({ choices: [{ message: { role: "assistant", content: '{"lessons":[]}' } }] });
    const system = body.messages[0].content as string;
    const bot = Object.keys(byBot).find((name) => system.includes(`front desk of ${name}`)) ?? "";
    seen.push({ bot, body, at: Date.now() });
    const a = byBot[bot]?.shift() ?? { d: { action: "skip", reason: "none", confidence: 0.9 } };
    if (a.delayMs) await new Promise((r) => setTimeout(r, a.delayMs));
    return Response.json({
      choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: "decide", arguments: JSON.stringify(a.d) } }] } }],
      usage: { prompt_tokens: 100, completion_tokens: 10 },
    });
  });
  return seen;
}

async function aiOn(token: string): Promise<void> {
  const r = await api("/v1/settings/ai", { method: "PUT", token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-test-front-desk" } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
}

/** A second runner-backed AI member in the world's group. */
async function addBot(w: World, name: string): Promise<TestMember> {
  const bot = await createMember({ kind: "bot", name });
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: bot.id } })).status).toBe(200);
  expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex" } })).status).toBe(200);
  return bot;
}

async function send(c: Client, gid: string, topicId: string | undefined, body: string, extra: Record<string, unknown> = {}) {
  const frame = { t: "send", groupId: gid, ...(topicId ? { topicId } : {}), clientMsgId: clientMsgId(), body, ...extra };
  for (;;) {
    const r = await c.request(frame);
    if (r.ok) return r.result;
    expect(r.error?.code, JSON.stringify(r)).toBe("rate_limited");
    await new Promise((x) => setTimeout(x, (r.error.retryAfterMs ?? 100) + 20));
  }
}

async function finish(d: Daemon, runId: string, sessionId: string): Promise<void> {
  d.send({ t: "run.accept", runId });
  await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "done", sessionId }));
}

describe("rules", () => {
  it("know a new piece of work by its first words", () => {
    expect(isNewTask("新任务：写周报")).toBe(true);
    expect(isNewTask("@dev 新任务 写周报")).toBe(true);
    expect(isNewTask("New task: weekly report")).toBe(true);
    expect(isNewTask("这不是新任务，接着改")).toBe(false);
    expect(isNewTask("再加一行")).toBe(false);
  });

  it("write the front desk's question as a valid card ask", () => {
    const card = askCard("是哪个页面？", "m-asker", Date.now() + 60_000);
    expect(CardV2.safeParse(card).success).toBe(true);
    expect(card).toMatchObject({ intent: "ask", title: "是哪个页面？", ask: { from: ["m-asker"], blocking: true } });
    const long = askCard(`第一行\n${"细节".repeat(300)}`, "m-asker", Date.now() + 60_000);
    expect(CardV2.safeParse(long).success).toBe(true);
    expect(long.title).toBe("第一行");
    expect(long.blocks[0]).toMatchObject({ type: "text", id: "question" });
  });
});

describe("a DM's chat area (接 3.2)", () => {
  it("continues the member's last session, starts fresh on 新任务, and runs 新任务 next to a running request", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    expect(dm.status).toBe(200);
    const gid = dm.body.group.id as string;
    const main = mainTopicId(gid);

    await send(c, gid, main, "建一个 dm-main.txt");
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.resumeSessionId).toBeUndefined();
    await finish(d, a.runId, "s-1");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // The next sentence continues the same CLI session (verify-log problem 2).
    await send(c, gid, main, "再加一行");
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBe("s-1");
    expect((await api(`/v1/runs/${b.runId}/context`, { token: w.owner.token })).body.manifest).toMatchObject({ resumed: true });
    await finish(d, b.runId, "s-2");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.status === "done");

    // 新任务 starts a fresh session.
    await send(c, gid, main, "新任务：写周报");
    const x = await d.next((f) => f.t === "run.assign" && ![a.runId, b.runId].includes(f.runId));
    expect(x.resumeSessionId).toBeUndefined();
    d.send({ t: "run.accept", runId: x.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === x.runId && e.data.run.status === "running");

    // While it runs, more words go to it; 新任务 gets its own run next to it.
    await send(c, gid, main, "周报加上数据");
    const input = await d.next((f) => f.t === "run.input");
    expect(input).toMatchObject({ runId: x.runId, text: "周报加上数据" });
    await send(c, gid, main, "新任务 看下 CI");
    const y = await d.next((f) => f.t === "run.assign" && ![a.runId, b.runId, x.runId].includes(f.runId));
    expect(y.resumeSessionId).toBeUndefined();
    d.send({ t: "run.accept", runId: y.runId });
    await d.acked(x.runId, d.done(x.runId, { status: "succeeded", resultText: "ok" }));
    await d.acked(y.runId, d.done(y.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });
});

describe("the 群聊 (接 1.5)", () => {
  it("runs another person's request next to a running one; each person's follow-ups reach their own run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c: alice } = await Client.connect(w.human.token);
    const { c: joye } = await Client.connect(w.owner.token);
    const main = mainTopicId(w.gid);

    await send(alice, w.gid, main, "@dev 看下 CI", { mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await alice.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");

    // Another person's request: its own run, its own session, not input for Alice's run.
    await send(joye, w.gid, main, "@dev 写一下周报", { mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBeUndefined();
    expect(b.prompt).toContain("写一下周报");
    d.send({ t: "run.accept", runId: b.runId });
    expect(d.frames.some((f) => f.t === "run.input")).toBe(false);

    // Each person's next words go to their own run.
    await send(alice, w.gid, main, "@dev 顺便看下 lint", { mentions: [w.bot.id] });
    await d.next((f) => f.t === "run.input" && f.runId === a.runId && f.text === "@dev 顺便看下 lint");
    await send(joye, w.gid, main, "@dev 加上数据", { mentions: [w.bot.id] });
    await d.next((f) => f.t === "run.input" && f.runId === b.runId && f.text === "@dev 加上数据");

    // A reply to Alice's request from Joye goes to Alice's run.
    const request = alice.events().find((e) => e.type === "message.created" && e.data.message.body === "@dev 看下 CI").data.message;
    await send(joye, w.gid, main, "@dev 这个先跑一下 e2e", { mentions: [w.bot.id], replyTo: { messageId: request.id } });
    await d.next((f) => f.t === "run.input" && f.runId === a.runId && f.text.startsWith("@dev 这个先跑一下 e2e"));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);

    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    alice.close();
    joye.close();
  });
});

describe("the front desk's question (接 1.8)", () => {
  it("is a card ask in the 群聊 and the person's next message without an @ answers it", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk({
      dev: [
        { d: { action: "ask", reason: "which page", confidence: 0.9, reply: "是哪个页面？" } },
        { d: { action: "dispatch", reason: "clear now", confidence: 0.9, brief: { goal: "修设置页" } } },
      ],
    });
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const main = mainTopicId(w.gid);
    await send(c, w.gid, main, "@dev 页面坏了", { mentions: [w.bot.id] });
    const asked = (await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id)).data.message;
    expect(asked.body).toBe("是哪个页面？");
    expect(asked.cardV2).toMatchObject({ intent: "ask", ask: { from: [w.human.id] } });
    expect(asked.cardState.ask).toMatchObject({ status: "open", from: [w.human.id] });

    // No @: it reaches the member because its card waits on this person.
    await send(c, w.gid, main, "设置页");
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("Goal: 修设置页");
    expect(seen).toHaveLength(2);
    // The card stops waiting.
    await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === asked.id && e.data.cardState?.ask?.status !== "open");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));

    // The question was answered: another plain message reaches nobody.
    await send(c, w.gid, main, "谢谢");
    await new Promise((r) => setTimeout(r, 300));
    expect(seen).toHaveLength(2);
    d.close();
    c.close();
  });

  it("makes a topic wait for the answer, and an answer in the card starts the work", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    fakeDesk({ dev: [{ d: { action: "ask", reason: "which page", confidence: 0.9, reply: "是哪个页面？" } }] });
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    const first = await send(c, w.gid, undefined, "@dev 页面坏了", { mentions: [w.bot.id] });
    const topicId = first.message.topicId as string;
    const asked = (await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === topicId)).data.message;
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "waiting_answer" && e.data.waitingOn?.includes(w.human.id));

    await c.ok({ t: "card.answer", groupId: w.gid, messageId: asked.id, rev: 1, actionId: "send", inputs: { answer: "设置页" } });
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("设置页");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });
});

describe("hand-offs (接 1.9, 1.13)", () => {
  it("one member's slow front desk does not hold back another member", async () => {
    const w = await world();
    const ops = await addBot(w, "ops");
    await aiOn(w.owner.token);
    fakeDesk({
      // The message to ops came in while dev thought: dev decides again (接 1.13).
      dev: [
        { d: { action: "reply", reason: "hi", confidence: 0.9, reply: "dev 在" }, delayMs: 2500 },
        { d: { action: "reply", reason: "hi", confidence: 0.9, reply: "dev 在" } },
      ],
      ops: [{ d: { action: "reply", reason: "hi", confidence: 0.9, reply: "ops 在" } }],
    });
    const { c } = await Client.connect(w.human.token);
    const main = mainTopicId(w.gid);
    await send(c, w.gid, main, "@dev 你在吗", { mentions: [w.bot.id] });
    await send(c, w.gid, main, "@ops 你在吗", { mentions: [ops.id] });
    const opsReply = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === ops.id, 2000);
    expect(opsReply.data.message.body).toBe("ops 在");
    // dev is still thinking.
    expect(c.events().some((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id)).toBe(false);
    await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id, 5000);
    c.close();
  });

  it("decides again when the topic changed while the front desk thought", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    const seen = fakeDesk({
      dev: [
        { d: { action: "reply", reason: "old", confidence: 0.9, reply: "旧的回答" }, delayMs: 1000 },
        { d: { action: "reply", reason: "new", confidence: 0.9, reply: "新的回答" } },
      ],
    });
    const { c } = await Client.connect(w.human.token);
    const { c: joye } = await Client.connect(w.owner.token);
    const main = mainTopicId(w.gid);
    await send(c, w.gid, main, "@dev 登录页是不是坏了", { mentions: [w.bot.id] });
    await waitUntil(() => seen.length === 1, 3000, "first decision");
    await send(joye, w.gid, main, "我刚修好了登录页");
    const reply = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id, 5000);
    expect(reply.data.message.body).toBe("新的回答");
    expect(seen).toHaveLength(2);
    expect(seen[1].body.messages[1].content).toContain("我刚修好了登录页");
    const list = await api(`/v1/bots/${w.bot.id}/decisions?groupId=${w.gid}&topicId=${main}`, { token: w.owner.token });
    expect(list.body.decisions).toHaveLength(1);
    expect(list.body.decisions[0].reason).toBe("new");
    c.close();
    joye.close();
  });
});
