// Who reads what an AI member learned (docs/protocol.md "Memory"): a memory of an AI member's own
// scope records the group it came from (`fromGroup`), so what a run notes in one group does not
// reach the member's runs in other groups, nor people outside that group, until a person
// releases it; a direct conversation's lessons stay between the person and the member; a group
// convention needs a person's message; the daily consolidation never widens the audience by itself
// and retries a failed scope the next hour.
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { consolidateAll, localClock, teamZone } from "../src/memory/consolidate";
import { readableMemories } from "../src/memory/inject";
import { createMemory } from "../src/memory/store";
import { Daemon, downViolations, mention, world, type World } from "./daemon";
import { BASE, Client, api, clientMsgId, createGroup, createMember, protocolViolations, type TestMember } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const SECRET = "收购对象是 Acme，周五前不要对外说";

/** Group 1 (Joye, Alice, dev): a run notes SECRET as the member's lesson. Group 2 (Joye, Carol, dev) is made. */
async function noted(): Promise<{ w: World; carol: TestMember; gid2: string; d: Daemon; firstRun: string; noteId: string; close: () => void }> {
  const w = await world();
  const carol = await createMember({ name: "Carol" });
  const gid2 = await createGroup(w.owner, [carol.id, w.bot.id]);
  const { d } = await Daemon.connect(w.machineToken, {});
  const { c: alice } = await Client.connect(w.human.token);
  await mention(alice, w, "@dev 准备收购的材料");
  const a = await d.next((f) => f.t === "run.assign");
  d.send({ t: "run.accept", runId: a.runId });
  const note = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/memories`, {
    method: "POST",
    headers: { authorization: `Bearer ${w.machineToken}`, "content-type": "application/json" },
    body: JSON.stringify({ scope: "member", kind: "lesson", text: SECRET }),
  });
  expect(note.status).toBe(200);
  const item = (await note.json()) as { id: string; fromGroup?: string };
  expect(item.fromGroup).toBe(w.gid);
  await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "好了" }));
  await alice.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");
  // Carol is not in group 1 and cannot read its memory.
  expect((await api(`/v1/memories?scope=${encodeURIComponent(`group:${w.gid}`)}`, { token: carol.token })).status).toBe(404);
  return { w, carol, gid2, d, firstRun: a.runId, noteId: item.id, close: () => (d.close(), alice.close()) };
}

const memberList = (w: World, token: string) => api(`/v1/memories?scope=${encodeURIComponent(`member:${w.bot.id}`)}&status=all`, { token });
const texts = (res: { body: any }) => res.body.memories.map((m: any) => m.text);

describe("memory across groups", () => {
  it("does not show a lesson noted in one group to a person outside that group", async () => {
    const s = await noted();
    try {
      const seen = await api(`/v1/memories?scope=${encodeURIComponent(`member:${s.w.bot.id}`)}&status=all`, { token: s.carol.token });
      expect(seen.body.memories.map((m: any) => m.text)).not.toContain(SECRET);
      expect((await api(`/v1/memories/${s.noteId}`, { token: s.carol.token })).status).toBe(404);
      expect((await api(`/v1/memories/${s.noteId}/versions`, { token: s.carol.token })).status).toBe(404);
      // People in the group read it.
      expect(texts(await memberList(s.w, s.w.human.token))).toContain(SECRET);
    } finally {
      s.close();
    }
  });

  it("does not put a lesson noted in one group into a run for a person outside that group", async () => {
    const s = await noted();
    const { c: carolWs } = await Client.connect(s.carol.token);
    try {
      await carolWs.ok({ t: "send", groupId: s.gid2, clientMsgId: clientMsgId(), body: "@dev 最近在忙什么", mentions: [s.w.bot.id] });
      const b = await s.d.next((f) => f.t === "run.assign" && f.runId !== s.firstRun);
      expect(b.prompt).not.toContain(SECRET);
    } finally {
      s.close();
      carolWs.close();
    }
  });

  it("puts it into a run asked by a person of that group, also in another group (design-visibility 3.5)", async () => {
    const s = await noted();
    const { c: joye } = await Client.connect(s.w.owner.token);
    try {
      // Joye is in group 1: a run working for Joye may read it, wherever it runs.
      await joye.ok({ t: "send", groupId: s.gid2, clientMsgId: clientMsgId(), body: "@dev 总结一下", mentions: [s.w.bot.id] });
      const b = await s.d.next((f) => f.t === "run.assign" && f.runId !== s.firstRun);
      expect(b.prompt).toContain(SECRET);
    } finally {
      s.close();
      joye.close();
    }
  });

  it("shows it everywhere once a person of that group releases it", async () => {
    const s = await noted();
    const { c: carolWs } = await Client.connect(s.carol.token);
    try {
      expect((await api(`/v1/memories/${s.noteId}/release`, { method: "POST", token: s.carol.token })).status).toBe(404);
      // Alice reads it but cannot change the AI member's memory.
      expect((await api(`/v1/memories/${s.noteId}/release`, { method: "POST", token: s.w.human.token })).status).toBe(403);
      const released = await api(`/v1/memories/${s.noteId}/release`, { method: "POST", token: s.w.owner.token });
      expect(released.status, JSON.stringify(released.body)).toBe(200);
      expect(released.body.fromGroup).toBeUndefined();
      expect(released.body.version).toBe(2);
      expect(texts(await memberList(s.w, s.carol.token))).toContain(SECRET);
      await carolWs.ok({ t: "send", groupId: s.gid2, clientMsgId: clientMsgId(), body: "@dev 最近在忙什么", mentions: [s.w.bot.id] });
      const b = await s.d.next((f) => f.t === "run.assign" && f.runId !== s.firstRun);
      expect(b.prompt).toContain(SECRET);
    } finally {
      s.close();
      carolWs.close();
    }
  });

  it("keeps what a run learns in a direct conversation between that person and the member", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    expect(dm.status).toBe(200);
    const { c } = await Client.connect(w.human.token);
    try {
      await c.ok({ t: "send", groupId: dm.body.group.id, clientMsgId: clientMsgId(), body: "我下个月要离职，先别说" });
      const a = await d.next((f) => f.t === "run.assign");
      d.send({ t: "run.accept", runId: a.runId });
      const note = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/memories`, {
        method: "POST",
        headers: { authorization: `Bearer ${w.machineToken}`, "content-type": "application/json" },
        body: JSON.stringify({ scope: "member", kind: "lesson", text: "Alice 下个月离职" }),
      });
      expect(((await note.json()) as any).fromGroup).toBe(dm.body.group.id);
      await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "知道了" }));
      await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");
      expect(texts(await memberList(w, w.human.token))).toContain("Alice 下个月离职");
      // The owner is not in the direct conversation.
      expect(texts(await memberList(w, w.owner.token))).not.toContain("Alice 下个月离职");
      // Nor does it go into Alice's own run in group 1, which others read.
      await mention(c, w, "@dev 排一下下个月的值班");
      const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
      expect(b.prompt).not.toContain("Alice 下个月离职");
    } finally {
      d.close();
      c.close();
    }
  });

  it("keeps a memory noted before groups were recorded for owners only, out of prompts, until released", async () => {
    const w = await world();
    const old = await createMemory(
      env.DB,
      { scope: `member:${w.bot.id}`, kind: "lesson", text: "旧的经验，不知道是哪个群的", source: { kind: "run", ids: ["01OLDRUN000000000000000000"] }, authorId: w.bot.id, human: false, status: "active", confidence: 0.5, fromGroup: "unknown" },
      w.bot.id,
    );
    expect(old.fromGroup).toBe("unknown");
    expect(texts(await memberList(w, w.human.token))).not.toContain(old.text);
    expect(texts(await memberList(w, w.owner.token))).toContain(old.text);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    try {
      await mention(c, w, "@dev 看一下");
      const a = await d.next((f) => f.t === "run.assign");
      expect(a.prompt).not.toContain(old.text);
      expect((await api(`/v1/memories/${old.id}/release`, { method: "POST", token: w.owner.token })).status).toBe(200);
      expect(texts(await memberList(w, w.human.token))).toContain(old.text);
    } finally {
      d.close();
      c.close();
    }
  });
});

