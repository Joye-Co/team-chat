// 1.1.0 gap fixes of the cards area: files and 产出 a card names, draft content hashes,
// reserved 产出 paths, anonymous counts, refs from an aside topic, recorded decisions, member
// card status, shape reasons, catalogs per socket and frame sources.
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const FEATURES = ["interact/1", "refs/1"];

export const base = (extra: Partial<CardV2>): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "info",
  title: "卡片",
  fallback: "卡片",
  blocks: [],
  ...extra,
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function upload(m: TestMember, groupId: string, name = "a.png", mime = "image/png", text = "png"): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: await sha(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

async function world() {
  const owner = await createMember({ level: "owner", name: "Joye" });
  const alice = await createMember({ name: "Alice" });
  const bob = await createMember({ name: "Bob" });
  const bot = await createMember({ kind: "bot", name: "Ledger" });
  const gid = await createGroup(owner, [alice.id, bob.id, bot.id]);
  const connect = async (m: TestMember, features: string[] = FEATURES) => (await Client.connect(m.token, { features })).c;
  return { owner, alice, bob, bot, gid, connect };
}

const send = (c: Client, gid: string, cardV2: CardV2, extra: Record<string, unknown> = {}) =>
  c.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2, ...extra });

describe("files and 产出 a card names (PC-2)", () => {
  it("refuses a file or 产出 of another group and takes one of this group", async () => {
    const w = await world();
    const other = await createGroup(w.owner, [w.bot.id]);
    const mine = await upload(w.bot, w.gid);
    const theirs = await upload(w.bot, other);
    const cbot = await w.connect(w.bot);
    const image = (fileId: string) => base({ blocks: [{ id: "img", type: "image", fileId }] });
    const bad = await send(cbot, w.gid, image(theirs));
    expect(bad, JSON.stringify(bad)).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    expect(bad.error.message).toContain("file not in group");
    const choice = base({
      intent: "ask",
      ask: { from: [w.alice.id] },
      blocks: [
        {
          id: "pick",
          type: "imageChoice",
          label: "哪个？",
          fallbackText: "选图",
          options: [
            { id: "a", label: "A", fileId: mine },
            { id: "b", label: "B", fileId: theirs },
          ],
        },
      ],
    });
    expect(await send(cbot, w.gid, choice)).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    const evidence = base({ blocks: [{ id: "e", type: "evidence", label: "截图", fileId: theirs }] });
    expect(await send(cbot, w.gid, evidence)).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    const ref = base({ blocks: [{ id: "r", type: "ref", label: "页面", target: { kind: "artifact", artifactId: "01JBX0ART0000000000000000Z" } }] });
    const r = await send(cbot, w.gid, ref);
    expect(r).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    expect(r.error.message).toContain("产出 not in group");
    const ok = await send(cbot, w.gid, image(mine));
    expect(ok, JSON.stringify(ok)).toMatchObject({ ok: true });
    // A patch that names a file of the other group is refused too.
    const p = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: ok.result.message.id, baseRev: 1, ops: [{ op: "set", id: "img", props: { fileId: theirs } }] });
    expect(p).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    cbot.close();
  });
});

async function hex(text: string): Promise<string> {
  return sha(new TextEncoder().encode(text));
}

describe("increment rules (PC-6)", () => {
  it("stores the content hash of a draft answer and refuses a stale one", async () => {
    const w = await world();
    const [ca, cbot] = await Promise.all([w.connect(w.alice), w.connect(w.bot)]);
    const card = base({
      intent: "ask",
      title: "公告稿",
      ask: { from: [w.alice.id] },
      blocks: [{ id: "d", type: "draft", text: "明天发版。", editable: true }],
      actions: [{ id: "ok", label: "发", inputs: ["d"] }],
    });
    const m = (await send(cbot, w.gid, card)).result.message;
    const stale = await ca.request({ t: "card.answer", groupId: w.gid, messageId: m.id, rev: 1, actionId: "ok", contentHash: await hex("今天发版。") });
    expect(stale, JSON.stringify(stale)).toMatchObject({ ok: false, error: { code: "conflict", reason: "card.superseded" } });
    const edited = "明天上午发版。";
    const r = await ca.request({ t: "card.answer", groupId: w.gid, messageId: m.id, rev: 1, actionId: "ok", editedValues: { d: edited }, contentHash: await hex(edited) });
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true });
    const u = await cbot.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState.answers.length === 1);
    expect(u.data.cardState.answers[0].contentHash).toBe(await hex(edited));
    for (const c of [ca, cbot]) c.close();
  });

  it("refuses 产出 paths under __teamchat/", async () => {
    const w = await world();
    const cbot = await w.connect(w.bot);
    const root = (await cbot.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "页面" })).message;
    const html = await upload(w.bot, w.gid, "index.html", "text/html", "<p>x</p>");
    const files = (path: string) => [{ path: "index.html", fileId: html }, { path, fileId: html }];
    const bad = await api("/v1/artifacts", { token: w.bot.token, body: { groupId: w.gid, topicId: root.topicId, title: "页", kind: "html", entry: "index.html", files: files("__teamchat/x.js") } });
    expect(bad.status, JSON.stringify(bad.body)).toBe(400);
    expect(bad.body.error.code).toBe("invalid");
    const ok = await api("/v1/artifacts", { token: w.bot.token, body: { groupId: w.gid, topicId: root.topicId, title: "页", kind: "html", entry: "index.html", files: files("teamchat/x.js") } });
    expect(ok.status, JSON.stringify(ok.body)).toBe(200);
    const v2 = await api(`/v1/artifacts/${ok.body.artifact.id}/versions`, { token: w.bot.token, body: { entry: "index.html", files: files("__teamchat/diagram.js") } });
    expect(v2.status, JSON.stringify(v2.body)).toBe(400);
    cbot.close();
  });

  it("gives members who do not see the answers of an anonymous questionnaire the count", async () => {
    const w = await world();
    const [ca, cb, cbot] = await Promise.all([w.connect(w.alice), w.connect(w.bob), w.connect(w.bot)]);
    const card = base({
      intent: "ask",
      title: "实习反馈",
      ask: { from: [w.alice.id, w.bob.id], blocking: false },
      blocks: [
        { id: "q", type: "questionnaire", children: ["mood", "next"], anonymous: true, fallbackText: "实习反馈" },
        { id: "mood", type: "input", kind: "text", label: "感受" },
        { id: "next", type: "input", kind: "text", label: "建议" },
      ],
    });
    const m = (await send(cbot, w.gid, card)).result.message;
    const r = await ca.request({ t: "card.event", groupId: w.gid, messageId: m.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "q", event: "action.press", inputs: { mood: { value: "还行" } } });
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true });
    const u = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    expect(u.data.cardState.answers).toEqual([]);
    expect(u.data.cardState.answerCount).toBe(1);
    const h = await cb.request({ t: "history", groupId: w.gid, topicId: m.topicId });
    expect(h.messages.find((x: any) => x.id === m.id).cardState.answerCount).toBe(1);
    for (const c of [ca, cb, cbot]) c.close();
  });
});

