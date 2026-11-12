// Memory past the prompt budget (盯 4.5, 4.9; 接 4.1): a prompt carries the conventions people
// wrote, then what is relevant to the request, then the newest; a run finds the rest with
// `GET /v1/machine/runs/:id/memories?q=` (chat-run memory search) under the same audience rules;
// a memory counts as used when a run searched it or cited its id, not when a prompt carried it.
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { EXECUTOR_MEMORY_BYTES, priority, search } from "../src/memory/inject";
import { score, terms } from "../src/memory/relevance";
import { createMemory, type MemoryRow } from "../src/memory/store";
import { Daemon, downViolations, mention, world, type World } from "./daemon";
import { BASE, Client, api, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const row = (id: string, text: string, o: Partial<MemoryRow> = {}): MemoryRow => ({
  id,
  scope: "team",
  kind: "fact",
  text,
  when_text: null,
  source: '{"kind":"manual","ids":[]}',
  note: null,
  author_id: "M1",
  human: 0,
  status: "active",
  confidence: 1,
  uses: 0,
  last_used_at: null,
  version: 1,
  created_at: 1,
  updated_at: 1,
  from_group: null,
  ...o,
});

describe("relevance", () => {
  it("uses Chinese two-character words and whole words, without filler", () => {
    expect([...terms("帮我看看部署脚本")]).toEqual(expect.arrayContaining(["部署", "脚本"]));
    expect(terms("帮我看看部署脚本").has("帮我")).toBe(false);
    expect(terms("Fix the deployScript for staging").has("staging")).toBe(true);
    expect(terms("Fix the deployScript").has("the")).toBe(false);
    expect(terms("a 我").size).toBe(0);
  });

  it("ranks rarer shared terms higher and scores no match as 0", () => {
    const rows = [row("A", "登录页面用中文"), row("B", "部署到悉尼区域前先跑 smoke 测试"), row("C", "测试要先起 mock 服务")];
    const s = Object.fromEntries(score(rows, (r) => r.text, "部署悉尼的测试").map((x) => [x.item.id, x.score]));
    expect(s.A).toBe(0);
    expect(s.B).toBeGreaterThan(s.C);
    expect(s.C).toBeGreaterThan(0);
    expect(search(rows, "部署悉尼的测试").map((r) => r.id)).toEqual(["B", "C"]);
    expect(search(rows, "?!")).toEqual([]);
  });

  it("puts people's conventions first, then relevant, then newest", () => {
    const rows = [
      row("NEW", "无关的新事实", { updated_at: 9 }),
      row("REL", "悉尼区域的数据库", { updated_at: 2 }),
      row("CONV", "提交信息只写一行", { kind: "convention", human: 1, updated_at: 1 }),
      row("AICONV", "AI 记的约定", { kind: "convention", human: 0, updated_at: 3 }),
    ];
    expect(priority(rows, "查一下悉尼").map((r) => r.id)).toEqual(["CONV", "REL", "NEW", "AICONV"]);
  });
});

async function fill(w: World, n: number): Promise<void> {
  // Unrelated facts, newer than what the tests look for: far more than 8 KB.
  for (let i = 0; i < n; i++) {
    await createMemory(
      env.DB,
      { scope: "team", kind: "fact", text: `第 ${i} 条无关的记录：办公室的咖啡机每周三清洗一次，钥匙放在前台抽屉里`, source: { kind: "manual", ids: [] }, authorId: w.owner.id, human: true, status: "active", confidence: 1 },
      w.owner.id,
      Date.now() - 1000 + i,
    );
  }
}

const runSearch = (w: World, runId: string, q: string, extra = "") =>
  SELF.fetch(`${BASE}/v1/machine/runs/${runId}/memories?q=${encodeURIComponent(q)}${extra}`, { headers: { authorization: `Bearer ${w.machineToken}` } });

describe("memory past the prompt budget", () => {
  it("carries people's conventions and what fits the request, finds the rest, and counts only real use", async () => {
    const w = await world();
    const old = Date.now() - 60_000;
    const conv = await createMemory(
      env.DB,
      { scope: "team", kind: "convention", text: "提交信息只写一行", source: { kind: "manual", ids: [] }, authorId: w.owner.id, human: true, status: "active", confidence: 1 },
      w.owner.id,
      old,
    );
    const sydney = await createMemory(
      env.DB,
      { scope: "team", kind: "fact", text: "生产数据库在悉尼区域，迁移前要先停写", source: { kind: "manual", ids: [] }, authorId: w.owner.id, human: true, status: "active", confidence: 1 },
      w.owner.id,
      old + 1,
    );
    const carol = await createMember({ name: "Carol" });
    const gid2 = await createGroup(w.owner, [carol.id, w.bot.id]);
    // Learned in another group, and another person's preference: never found here.
    const elsewhere = await createMemory(
      env.DB,
      { scope: `member:${w.bot.id}`, kind: "lesson", text: "悉尼区域的迁移脚本在另一个仓库", source: { kind: "run", ids: ["R0"] }, authorId: w.bot.id, human: false, status: "active", confidence: 0.6, fromGroup: gid2 },
      w.bot.id,
      old + 2,
    );
    const carolPref = await createMemory(
      env.DB,
      { scope: `person:${carol.id}`, kind: "preference", text: "Carol 说悉尼迁移要先通知她", source: { kind: "manual", ids: [] }, authorId: carol.id, human: true, status: "active", confidence: 1 },
      carol.id,
      old + 3,
    );
    await fill(w, 120);

    const { d } = await Daemon.connect(w.machineToken, { features: ["context"] });
    const { c } = await Client.connect(w.human.token);
    try {
      await mention(c, w, "@dev 把悉尼的生产数据库迁移一下");
      const a = await d.next((f) => f.t === "run.assign");
      const block = (a.prompt as string).slice(a.prompt.indexOf("<memory>"), a.prompt.indexOf("</memory>") + 9);
      expect(new TextEncoder().encode(block).length).toBeLessThanOrEqual(EXECUTOR_MEMORY_BYTES);
      expect(block).toContain(`[${conv.id}]`);
      expect(block).toContain(`[${sydney.id}]`);
      expect(block).toContain("chat-run memory search");
      expect(block).not.toContain(elsewhere.id);
      expect(block).not.toContain(carolPref.id);
      d.send({ t: "run.accept", runId: a.runId });

      // Being in the prompt is not a use.
      const get = async (id: string) => (await api(`/v1/memories/${id}`, { token: w.owner.token })).body;
      expect((await get(conv.id)).uses).toBe(0);

      // An old memory the prompt could not carry is still found.
      const far = await createMemory(
        env.DB,
        { scope: "team", kind: "fact", text: "旧机房的交换机密码在保险柜", source: { kind: "manual", ids: [] }, authorId: w.owner.id, human: true, status: "active", confidence: 1 },
        w.owner.id,
        old - 1,
      );
      const res = await runSearch(w, a.runId, "交换机");
      expect(res.status).toBe(200);
      const hits = ((await res.json()) as any).memories;
      expect(hits.map((m: any) => m.id)).toEqual([far.id]);
      expect(hits[0]).toMatchObject({ uses: 1, text: "旧机房的交换机密码在保险柜" });
      expect((await get(far.id)).uses).toBe(1);

      // Same audience as the prompt.
      const ids = ((await (await runSearch(w, a.runId, "悉尼")).json()) as any).memories.map((m: any) => m.id);
      expect(ids).toContain(sydney.id);
      expect(ids).not.toContain(elsewhere.id);
      expect(ids).not.toContain(carolPref.id);
      expect(((await (await runSearch(w, a.runId, "咖啡机", "&limit=3")).json()) as any).memories).toHaveLength(3);
      expect((await runSearch(w, a.runId, " ")).status).toBe(400);
      expect((await runSearch(w, a.runId, "x", "&limit=51")).status).toBe(400);
      expect((await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/memories?q=x`, { headers: { authorization: "Bearer tm_nope" } })).status).toBe(401);

      // Citing an id in the result counts a use; an id the run may not read does not.
      await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: `按 [${conv.id}] 提交；参考了 ${carolPref.id}` }));
      await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");
      let used = 0;
      for (let i = 0; i < 50 && used === 0; i++) {
        used = (await get(conv.id)).uses;
        if (used === 0) await new Promise((r) => setTimeout(r, 100));
      }
      expect(used).toBe(1);
      expect((await get(conv.id)).lastUsedAt).toBeGreaterThan(0);
      expect((await api(`/v1/memories/${carolPref.id}`, { token: carol.token })).body.uses).toBe(0);
      // The search for 悉尼 above counted it once; the prompt never did.
      expect((await get(sydney.id)).uses).toBe(1);
    } finally {
      d.close();
      c.close();
    }
  });
});
