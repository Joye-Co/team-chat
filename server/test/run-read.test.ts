// What a run reads of its group besides messages (machine-protocol.md, "Topic context"): the
// 产出 of its topic and group with their content, a card whole, the group's decisions and
// members, another topic of the same group, and one compact state card of a topic. Never
// another group.
import type { CardV2 } from "@chat/protocol";
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { membersText, topicStateText } from "../src/context/read";
import { Daemon, downViolations, world } from "./daemon";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function upload(m: TestMember, groupId: string, name: string, mime: string, text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: await sha(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

async function artifact(m: TestMember, groupId: string, topicId: string, title: string, text: string) {
  const fileId = await upload(m, groupId, "plan.md", "text/markdown", text);
  const r = await api("/v1/artifacts", { token: m.token, body: { groupId, topicId, title, kind: "file", entry: "plan.md", files: [{ path: "plan.md", fileId }] } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return { id: r.body.artifact.id as string, fileId };
}

const card = (extra: Partial<CardV2>): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "ask",
  title: "今天发版吗？",
  fallback: "今天发版吗？",
  blocks: [{ id: "pick", type: "decision", deliver: "immediate", options: [{ id: "yes", label: "发" }, { id: "no", label: "不发" }] }],
  ...extra,
});

describe("render and prompt text", () => {
  it("names the members with 岗位 and marks AI members and the run's own member", () => {
    const text = membersText(
      [
        { id: "u1", name: "张三", role: "产品负责人", ai: false },
        { id: "b1", name: "小克", role: "前端", ai: true },
        { id: "b2", name: "小测", ai: true },
      ],
      "b1",
    );
    expect(text).toContain("- 张三 · 产品负责人");
    expect(text).toContain("- 小克 · 前端 · AI member · you");
    expect(text).toContain("- 小测 · AI member");
    const many = membersText(Array.from({ length: 50 }, (_, i) => ({ id: `u${i}`, name: `成员${i}`, ai: false })), "x");
    expect(many).toContain("10 more: chat-run member list");
  });

  it("keeps the state card within 2000 characters", () => {
    const long = "很长的一句话".repeat(100);
    const text = topicStateText(
      {
        title: long,
        messages: 300,
        authors: Array.from({ length: 8 }, (_, i) => ({ authorId: `u${i}`, count: 10, lastSeq: i, lastAt: 0, lastBody: long })),
        artifacts: Array.from({ length: 8 }, (_, i) => ({ id: `a${i}`, title: long, kind: "html", version: 3, updatedAt: 0, createdBy: "u1", note: long })),
        openCards: Array.from({ length: 5 }, (_, i) => ({ messageId: `m${i}`, title: long, from: ["u1", "u2"] })),
        decisions: Array.from({ length: 5 }, () => ({ title: long, label: "发", choices: [long], by: "u1", at: 0 })),
      },
      (id) => id,
    );
    expect([...text].length).toBeLessThanOrEqual(2000);
    expect(text).toContain("产出");
  });
});

describe("chat-run reads", () => {
  it("reads 产出, cards, decisions, members, another topic and the topic state of its own group only", async () => {
    const w = await world();
    await env.DB.prepare("UPDATE members SET role = ? WHERE id = ?").bind("前端", w.bot.id).run();
    const { d } = await Daemon.connect(w.machineToken, { features: ["context"] });
    const { c } = await Client.connect(w.human.token, { features: ["interact/1", "refs/1"] });
    const cbot = (await Client.connect(w.bot.token, { features: ["interact/1", "refs/1"] })).c;

    // Another topic of the same group, without the bot.
    const other = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "B 话题：回滚步骤要先停写入" });
    const otherTopic = other.message.topicId as string;

    // The run's topic: a 产出, a message that points at it, an answered card and an open one.
    const first = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev 按上次那份方案改", mentions: [w.bot.id] });
    const topicId = first.message.topicId as string;
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("Members of this group (name · 岗位):");
    expect(a.prompt).toContain("- Alice");
    expect(a.prompt).toContain("- dev · 前端 · AI member · you");

    const plan = await artifact(w.owner, w.gid, topicId, "上线方案", "# 上线方案\n1. 先灰度 10%\n");
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "方案在这", artifactRefs: [{ artifactId: plan.id, version: 1 }] });
    const asked = await cbot.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "", cardV2: card({ ask: { from: [w.human.id], record: true } }) });
    const answered = await c.request({ t: "card.event", groupId: w.gid, messageId: asked.message.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    expect(answered, JSON.stringify(answered)).toMatchObject({ ok: true });
    const open = await cbot.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "", cardV2: card({ title: "要通知客户吗？", fallback: "要通知客户吗？", ask: { from: [w.human.id] } }) });
    expect((await api(`/v1/groups/${w.gid}/decisions`, { token: w.human.token, body: { text: "周五不发版", topicId } })).status).toBe(200);

    // A group the bot is not in, with its own 产出 and topic.
    const elsewhere = await createGroup(w.owner, [], "other"); // Alice (the run is hers) is not in it either
    const co = (await Client.connect(w.owner.token)).c;
    const far = await co.ok({ t: "send", groupId: elsewhere, clientMsgId: clientMsgId(), body: "别的群" });
    const farArt = await artifact(w.owner, elsewhere, far.message.topicId, "别的群的产出", "secret");

    const auth = { authorization: `Bearer ${w.machineToken}` };
    const get = async (path: string) => {
      const r = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}${path}`, { headers: auth });
      return { status: r.status, body: (await r.json()) as any };
    };

    // 产出: list, content, versions; the line names the title.
    const list = await get("/artifacts");
    expect(list.status).toBe(200);
    expect(list.body.artifacts.map((x: any) => x.title)).toEqual(["上线方案"]);
    expect((await get(`/artifacts?scope=group`)).body.artifacts.some((x: any) => x.id === plan.id)).toBe(true);
    expect((await get(`/artifacts?topic=${otherTopic}`)).body.artifacts).toEqual([]);
    const got = await get(`/artifacts/${plan.id}`);
    expect(got.status).toBe(200);
    expect(got.body).toMatchObject({ version: 1, text: "# 上线方案\n1. 先灰度 10%\n", artifact: { title: "上线方案", currentVersion: 1 } });
    expect(got.body.versions[0].files[0].fileId).toBe(plan.fileId);
    expect((await get(`/artifacts/${plan.id}?version=9`)).status).toBe(404);
    expect((await get(`/artifacts/${farArt.id}`)).status).toBe(404);
    expect((await get(`/artifacts?scope=all`)).status).toBe(400);
    const hist = await get("/topic/messages");
    expect(hist.body.messages.map((m: any) => m.text).join("\n")).toContain(`[产出 a ${plan.id} v1 · 上线方案]`);

    // Another topic of the same group; never another group.
    const b = await get(`/topic/messages?topic=${otherTopic}`);
    expect(b.status).toBe(200);
    expect(b.body.messages[0].text).toContain("B 话题：回滚步骤要先停写入");
    expect((await get(`/topic/messages/${other.message.id}?topic=${otherTopic}`)).body.message.text).toContain("回滚步骤");
    expect((await get(`/topic/messages/${other.message.id}`)).status).toBe(404);
    expect((await get(`/topic/messages?topic=${far.message.topicId}`)).status).toBe(404);
    expect((await get(`/topic/messages?topic=../x`)).status).toBe(400);

    // A card whole, with its answers and who is waited on.
    const answeredCard = await get(`/cards/${asked.message.id}`);
    expect(answeredCard.status).toBe(200);
    expect(answeredCard.body).toMatchObject({ topicId, card: { title: "今天发版吗？" }, answers: [{ actorId: w.human.id, actorName: "Alice" }] });
    const openCard = await get(`/cards/${open.message.id}`);
    expect(openCard.body.ask).toEqual({ status: "open", from: ["Alice"] });
    expect((await get(`/cards/${first.message.id}`)).status).toBe(404);
    expect((await get(`/cards/${far.message.id}`)).status).toBe(404);

    // Decisions, members.
    const dec = await get("/decisions");
    expect(dec.status).toBe(200);
    expect(dec.body.decisions[0]).toMatchObject({ title: "今天发版吗？", actionLabel: "发", answeredByName: "Alice" });
    expect(dec.body.recorded[0]).toMatchObject({ text: "周五不发版", recordedByName: "Alice" });
    const mem = await get("/members");
    expect(mem.body.members).toEqual(
      expect.arrayContaining([
        { id: w.human.id, name: "Alice", ai: false },
        { id: w.bot.id, name: "dev", role: "前端", ai: true },
      ]),
    );

    // The state card of the topic, and of the other topic.
    const state = await get("/topic/state");
    expect(state.status).toBe(200);
    expect(state.body.topicId).toBe(topicId);
    expect(state.body.text).toContain(`上线方案 · a ${plan.id} v1`);
    expect(state.body.text).toContain(`m ${open.message.id} · 要通知客户吗？ · waiting on Alice`);
    expect(state.body.text).toContain("今天发版吗？ → 发");
    // A decision recorded by hand (记一条) in the topic is there too, newest first.
    expect(state.body.text).toContain("- 周五不发版 (recorded by Alice");
    expect(state.body.text.indexOf("周五不发版")).toBeLessThan(state.body.text.indexOf("今天发版吗？ → 发"));
    expect(state.body.text).toContain("Alice:");
    expect([...state.body.text].length).toBeLessThanOrEqual(2000);
    expect((await get(`/topic/state?topic=${otherTopic}`)).body.text).toContain("产出: none yet");

    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    for (const x of [c, cbot, co]) x.close();
    d.close();
  });
});