const DAY = 24 * 60 * 60_000;
const HOUR = 60 * 60_000;
/** Today's 04:00 in the team's zone, `days` later. */
const after3 = (days: number) => localClock(Date.now(), teamZone(env)).cutoff + HOUR + days * DAY;

async function aiOn(token: string): Promise<void> {
  const r = await api("/v1/settings/ai", { method: "PUT", token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-test-memory-0000" } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
}
async function aiOff(token: string): Promise<void> {
  await api("/v1/settings/ai", { method: "DELETE", token });
}

const realFetch = globalThis.fetch;
type Call = { system: string; prompt: string };

/** DeepSeek's forced tool call answered by `answer`; `null` answers with a server error. */
function fakeModel(answer: (c: Call) => unknown): Call[] {
  const seen: Call[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
    const body = (await req.json()) as any;
    const c = { system: body.messages[0].content as string, prompt: body.messages[1].content as string };
    seen.push(c);
    const args = answer(c);
    if (args === null) return new Response("overloaded", { status: 500 });
    return Response.json({
      choices: [{ message: { role: "assistant", content: null, tool_calls: [{ type: "function", function: { name: body.tool_choice.function.name, arguments: JSON.stringify(args) } }] } }],
      usage: { prompt_tokens: 100, completion_tokens: 10 },
    });
  });
  return seen;
}

describe("daily consolidation and a memory's group", () => {
  it("does not merge across groups, show other groups' lessons, or promote a group's lesson by itself", async () => {
    const w = await world();
    const carol = await createMember({ name: "Carol" });
    const bot2 = await createMember({ kind: "bot", name: "ops" });
    const a = `member:${w.bot.id}`;
    const b = `member:${bot2.id}`;
    const base = { kind: "lesson" as const, source: { kind: "run" as const, ids: [] }, human: false, confidence: 0.5 };
    const open = await createMemory(env.DB, { ...base, scope: a, text: "提交前跑 lint", authorId: w.bot.id, status: "active" }, w.bot.id);
    const fromGroup = await createMemory(env.DB, { ...base, scope: a, text: "提交前跑 lint 和群里的私有检查", authorId: w.bot.id, status: "pending", fromGroup: w.gid }, w.bot.id);
    const shared = await createMemory(env.DB, { ...base, scope: b, text: "PR 要附测试结果", authorId: bot2.id, status: "pending" }, bot2.id);
    const otherGroup = await createMemory(env.DB, { ...base, scope: b, text: "ops 在别的群学到的秘密", authorId: bot2.id, status: "pending", fromGroup: "01OTHERGROUP00000000000000" }, bot2.id);
    await aiOn(w.owner.token);
    try {
      const seen = fakeModel((c) => {
        if (!c.system.includes(`Scope: ${a}`)) return { ops: [] };
        return {
          ops: [
            { op: "merge", id: fromGroup.id, into: open.id, text: "提交前跑 lint 和群里的私有检查" },
            { op: "promote", sources: [fromGroup.id, shared.id], text: "提交前都要跑检查" },
          ],
        };
      });
      const now = after3(5);
      const { day } = localClock(now, teamZone(env));
      const res = await consolidateAll(env, { now });
      expect(res.scopes).toEqual(expect.arrayContaining([a, b]));
      const call = seen.find((x) => x.system.includes(`Scope: ${a}`)) as Call;
      expect(call.prompt).toContain(shared.text);
      expect(call.prompt).not.toContain(otherGroup.text);

      const get = async (id: string) => (await api(`/v1/memories/${id}`, { token: w.owner.token })).body;
      // Not merged: the open memory keeps its text, the group's note became active as it was, still of its group.
      expect(await get(open.id)).toMatchObject({ text: "提交前跑 lint", version: 1 });
      expect(await get(fromGroup.id)).toMatchObject({ status: "active", text: fromGroup.text, fromGroup: w.gid });
      // Not promoted: a flagged line for a person of the group to decide.
      expect(texts(await api("/v1/memories?scope=team", { token: w.owner.token }))).not.toContain("提交前都要跑检查");
      const lines = (await api(`/v1/memories/updates?scope=${encodeURIComponent(a)}&day=${day}`, { token: w.human.token })).body.updates;
      const held = lines.find((u: any) => u.change === "flagged" && u.memoryId === fromGroup.id);
      expect(held.note).toContain("提交前都要跑检查");
      // Carol is not in the group: neither the memory nor its lines.
      const carolLines = (await api(`/v1/memories/updates?scope=${encodeURIComponent(a)}&day=${day}`, { token: carol.token })).body.updates;
      expect(carolLines.some((u: any) => u.memoryId === fromGroup.id)).toBe(false);
      expect((await api(`/v1/memories/updates/${held.id}/undo`, { method: "POST", token: carol.token })).status).toBe(404);
    } finally {
      await aiOff(w.owner.token);
    }
  });

  it("names only the sources when a held promotion mixes groups", async () => {
    const w = await world();
    const bot2 = await createMember({ kind: "bot", name: "ops2" });
    const a = `member:${w.bot.id}`;
    const base = { kind: "lesson" as const, source: { kind: "run" as const, ids: [] }, human: false, confidence: 0.5 };
    const here = await createMemory(env.DB, { ...base, scope: a, text: "本群的经验", authorId: w.bot.id, status: "pending", fromGroup: w.gid }, w.bot.id);
    const there = await createMemory(env.DB, { ...base, scope: a, text: "别的群的经验", authorId: w.bot.id, status: "pending", fromGroup: "01OTHERGROUP00000000000001" }, w.bot.id);
    const shared = await createMemory(env.DB, { ...base, scope: `member:${bot2.id}`, text: "公开的经验", authorId: bot2.id, status: "pending" }, bot2.id);
    await aiOn(w.owner.token);
    try {
      fakeModel((c) => (c.system.includes(`Scope: ${a}`) ? { ops: [{ op: "promote", sources: [here.id, there.id, shared.id], text: "合并后的别的群的经验" }] } : { ops: [] }));
      const now = after3(6);
      const { day } = localClock(now, teamZone(env));
      await consolidateAll(env, { now });
      const lines = (await api(`/v1/memories/updates?scope=${encodeURIComponent(a)}&day=${day}`, { token: w.owner.token })).body.updates;
      const held = lines.find((u: any) => u.change === "flagged");
      expect(held.note).not.toContain("合并后的别的群的经验");
      expect(held.note).toContain(there.id);
      expect(texts(await api("/v1/memories?scope=team", { token: w.owner.token }))).not.toContain("合并后的别的群的经验");
    } finally {
      await aiOff(w.owner.token);
    }
  });

  it("tries a scope again the next hour after the model failed", async () => {
    const w = await world();
    const scope = `member:${w.bot.id}`;
    const p = await createMemory(env.DB, { scope, kind: "lesson", text: "先看日志", source: { kind: "run", ids: [] }, authorId: w.bot.id, human: false, status: "pending", confidence: 0.5 }, w.bot.id);
    await aiOn(w.owner.token);
    try {
      let fail = true;
      fakeModel(() => (fail ? null : { ops: [{ op: "add", id: p.id }] }));
      const now = after3(7);
      expect((await consolidateAll(env, { now })).scopes).not.toContain(scope);
      expect((await api(`/v1/memories/${p.id}`, { token: w.owner.token })).body.status).toBe("pending");
      fail = false;
      expect((await consolidateAll(env, { now: now + HOUR })).scopes).toContain(scope);
      expect((await api(`/v1/memories/${p.id}`, { token: w.owner.token })).body.status).toBe("active");
      // Once done, not again that day.
      expect((await consolidateAll(env, { now: now + 2 * HOUR })).scopes).not.toContain(scope);
    } finally {
      await aiOff(w.owner.token);
    }
  });
});

// design-visibility 3.5: a run reads the memory of the groups the person it works for may read.
// P (Joye) is in G1 and G2, Q (Alice) only in G1; the AI member is in G1, not in G2; P and the
// member also have a direct conversation.
describe("memory a run reads for its person", () => {
  const MEM_G2 = "在产品群学到：导出按 CSV 做";
  const MEM_DM = "私聊里学到：Joye 的口令是 tiger";
  const CONV_G2 = "产品群约定：导出格式先做 xlsx";
  const PREF = "Joye 喜欢简短的导出说明";

  async function reach() {
    const w = await world();
    const carol = await createMember({ name: "Carol" });
    const g2 = await createGroup(w.owner, [carol.id], "product");
    const dm = await api("/v1/dms", { token: w.owner.token, body: { memberId: w.bot.id } });
    expect(dm.status).toBe(200);
    const dmId = dm.body.group.id as string;
    const base = { source: { kind: "run" as const, ids: [] }, confidence: 0.6, status: "active" as const };
    const memG2 = await createMemory(env.DB, { ...base, scope: `member:${w.bot.id}`, kind: "lesson", text: MEM_G2, authorId: w.bot.id, human: false, fromGroup: g2 }, w.bot.id);
    const memDm = await createMemory(env.DB, { ...base, scope: `member:${w.bot.id}`, kind: "lesson", text: MEM_DM, authorId: w.bot.id, human: false, fromGroup: dmId }, w.bot.id);
    const conv = await createMemory(env.DB, { ...base, scope: `group:${g2}`, kind: "convention", text: CONV_G2, authorId: w.owner.id, human: true }, w.owner.id);
    const pref = await createMemory(env.DB, { ...base, scope: `person:${w.owner.id}`, kind: "preference", text: PREF, authorId: w.owner.id, human: true }, w.owner.id);
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c: joye } = await Client.connect(w.owner.token);
    const { c: alice } = await Client.connect(w.human.token);
    return { w, g2, dmId, carol, memG2, memDm, conv, pref, d, joye, alice, close: () => (d.close(), joye.close(), alice.close()) };
  }
  type Reach = Awaited<ReturnType<typeof reach>>;

  const machine = (s: Reach, path: string, init: { method?: string; body?: unknown } = {}) =>
    SELF.fetch(`${BASE}/v1/machine/runs/${path}`, {
      method: init.method ?? "GET",
      headers: { authorization: `Bearer ${s.w.machineToken}`, "content-type": "application/json" },
      ...(init.body ? { body: JSON.stringify(init.body) } : {}),
    });
  /** A run asked by `c` in G1, accepted; its id and prompt. */
  async function runOf(s: Reach, c: Client, body: string, prev: string[] = []): Promise<{ runId: string; prompt: string }> {
    await c.ok({ t: "send", groupId: s.w.gid, clientMsgId: clientMsgId(), body: `@dev ${body}`, mentions: [s.w.bot.id] });
    const a = await s.d.next((f) => f.t === "run.assign" && !prev.includes(f.runId));
    s.d.send({ t: "run.accept", runId: a.runId });
    return { runId: a.runId, prompt: a.prompt as string };
  }
  async function finish(s: Reach, runId: string): Promise<void> {
    await s.d.acked(runId, s.d.done(runId, { status: "succeeded", resultText: "好了" }));
  }
  const searchTexts = async (s: Reach, runId: string, q: string): Promise<string[]> => {
    const r = await machine(s, `${runId}/memories?q=${encodeURIComponent(q)}&limit=50`);
    expect(r.status).toBe(200);
    return ((await r.json()) as any).memories.map((m: any) => m.text);
  };
  const desk = (s: Reach) => (env as unknown as BotsEnv).BOT_DESK.get((env as unknown as BotsEnv).BOT_DESK.idFromName(s.w.bot.id));
  const setRun = (s: Reach, runId: string, by: string, forId: string | null) =>
    runInDurableObject(desk(s), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET requested_by = ?, requested_for = ? WHERE id = ?", by, forId, runId);
    });

  it("puts another group's lesson into a run for a person of that group, never a direct conversation's", async () => {
    const s = await reach();
    try {
      const p = await runOf(s, s.joye, "导出怎么做");
      expect(p.prompt).toContain(MEM_G2);
      expect(p.prompt).toContain(PREF);
      expect(p.prompt).not.toContain(MEM_DM);
      // The prompt carries only this group's group layer; the search finds the others.
      expect(p.prompt).not.toContain(CONV_G2);
      await finish(s, p.runId);
      const q = await runOf(s, s.alice, "导出怎么做", [p.runId]);
      expect(q.prompt).not.toContain(MEM_G2);
      expect(q.prompt).not.toContain(MEM_DM);
      expect(q.prompt).not.toContain(PREF);
      await finish(s, q.runId);
    } finally {
      s.close();
    }
  });

  it("finds another readable group's convention with memory search", async () => {
    const s = await reach();
    try {
      const p = await runOf(s, s.joye, "导出");
      const found = await searchTexts(s, p.runId, "导出");
      expect(found).toEqual(expect.arrayContaining([CONV_G2, MEM_G2, PREF]));
      expect(found).not.toContain(MEM_DM);
      await finish(s, p.runId);
      const q = await runOf(s, s.alice, "导出", [p.runId]);
      const notFound = await searchTexts(s, q.runId, "导出");
      expect(notFound).not.toContain(CONV_G2);
      expect(notFound).not.toContain(MEM_G2);
      expect(notFound).not.toContain(PREF);
      await finish(s, q.runId);
    } finally {
      s.close();
    }
  });

  it("reads cited ids under the same rule", async () => {
    const s = await reach();
    try {
      const ids = [s.memG2.id, s.memDm.id, s.conv.id, s.pref.id];
      const forP = await readableMemories(env.DB, { botId: s.w.bot.id, groupId: s.w.gid, requesterId: s.w.owner.id, groups: [s.w.gid, s.g2] }, ids);
      expect(forP.map((r) => r.id).sort()).toEqual([s.memG2.id, s.pref.id].sort());
      const search = await readableMemories(env.DB, { botId: s.w.bot.id, groupId: s.w.gid, requesterId: s.w.owner.id, groups: [s.w.gid, s.g2], searchGroups: true }, ids);
      expect(search.map((r) => r.id).sort()).toEqual([s.memG2.id, s.conv.id, s.pref.id].sort());
      const forQ = await readableMemories(env.DB, { botId: s.w.bot.id, groupId: s.w.gid, requesterId: s.w.human.id, groups: [s.w.gid] }, ids);
      expect(forQ).toEqual([]);
    } finally {
      s.close();
    }
  });

  it("reads and writes the person layer of whom a handed-over run works for", async () => {
    const s = await reach();
    const other = await createMember({ kind: "bot", name: "ops" });
    try {
      const r = await runOf(s, s.alice, "导出");
      // As after a hand-over: another AI member asked, for Joye.
      await setRun(s, r.runId, other.id, s.w.owner.id);
      expect(await searchTexts(s, r.runId, "导出")).toEqual(expect.arrayContaining([PREF, MEM_G2]));
      const note = await machine(s, `${r.runId}/memories`, { method: "POST", body: { scope: "person", kind: "preference", text: "Joye 要 CSV" } });
      expect(note.status).toBe(200);
      const item = (await note.json()) as any;
      expect(item.scope).toBe(`person:${s.w.owner.id}`);
      expect(item.fromGroup).toBeUndefined();
      await finish(s, r.runId);
    } finally {
      s.close();
    }
  });

  it("keeps what a run learned after reading another group with that group", async () => {
    const s = await reach();
    try {
      const p = await runOf(s, s.joye, "按产品群的需求做导出");
      expect((await machine(s, `${p.runId}/members?group=${s.g2}`)).status).toBe(200);
      const note = await machine(s, `${p.runId}/memories`, { method: "POST", body: { scope: "member", kind: "lesson", text: "产品群要求导出带表头" } });
      expect(note.status).toBe(200);
      expect(((await note.json()) as any).fromGroup).toBe(s.g2);
      const person = await machine(s, `${p.runId}/memories`, { method: "POST", body: { scope: "person", kind: "preference", text: "Joye 要表头" } });
      expect(((await person.json()) as any).fromGroup).toBe(s.g2);
      const group = await machine(s, `${p.runId}/memories`, { method: "POST", body: { scope: "group", kind: "convention", text: "导出带表头" } });
      expect(group.status).toBe(403);
      expect(((await group.json()) as any).error).toMatchObject({ code: "forbidden", reason: "memory.read_other_group" });
      // A second other group: the source is unknown.
      const g3 = await createGroup(s.w.owner, [], "ops");
      expect((await machine(s, `${p.runId}/members?group=${g3}`)).status).toBe(200);
      const two = await machine(s, `${p.runId}/memories`, { method: "POST", body: { scope: "member", kind: "lesson", text: "两个群都看过" } });
      expect(((await two.json()) as any).fromGroup).toBe("unknown");
      await finish(s, p.runId);
      const q = await runOf(s, s.alice, "导出", [p.runId]);
      expect(q.prompt).not.toContain("产品群要求导出带表头");
      expect(q.prompt).not.toContain("两个群都看过");
      await finish(s, q.runId);
    } finally {
      s.close();
    }
  });

  it("gives a private card's answer run and a hook's run only their own group's memory", async () => {
    const s = await reach();
    try {
      const p = await runOf(s, s.joye, "导出");
      await setRun(s, p.runId, "anonymous", s.w.owner.id);
      const anon = await searchTexts(s, p.runId, "导出");
      expect(anon).not.toContain(MEM_G2);
      expect(anon).not.toContain(CONV_G2);
      expect(anon).not.toContain(PREF);
      expect((await machine(s, `${p.runId}/memories`, { method: "POST", body: { scope: "person", kind: "preference", text: "x" } })).status).toBe(400);
      // A hook's run works for nobody.
      await setRun(s, p.runId, "integration:hook", null);
      const hook = await searchTexts(s, p.runId, "导出");
      expect(hook).not.toContain(MEM_G2);
      expect(hook).not.toContain(CONV_G2);
      await finish(s, p.runId);
    } finally {
      s.close();
    }
  });
});