describe("记一条 (PC-8)", () => {
  it("records a decision by hand, lists it and refuses non-members and empty text", async () => {
    const w = await world();
    const ca = await w.connect(w.alice);
    const root = (await ca.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "发版" })).message;
    const r = await api(`/v1/groups/${w.gid}/decisions`, { token: w.alice.token, body: { text: "  周五停止合并  ", topicId: root.topicId } });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body.decision).toMatchObject({ text: "周五停止合并", topicId: root.topicId, recordedBy: w.alice.id });
    const ev = await ca.waitEvent((e) => e.type === "decision.recorded");
    expect(ev.data.decision.id).toBe(r.body.decision.id);
    const g = await api(`/v1/groups/${w.gid}/decisions`, { token: w.bob.token, body: { text: "群里定的" } });
    expect(g.status).toBe(200);
    expect(g.body.decision.topicId).toBeNull();
    const list = await api(`/v1/groups/${w.gid}/decisions`, { token: w.bob.token, method: "GET" });
    expect(list.status).toBe(200);
    expect(list.body.recorded.map((d: any) => d.text)).toEqual(["群里定的", "周五停止合并"]);
    const outsider = await createMember();
    expect((await api(`/v1/groups/${w.gid}/decisions`, { token: outsider.token, body: { text: "x" } })).status).toBe(404);
    expect((await api(`/v1/groups/${w.gid}/decisions`, { token: w.bob.token, body: { text: "   " } })).status).toBe(400);
    expect((await api(`/v1/groups/${w.gid}/decisions`, { token: w.bob.token, body: { text: "x", topicId: "01JBX0TOPIC0000000000000ZZ" } })).status).toBe(404);
    ca.close();
  });
});

describe("member card status (PC-9)", () => {
  it("lets the author mark a card executing, failed and clear it by patch", async () => {
    const w = await world();
    const [ca, cbot] = await Promise.all([w.connect(w.alice), w.connect(w.bot)]);
    const card = base({ intent: "done", title: "发版", blocks: [{ id: "t", type: "text", text: "开始发版。" }] });
    const m = (await send(cbot, w.gid, { ...card, status: "executing" } as CardV2)).result.message;
    expect(m.cardV2.status).toBe("executing");
    const p = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: m.id, baseRev: 1, ops: [{ op: "card", props: { status: "failed" } }] });
    expect(p, JSON.stringify(p)).toMatchObject({ ok: true });
    expect(p.result.message.cardV2.status).toBe("failed");
    const u = await ca.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2?.status === "failed");
    expect(u.data.cardV2.rev).toBe(2);
    const c = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: m.id, baseRev: 2, ops: [{ op: "card", props: { status: null } }] });
    expect(c, JSON.stringify(c)).toMatchObject({ ok: true });
    expect(c.result.message.cardV2.status).toBeUndefined();
    expect(await ca.request({ t: "card.patch", groupId: w.gid, messageId: m.id, baseRev: 3, ops: [{ op: "card", props: { status: "failed" } }] })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    const bad = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: m.id, baseRev: 3, ops: [{ op: "card", props: { status: "paused" } }] });
    expect(bad).toMatchObject({ ok: false, error: { code: "invalid" } });
    for (const x of [ca, cbot]) x.close();
  });
});

describe("error reasons (PC-12)", () => {
  it("names card.limits for a bad card shape and card.block_gone for a patch on a missing card", async () => {
    const w = await world();
    const cbot = await w.connect(w.bot);
    const bad = await send(cbot, w.gid, { ...base({}), intent: "maybe" } as unknown as CardV2);
    expect(bad, JSON.stringify(bad)).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    const m = (await send(cbot, w.gid, base({ blocks: [{ id: "t", type: "text", text: "x" }] }))).result.message;
    const op = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: m.id, baseRev: 1, ops: [{ op: "card", props: { intent: "maybe" } }] });
    expect(op, JSON.stringify(op)).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    const gone = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: "01JBX0MSG000000000000000ZZ", baseRev: 1, ops: [{ op: "remove", id: "t" }] });
    expect(gone, JSON.stringify(gone)).toMatchObject({ ok: false, error: { code: "not_found", reason: "card.block_gone" } });
    cbot.close();
  });
});
