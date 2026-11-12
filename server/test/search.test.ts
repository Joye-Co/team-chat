import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { runMigrations } from "../src/core/migrations";
import { type GroupRoom, roomMigrationSteps } from "../src/core/room";
import { Client, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(() => {
  expect(protocolViolations).toEqual([]);
});

async function post(c: Client, groupId: string, body: string, extra: Record<string, unknown> = {}) {
  return c.ok({ t: "send", groupId, clientMsgId: clientMsgId(), body, ...extra });
}

async function search(c: Client, frame: Record<string, unknown>) {
  const r = await c.request({ t: "search", ...frame });
  expect(r.t, JSON.stringify(r)).toBe("searchResults");
  return r.hits as any[];
}

async function setup() {
  const a = await createMember({ name: "A" });
  const b = await createMember({ name: "B" });
  const outsider = await createMember({ name: "O" });
  const gid = await createGroup(a, [b.id], "search");
  const { c: ca } = await Client.connect(a.token);
  const { c: cb } = await Client.connect(b.token);
  const { c: co } = await Client.connect(outsider.token);
  return { a, b, outsider, gid, ca, cb, co };
}

describe("search", () => {
  afterAll(() => new Promise((r) => setTimeout(r, 300)));

  it("finds Chinese two-character words, mixed text and code identifiers", async () => {
    const { gid, ca, cb, a } = await setup();
    const s1 = await post(ca, gid, "今天下午我们部署新版本到生产环境");
    const topicId = s1.topic.id;
    await post(ca, gid, "v2 部署到 prod 之前先跑测试", { topicId });
    await post(ca, gid, "the bug is in parseJSONData and user_id handling", { topicId });
    await post(ca, gid, "完全无关的内容", { topicId });

    const hits = await search(cb, { query: "部署", groupId: gid });
    expect(hits.map((h) => h.body ?? h.snippet).length).toBe(2);
    expect(hits[0].snippet).toContain("部署");
    expect(hits[0]).toMatchObject({ groupId: gid, topicId, authorId: a.id });
    // Newest first.
    expect(hits[0].seq).toBeGreaterThan(hits[1].seq);
    // Highlights are code-point ranges on the snippet.
    const h = hits[0].highlights[0];
    expect([...hits[0].snippet].slice(h.start, h.start + h.length).join("")).toBe("部署");

    expect((await search(cb, { query: "部署 prod", groupId: gid })).map((x) => x.snippet)).toEqual([
      expect.stringContaining("prod"),
    ]);
    expect(await search(cb, { query: "生产环境" })).toHaveLength(1);
    expect(await search(cb, { query: "上线" })).toHaveLength(0);
    expect(await search(cb, { query: "parseJSONData" })).toHaveLength(1);
    expect(await search(cb, { query: "json" })).toHaveLength(1);
    expect(await search(cb, { query: "user_id" })).toHaveLength(1);
    // Prefix of the last word (search as you type).
    expect(await search(cb, { query: "pars" })).toHaveLength(1);
    // A single Chinese character.
    expect(await search(cb, { query: "署" })).toHaveLength(2);
  });

  it("finds topic titles and filters by topic", async () => {
    const { gid, ca, cb } = await setup();
    const t1 = await post(ca, gid, "第一条", { title: "季度规划会议" });
    const t2 = await post(ca, gid, "规划第二个话题");
    const hits = await search(cb, { query: "规划" });
    expect(hits).toHaveLength(2);
    const titleHit = hits.find((h) => h.topicId === t1.topic.id);
    expect(titleHit).toMatchObject({ messageId: t1.message.id, snippet: "季度规划会议" });
    expect(await search(cb, { query: "规划", groupId: gid, topicId: t2.topic.id })).toHaveLength(1);

    await cb.ok({ t: "topic.update", groupId: gid, topicId: t2.topic.id, title: "预算审核" });
    expect((await search(cb, { query: "预算" })).map((h) => h.topicId)).toEqual([t2.topic.id]);

    const bad = await cb.request({ t: "search", query: "规划", topicId: t1.topic.id });
    expect(bad).toMatchObject({ t: "ack", ok: false, error: { code: "invalid" } });
  });

  it("follows edits and deletes", async () => {
    const { gid, ca, cb } = await setup();
    const s = await post(ca, gid, "原来的内容 alpha", { title: "编辑测试" });
    await post(ca, gid, "另一条 alpha", { topicId: s.topic.id });
    expect(await search(cb, { query: "alpha" })).toHaveLength(2);

    await ca.ok({ t: "edit", groupId: gid, messageId: s.message.id, body: "修改以后 beta" });
    expect(await search(cb, { query: "原来" })).toHaveLength(0);
    expect((await search(cb, { query: "beta" })).map((h) => h.messageId)).toEqual([s.message.id]);
    expect(await search(cb, { query: "alpha" })).toHaveLength(1);

    const s2 = await post(ca, gid, "要删除的 gamma", { topicId: s.topic.id });
    expect(await search(cb, { query: "gamma" })).toHaveLength(1);
    await ca.ok({ t: "delete", groupId: gid, messageId: s2.message.id });
    expect(await search(cb, { query: "gamma" })).toHaveLength(0);
  });

  it("returns only groups the member belongs to", async () => {
    const { gid, ca, co, outsider, a } = await setup();
    await post(ca, gid, "机密 delta");
    const ownGid = await createGroup(outsider, [], "own");
    await post(co, ownGid, "自己的 delta");

    const forbidden = await co.request({ t: "search", query: "delta", groupId: gid });
    expect(forbidden).toMatchObject({ t: "ack", ok: false, error: { code: "forbidden" } });
    const mine = await search(co, { query: "delta" });
    expect(mine.map((h) => h.groupId)).toEqual([ownGid]);

    // Across all of mine: hits of both groups, newest first.
    const other = await createGroup(a, [], "a2");
    await post(ca, other, "第二个群 delta");
    const all = await search(ca, { query: "delta" });
    expect(all.map((h) => h.groupId)).toEqual([other, gid]);
    expect(all[0].createdAt).toBeGreaterThanOrEqual(all[1].createdAt);
    expect(await search(ca, { query: "delta", limit: 1 })).toHaveLength(1);
  });

  it("treats FTS syntax and SQL in queries as plain text", async () => {
    const { gid, ca, cb } = await setup();
    await post(ca, gid, "safe text 部署");
    const attempts = [
      '"部署" OR *',
      "部署 OR safe",
      "NEAR(safe text)",
      "tokens:safe",
      "safe*",
      "^safe",
      "'); DROP TABLE messages; --",
      '" AND 1=1 --',
      "(safe) NOT text",
      "\u0000safe",
      "😀",
      "a".repeat(200),
    ];
    for (const q of attempts) {
      const r = await cb.request({ t: "search", query: q, groupId: gid });
      expect(r.t, `${q}: ${JSON.stringify(r)}`).toBe("searchResults");
    }
    expect(await search(cb, { query: "😀" })).toEqual([]);
    expect(await search(cb, { query: "部署 OR nothing" })).toEqual([]);
    expect(await search(cb, { query: "safe text" })).toHaveLength(1);
    const tooLong = await cb.request({ t: "search", query: "a".repeat(201) });
    expect(tooLong).toMatchObject({ t: "ack", ok: false, error: { code: "invalid" } });
  });

  it("backfills messages written before the search-1 migration", async () => {
    const { gid, ca, cb } = await setup();
    const s = await post(ca, gid, "老消息 epsilon", { title: "旧话题标题" });
    for (let i = 0; i < 12; i++) await post(ca, gid, `第 ${i} 条 epsilon`, { topicId: s.topic.id });
    const del = await post(ca, gid, "已删除 epsilon", { topicId: s.topic.id });
    await ca.ok({ t: "delete", groupId: gid, messageId: del.message.id });
    const stub = env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid));
    await runInDurableObject(stub, async (_room: GroupRoom, state) => {
      const sql = state.storage.sql;
      // Back to a room from before search-1, then run the migrations as a deploy does.
      sql.exec("DROP TABLE search_fts");
      sql.exec("DROP TABLE search_docs");
      sql.exec("DROP TABLE search_backfill");
      sql.exec("DELETE FROM migrations WHERE name = 'search-1'");
      expect(runMigrations(state.storage, roomMigrationSteps())).toEqual(["search-1"]);
      expect(sql.exec("SELECT count(*) AS n FROM search_docs").one().n).toBe(0);
    });
    // A write starts the backfill in the background (and is indexed live).
    await post(ca, gid, "新消息 epsilon", { topicId: s.topic.id });
    expect(await search(cb, { query: "epsilon" })).toHaveLength(14);
    expect(await search(cb, { query: "旧话题" })).toHaveLength(1);
    await runInDurableObject(stub, async (_room: GroupRoom, state) => {
      const row = state.storage.sql.exec<{ c: number; u: number; d: number }>(
        "SELECT msg_cursor AS c, until_seq AS u, topics_done AS d FROM search_backfill",
      ).one();
      expect(row.c).toBe(row.u);
      expect(row.d).toBe(1);
    });
  });
});
