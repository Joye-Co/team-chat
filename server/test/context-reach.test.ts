// What an AI member may read for the person it works for (docs/plans/usable/design-visibility.zh.md
// 3.1-3.6): a run reads its own group and every group of kind "group" its person is in, even one
// the AI member is not in; never another group, a DM or a side chat; never for a hook's run or a
// private card's answer; a session that read a group goes on only for a person who may read it.
import { type CardV2, mainTopicId } from "@chat/protocol";
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { READ_GROUPS_NOTE } from "../src/bots/dispatch";
import { type Env, roomStub } from "../src/core/env";
import { Daemon, type World, downViolations, world } from "./daemon";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const FEATURES = ["context", "groups", "resume", "aside", "interact", "output"];

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function artifact(m: TestMember, groupId: string, topicId: string, title: string, text: string) {
  const bytes = new TextEncoder().encode(text);
  const c = await api("/v1/files", { token: m.token, body: { groupId, name: "plan.md", size: bytes.length, mime: "text/markdown", sha256: await sha(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const fileId = c.body.file.id as string;
  const put = await SELF.fetch(`${BASE}/v1/files/${fileId}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  const r = await api("/v1/artifacts", { token: m.token, body: { groupId, topicId, title, kind: "file", entry: "plan.md", files: [{ path: "plan.md", fileId }] } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return { id: r.body.artifact.id as string, fileId };
}

const card = (title: string, from: string[]): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "ask",
  title,
  fallback: title,
  ask: { from, record: true },
  blocks: [{ id: "pick", type: "decision", deliver: "immediate", options: [{ id: "csv", label: "CSV" }, { id: "xlsx", label: "Excel" }] }],
});

const desk = (botId: string) => (env as unknown as BotsEnv).BOT_DESK.get((env as unknown as BotsEnv).BOT_DESK.idFromName(botId));
const runRow = (botId: string, runId: string) =>
  runInDurableObject(desk(botId), async (_o, state) =>
    state.storage.sql.exec("SELECT fresh, read_groups, requested_by, requested_for FROM runs WHERE id = ?", runId).toArray()[0] as Record<string, unknown>,
  );

/**
 * P (Alice) in G1 and G2; Q only in G1; the AI member B only in G1; G3 without P; a DM of P and B
 * with a side chat. G2 has a topic, a card P answered, a 产出 with its file and a decision.
 */
async function setup() {
  const w = await world();
  const P = w.human;
  const Q = await createMember({ name: "Quinn" });
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: Q.id } })).status).toBe(200);
  const pm = await createMember({ kind: "bot", name: "pm" });
  const g2 = await createGroup(w.owner, [P.id, pm.id], "产品");
  const g3 = await createGroup(w.owner, [Q.id], "技术");
  const co = (await Client.connect(w.owner.token)).c;
  const cp = (await Client.connect(P.token, { features: ["interact/1", "refs/1"] })).c;
  const cpm = (await Client.connect(pm.token, { features: ["interact/1", "refs/1"] })).c;

  const m2 = await co.ok({ t: "send", groupId: g2, clientMsgId: clientMsgId(), body: "导出格式先做 CSV，再做 Excel" });
  const t2 = m2.message.topicId as string;
  const art2 = await artifact(w.owner, g2, t2, "需求文档 v2", "# 需求文档 v2\n先做导出\n");
  const asked = await cpm.ok({ t: "send", groupId: g2, topicId: t2, clientMsgId: clientMsgId(), body: "", cardV2: card("导出用什么格式？", [P.id]) });
  const answered = await cp.request({ t: "card.event", groupId: g2, messageId: asked.message.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "pick", event: "choice.submit", value: { value: "csv" } });
  expect(answered, JSON.stringify(answered)).toMatchObject({ ok: true });
  expect((await api(`/v1/groups/${g2}/decisions`, { token: P.token, body: { text: "先做导出", topicId: t2 } })).status).toBe(200);

  const m3 = await co.ok({ t: "send", groupId: g3, clientMsgId: clientMsgId(), body: "技术群的暗号 zeta" });
  const t3 = m3.message.topicId as string;
  const art3 = await artifact(w.owner, g3, t3, "技术方案", "zeta");

  const { d } = await Daemon.connect(w.machineToken, { features: FEATURES, maxParallelRuns: 4 });

  // DM(P, B), and a side chat in it about a topic of G1.
  const dm = await api("/v1/dms", { token: P.token, body: { memberId: w.bot.id } });
  expect(dm.status).toBe(200);
  const dmId = dm.body.group.id as string;
  const said = await cp.ok({ t: "send", groupId: dmId, topicId: mainTopicId(dmId), clientMsgId: clientMsgId(), body: "私聊口令 kiwi" });
  const dmRun = await d.next((f) => f.t === "run.assign" && f.groupId === dmId);
  await finish(d, dmRun.runId, "s-dm");
  const src = await co.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "G1 的开场" });
  const side = await cp.ok({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "单独问暗号 plum", context: { groupId: w.gid, topicId: src.message.topicId, uptoSeq: src.message.createdSeq } });
  const sideTopic = side.topic.id as string;
  const aside = await d.next((f) => f.t === "run.assign" && f.topicId === sideTopic);
  await finish(d, aside.runId, "s-aside");

  return { w, P, Q, g2, g3, t2, m2, art2, asked, t3, art3, dmId, dmMsg: said.message.id as string, sideTopic, d, clients: [co, cp, cpm] };
}

async function finish(d: Daemon, runId: string, sessionId: string): Promise<void> {
  d.send({ t: "run.accept", runId });
  await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "ok", sessionId }));
}

function reads(w: World, runId: string) {
  return async (path: string, init: RequestInit = {}) => {
    const r = await SELF.fetch(`${BASE}/v1/machine/runs/${runId}${path}`, { ...init, headers: { authorization: `Bearer ${w.machineToken}`, ...(init.body ? { "content-type": "application/json" } : {}) } });
    return { status: r.status, body: (await r.json()) as any };
  };
}

async function mention(c: Client, w: World, d: Daemon, body: string, topicId?: string, seen: string[] = []) {
  const sent = await c.ok({ t: "send", groupId: w.gid, ...(topicId ? { topicId } : {}), clientMsgId: clientMsgId(), body, mentions: [w.bot.id] });
  const a = await d.next((f) => f.t === "run.assign" && f.groupId === w.gid && f.topicId === sent.message.topicId && !seen.includes(f.runId));
  return { topicId: sent.message.topicId as string, a };
}

describe("reads across the person's groups", () => {
  it("reads every kind of thing in G2 for P's run, never G3, a DM or a side chat, and writes only its own group", async () => {
    const s = await setup();
    const { w, P, g2, g3, t2, m2, art2, asked, t3, art3, dmId, dmMsg, sideTopic, d } = s;
    const { a } = await mention(s.clients[1], w, d, "@dev 按产品群的需求文档做导出");
    expect(a.prompt).toContain(READ_GROUPS_NOTE);
    const get = reads(w, a.runId);

    // The groups it may read: its own and G2; no DM, no G3, no person.
    const groups = await get("/groups");
    expect(groups.status).toBe(200);
    expect(groups.body.groups).toEqual([
      { id: w.gid, name: "g", kind: "group", current: true },
      { id: g2, name: "产品", kind: "group", current: false },
    ]);
    expect(JSON.stringify(groups.body)).not.toContain(P.id);

    // Topic history, one message, the state card, by id alone or with the group.
    const hist = await get(`/topic/messages?group=${g2}&topic=${t2}`);
    expect(hist.status, JSON.stringify(hist.body)).toBe(200);
    expect(hist.body.messages[0].text).toContain("导出格式先做 CSV");
    expect((await get(`/topic/messages?topic=${t2}`)).body.messages[0].text).toContain("导出格式先做 CSV");
    expect((await get(`/topic/messages/${m2.message.id}?group=${g2}`)).body.message.text).toContain("导出格式先做 CSV");
    expect((await get(`/topic/messages/${m2.message.id}?topic=${t2}`)).status).toBe(200);
    expect((await get(`/topic/messages?group=${g2}`)).status).toBe(400);
    const state = await get(`/topic/state?topic=${t2}`);
    expect(state.body).toMatchObject({ topicId: t2, groupId: g2, groupName: "产品" });
    expect(state.body.text).toContain("需求文档 v2");
    expect((await get(`/topic/state?group=${g2}&topic=${t2}`)).body.groupId).toBe(g2);
    // The run's own topic carries no group.
    expect((await get("/topic/state")).body.groupId).toBeUndefined();

    // A card whole, 产出 with the entry's text, the file, decisions and members.
    const c = await get(`/cards/${asked.message.id}`);
    expect(c.status).toBe(200);
    expect(c.body).toMatchObject({ topicId: t2, groupId: g2, groupName: "产品", card: { title: "导出用什么格式？" }, answers: [{ actorId: P.id, actorName: "Alice" }] });
    expect((await get(`/artifacts?group=${g2}&scope=group`)).body.artifacts.map((x: any) => x.id)).toEqual([art2.id]);
    expect((await get(`/artifacts?group=${g2}&topic=${t2}`)).body.artifacts.map((x: any) => x.id)).toEqual([art2.id]);
    const got = await get(`/artifacts/${art2.id}`);
    expect(got.status).toBe(200);
    expect(got.body.text).toContain("先做导出");
    const file = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/files/${art2.fileId}`, { headers: { authorization: `Bearer ${w.machineToken}` } });
    expect(file.status).toBe(200);
    expect(await file.text()).toContain("需求文档 v2");
    const dec = await get(`/decisions?group=${g2}`);
    expect(dec.status).toBe(200);
    expect(dec.body.decisions[0]).toMatchObject({ title: "导出用什么格式？", answeredByName: "Alice" });
    expect(dec.body.recorded[0]).toMatchObject({ text: "先做导出" });
    expect((await get(`/members?group=${g2}`)).body.members.map((m: any) => m.name).sort()).toEqual(["Alice", "Joye", "pm"]);

    // Search: every readable group with its name, or one group.
    const all = await get(`/search?q=${encodeURIComponent("导出")}&scope=all`);
    expect(all.status).toBe(200);
    expect(all.body.hits.some((h: any) => h.groupId === g2 && h.groupName === "产品" && h.topicId === t2)).toBe(true);
    expect((await get(`/search?q=${encodeURIComponent("导出")}&scope=group&group=${g2}`)).body.hits.length).toBeGreaterThan(0);
    expect((await get(`/search?q=x&scope=topic&group=${g2}`)).status).toBe(400);
    for (const q of ["zeta", "kiwi", "plum"]) expect((await get(`/search?q=${q}&scope=all`)).body.hits).toEqual([]);

    // G3, the DM, the side chat and unknown ids: not_found, the same as each other.
    for (const path of [
      `/topic/messages?group=${g3}&topic=${t3}`,
      `/topic/messages?topic=${t3}`,
      `/topic/state?topic=${t3}`,
      `/artifacts/${art3.id}`,
      `/files/${art3.fileId}`,
      `/decisions?group=${g3}`,
      `/members?group=${g3}`,
      `/artifacts?group=${g3}&scope=group`,
      `/search?q=zeta&scope=group&group=${g3}`,
      `/topic/messages?group=${dmId}&topic=${mainTopicId(dmId)}`,
      `/topic/messages?topic=${mainTopicId(dmId)}`,
      `/topic/messages/${dmMsg}?group=${dmId}`,
      `/members?group=${dmId}`,
      `/topic/messages?topic=${sideTopic}`,
      `/topic/state?topic=${sideTopic}`,
      `/decisions?group=01UNKNOWNGROUP0000000000000`,
      `/topic/messages?topic=01UNKNOWNTOPIC0000000000000`,
    ]) {
      expect((await get(path)).status, path).toBe(404);
    }

    // Writes stay in the run's own group: a new version of G2's 产出 is not_found.
    const version = await get(`/artifacts/${art2.id}/versions`, { method: "POST", body: JSON.stringify({ entry: "plan.md", files: [{ path: "plan.md", fileId: art2.fileId }] }) });
    expect(version.status, JSON.stringify(version.body)).toBe(404);

    // The run remembers the one other group it read.
    expect(JSON.parse((await runRow(w.bot.id, a.runId)).read_groups as string)).toEqual([g2]);
    await finish(d, a.runId, "s-1");
    d.close();
    for (const x of s.clients) x.close();
  });

  it("reads nothing of G2 for Q, or for P once P left G2, and goes on with a session only for a person who may read what it read", async () => {
    const s = await setup();
    const { w, P, Q, g2, t2, art2, d } = s;
    const cq = (await Client.connect(Q.token)).c;

    // P's run reads G2, then P's next run in the topic continues that session and inherits it.
    const first = await mention(s.clients[1], w, d, "@dev 产品群定了先做什么？");
    expect((await reads(w, first.a.runId)(`/topic/messages?topic=${t2}`)).status).toBe(200);
    await finish(d, first.a.runId, "s-1");
    const second = await mention(s.clients[1], w, d, "@dev 再看一下需求文档", first.topicId, [first.a.runId]);
    expect(second.a.resumeSessionId).toBe("s-1");
    expect(JSON.parse((await runRow(w.bot.id, second.a.runId)).read_groups as string)).toEqual([g2]);
    const get2 = reads(w, second.a.runId);
    expect((await get2(`/artifacts/${art2.id}`)).status).toBe(200);
    // P leaves G2: the next read of the active run is not_found.
    expect((await api(`/v1/groups/${g2}/members/${P.id}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    expect((await get2(`/artifacts/${art2.id}`)).status).toBe(404);
    expect((await get2(`/topic/messages?topic=${t2}`)).status).toBe(404);
    expect((await get2("/groups")).body.groups.map((g: any) => g.id)).toEqual([w.gid]);
    await finish(d, second.a.runId, "s-2");

    // Q asks in the same topic: a new session (fresh), and nothing of G2.
    const third = await mention(cq, w, d, "@dev 产品群定了先做什么？", first.topicId, [first.a.runId, second.a.runId]);
    expect(third.a.resumeSessionId).toBeUndefined();
    expect((await runRow(w.bot.id, third.a.runId)).fresh).toBe(1);
    const get3 = reads(w, third.a.runId);
    for (const path of [`/topic/messages?topic=${t2}`, `/topic/messages?group=${g2}&topic=${t2}`, `/artifacts/${art2.id}`, `/files/${art2.fileId}`, `/decisions?group=${g2}`]) {
      expect((await get3(path)).status, path).toBe(404);
    }
    expect((await get3(`/search?q=${encodeURIComponent("导出")}&scope=all`)).body.hits.every((h: any) => h.groupId === undefined)).toBe(true);
    await finish(d, third.a.runId, "s-3");
    d.close();
    for (const x of [...s.clients, cq]) x.close();
  });

  it("reads only its own group for a hook's run and a private card's answer, and names no answerer", async () => {
    const s = await setup();
    const { w, P, g2, t2, d } = s;

    // A hook's run works for no person (requested_for is null).
    const hook = await mention(s.clients[1], w, d, "@dev 看一下");
    await runInDurableObject(desk(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET requested_by = ?, requested_for = NULL WHERE id = ?", "hook-bot", hook.a.runId);
    });
    const gh = reads(w, hook.a.runId);
    expect((await gh(`/topic/messages?topic=${t2}`)).status).toBe(404);
    expect((await gh(`/decisions?group=${g2}`)).status).toBe(404);
    expect((await gh("/groups")).body.groups.map((g: any) => g.id)).toEqual([w.gid]);
    d.send({ t: "run.accept", runId: hook.a.runId });

    // A private questionnaire answered by P: the run it starts reads only G1 and names nobody.
    const outputId = "p1";
    d.event(hook.a.runId, {
      type: "output",
      outputId,
      op: "card.post",
      body: {
        card: {
          v: 2,
          catalog: "tc.card/2.1",
          rev: 1,
          intent: "ask",
          title: "实习反馈",
          fallback: "实习反馈",
          ask: { from: [P.id], blocking: false, record: true },
          blocks: [
            { id: "q", type: "questionnaire", children: ["mood", "next"], anonymous: true, fallbackText: "实习反馈" },
            { id: "mood", type: "input", kind: "text", label: "感受" },
            { id: "next", type: "input", kind: "text", label: "建议" },
          ],
        },
      },
    });
    const posted = await d.next((f) => f.t === "run.output.result" && f.outputId === outputId);
    expect(posted, JSON.stringify(posted)).toMatchObject({ ok: true });
    await d.acked(hook.a.runId, d.done(hook.a.runId, { status: "succeeded", resultText: "ok" }));
    await waitUntil(async () => (await runInDurableObject(desk(w.bot.id), async (_o, state) => state.storage.sql.exec("SELECT 1 FROM runs WHERE ended_at IS NULL").toArray().length)) === 0, 5000, "runs end");
    const pressed = await s.clients[1].ok({ t: "card.event", groupId: w.gid, messageId: posted.result.messageId, rev: 1, clientEventId: crypto.randomUUID(), blockId: "q", event: "action.press", inputs: { mood: { value: "还行" } } });
    expect(pressed).toBeTruthy();
    const anon = await d.next((f) => f.t === "run.assign" && f.runId !== hook.a.runId && f.groupId === w.gid, 10_000);
    expect(await runRow(w.bot.id, anon.runId)).toMatchObject({ requested_by: "anonymous", requested_for: P.id });
    expect(anon.prompt).not.toContain(READ_GROUPS_NOTE);
    expect(anon.prompt).not.toContain(P.id);
    const ga = reads(w, anon.runId);
    expect((await ga(`/topic/messages?topic=${t2}`)).status).toBe(404);
    expect((await ga(`/topic/messages?group=${g2}&topic=${t2}`)).status).toBe(404);
    const groups = await ga("/groups");
    expect(groups.body.groups.map((g: any) => g.id)).toEqual([w.gid]);
    expect(JSON.stringify(groups.body)).not.toContain(P.id);
    expect(JSON.stringify(groups.body)).not.toContain("Alice");
    const ctx = await api(`/v1/runs/${anon.runId}/context`, { token: w.owner.token });
    expect(ctx.status).toBe(200);
    expect(JSON.stringify(ctx.body)).not.toContain(P.id);
    await finish(d, anon.runId, "s-anon");
    d.close();
    for (const x of s.clients) x.close();
  });

  it("lets readAs call only the read RPCs", async () => {
    const w = await world();
    const viewer = { id: w.bot.id, level: "member" as const, kind: "bot" as const };
    const stub = roomStub(env as unknown as Env, w.gid);
    const refused = await stub.readAs(w.gid, "cards.post", viewer, {});
    expect(refused).toMatchObject({ ok: false, error: { code: "forbidden" } });
    const ok = await stub.readAs(w.gid, "cards.decisions", viewer, { limit: 5 });
    expect(ok).toMatchObject({ ok: true });
  });
});
