// 1.1 boundary: Card v2 messages with their derived v1 card, card answers, side chat context,
// and 产出 page events. Routing answers and events to runs is tested by the bots feature.
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { type CardV2, cardV2ToV1 } from "@chat/protocol";
import { BASE, Client, api, clientMsgId, createGroup, createMember, protocolViolations, type TestMember } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const ask = (rev: number, from: string[], extra: Partial<CardV2> = {}): CardV2 => ({
  v: 2,
  rev,
  intent: "ask",
  title: "发布吗？",
  fallback: "请选择发布时间",
  ask: { from, record: true },
  blocks: [
    { type: "decision", id: "when", options: [{ id: "now", label: "现在" }, { id: "later", label: "晚点" }] },
    { type: "draft", id: "notes", text: "说明", editable: true },
  ],
  actions: [
    { id: "ship", label: "发布", style: "primary" },
    { id: "wait", label: "再等等", kind: "cancel" },
    { id: "pr", label: "看 PR", kind: "link", target: { kind: "url", url: "https://example.com/pr/1" } },
  ],
  ...extra,
});

async function setup() {
  const owner = await createMember({ level: "owner" });
  const alice = await createMember();
  const bob = await createMember();
  const gid = await createGroup(owner, [alice.id, bob.id]);
  const [co, ca, cb] = await Promise.all([owner, alice, bob].map(async (m) => (await Client.connect(m.token)).c));
  return { owner, alice, bob, gid, co, ca, cb };
}

describe("cards v2", () => {
  it("stores cardV2 next to a derived v1 card, takes answers once and checks rev, ask.from and blocks", async () => {
    const { owner, alice, bob, gid, co, ca, cb } = await setup();
    const card = ask(1, [alice.id]);
    const sent = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: card });
    const m = sent.message;
    expect(m.kind).toBe("card");
    expect(m.cardV2).toEqual(card);
    // The derived v1 card: the protocol derivation, the fallback marked for old readers, the ask's status.
    expect(m.card).toMatchObject({ v: 1, title: card.title, tone: cardV2ToV1(card).tone, status: { label: `等 ${alice.name} 拍板` } });
    expect(m.card.blocks[0]).toEqual({ type: "text", text: "这部分需要新版本：请选择发布时间" });
    expect(m.card.actions).toBeUndefined(); // the card has a decision and a draft to fill
    expect(m.cardState.ask).toMatchObject({ id: "a1", status: "open", from: [alice.id], blocking: true });
    expect(m.body).toBe("请选择发布时间");

    const answer = (c: Client, f: Record<string, unknown>) =>
      c.request({ t: "card.answer", groupId: gid, messageId: m.id, rev: 1, actionId: "ship", ...f });
    // Only ask.from may answer; the rev, the action and the block ids must match the card.
    expect(await answer(cb, {})).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await answer(ca, { rev: 2 })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await answer(ca, { actionId: "pr" })).toMatchObject({ ok: false, error: { code: "not_found" } });
    expect(await answer(ca, { inputs: { nope: 1 } })).toMatchObject({ ok: false, error: { code: "invalid" } });
    // inputs with editedValues over 64 KiB as JSON: too_large, as docs/protocol.md says.
    const long = "x".repeat(30_000);
    expect(await answer(ca, { editedValues: { notes: long, a: long, b: long } })).toMatchObject({
      ok: false,
      error: { code: "too_large" },
    });
    expect(await answer(ca, { inputs: { when: "now" }, editedValues: { notes: "改过" } })).toMatchObject({ ok: true });
    const ev = await cb.waitEvent((e) => e.type === "card.answered");
    expect(ev).toMatchObject({
      actorId: alice.id,
      data: { messageId: m.id, rev: 1, actionId: "ship", inputs: { when: "now" }, editedValues: { notes: "改过" } },
    });
    // The first answer wins.
    expect(await answer(ca, {})).toMatchObject({ ok: false, error: { code: "conflict", reason: "card.already_answered" } });

    // An edit needs a higher rev; the derived v1 card follows the new rev.
    const rev2 = ask(2, ["owners"], { intent: "done", ask: undefined, actions: [{ id: "ok", label: "好" }] });
    expect(await co.request({ t: "edit", groupId: gid, messageId: m.id, body: "", cardV2: ask(1, ["owners"]) })).toMatchObject({
      ok: false,
      error: { code: "conflict" },
    });
    const edited = await co.ok({ t: "edit", groupId: gid, messageId: m.id, body: "", cardV2: rev2 });
    expect(edited.message.cardV2.rev).toBe(2);
    expect(edited.message.card.tone).toBe("success");
    // The answer refreshed the state and the v1 card; the edit carries the new rev.
    const answered = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardState);
    expect(answered.data.cardState.ask).toMatchObject({ status: "answered", answeredBy: alice.id });
    expect(answered.data.card.status.label).toBe(`${alice.name} 选了 现在`);
    const upd = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id && e.data.cardV2);
    expect(upd.data.cardV2.rev).toBe(2);

    // A v1 press on a v2 card is action.press at the current rev; without an ask it is live (not stored).
    expect(await cb.request({ t: "card.action", groupId: gid, messageId: m.id, actionId: "ok" })).toMatchObject({ ok: true });
    await new Promise((r) => setTimeout(r, 200));
    expect(co.events(gid).filter((e) => e.type === "card.answered")).toHaveLength(1);

    // A send with an invalid cardV2 and an edit of a text message are refused.
    expect(
      await co.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: { ...card, intent: "vote" } }),
    ).toMatchObject({ ok: false, error: { code: "invalid" } });
    const text = await co.ok({ t: "send", groupId: gid, topicId: sent.topic.id, clientMsgId: clientMsgId(), body: "plain" });
    expect(
      await co.request({ t: "edit", groupId: gid, messageId: text.message.id, body: "", cardV2: card }),
    ).toMatchObject({ ok: false, error: { code: "invalid" } });

    const genv = env as unknown as { GROUP_ROOM: DurableObjectNamespace };
    const decisionRows = () =>
      runInDurableObject(genv.GROUP_ROOM.get(genv.GROUP_ROOM.idFromName(gid)), (_i, state: DurableObjectState) =>
        state.storage.sql.exec("SELECT title FROM card_decisions WHERE message_id = ?", m.id).toArray(),
      );
    expect((await decisionRows()).length).toBe(1);

    // Deleting the card removes what the answerer typed from the log, and its decision rows.
    await co.ok({ t: "delete", groupId: gid, messageId: m.id });
    expect(await decisionRows()).toEqual([]);
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const answers = page.events.filter((e: any) => e.type === "card.answered");
    expect(answers.length).toBe(1);
    expect(answers[0].data.inputs).toBeUndefined();
    expect(answers[0].data.editedValues).toBeUndefined();
    expect(page.events.find((e: any) => e.type === "message.created" && e.data.message.id === m.id).data.message.cardV2).toBeUndefined();
    void owner;
    for (const c of [co, ca, cb]) c.close();
  });

  it("refuse an expired ask and cards without v2", async () => {
    const { owner, gid, co } = await setup();
    const card = ask(1, ["owners"], { ask: { from: ["owners"], expiresAt: Date.now() - 1 } });
    const m = (await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: card })).message;
    expect(await co.request({ t: "card.answer", groupId: gid, messageId: m.id, rev: 1, actionId: "ship" })).toMatchObject({
      ok: false,
      error: { code: "conflict" },
    });
    const v1 = (
      await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", card: { v: 1, title: "t", blocks: [], actions: [{ id: "a", label: "A" }] } })
    ).message;
    expect(await co.request({ t: "card.answer", groupId: gid, messageId: v1.id, rev: 1, actionId: "a" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    void owner;
    co.close();
  });
});

describe("side chats", () => {
  it("start a side chat in a DM with a bot that is in the source group, and move its context on", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot", name: "jojo" });
    const other = await createMember({ kind: "bot", name: "outside" });
    const gid = await createGroup(owner, [bot.id]);
    const co = (await Client.connect(owner.token)).c;
    const src = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "做发布" });
    const dm = await api("/v1/dms", { token: owner.token, body: { memberId: bot.id } });
    expect(dm.status).toBe(200);
    const dmId = dm.body.group.id as string;
    const context = { groupId: gid, topicId: src.topic.id, uptoSeq: src.message.createdSeq };

    const side = await co.ok({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "卡在哪？", context });
    expect(side.topic.contextOf).toEqual(context);
    // A later send moves it on; the same or a lower seq changes nothing.
    await co.ok({ t: "send", groupId: dmId, topicId: side.topic.id, clientMsgId: clientMsgId(), body: "现在呢？", context: { ...context, uptoSeq: context.uptoSeq + 5 } });
    const moved = await co.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === side.topic.id);
    expect(moved.data.contextOf.uptoSeq).toBe(context.uptoSeq + 5);
    // Over HTTP too, and the context of a side chat cannot change topic.
    const src2 = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "另一个" });
    const moveTopic = await api(`/v1/groups/${dmId}/messages`, {
      token: owner.token,
      body: { clientMsgId: clientMsgId(), body: "x", topicId: side.topic.id, context: { ...context, topicId: src2.topic.id } },
    });
    expect(moveTopic.status).toBe(400);

    // Not in a normal group, not about a group the bot is not in, not with a person.
    expect(await co.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", context })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    const dm2 = await api("/v1/dms", { token: owner.token, body: { memberId: other.id } });
    expect(await co.request({ t: "send", groupId: dm2.body.group.id, clientMsgId: clientMsgId(), body: "x", context })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    const person = await createMember();
    const dm3 = await api("/v1/dms", { token: owner.token, body: { memberId: person.id } });
    expect(await co.request({ t: "send", groupId: dm3.body.group.id, clientMsgId: clientMsgId(), body: "x", context })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    expect(
      await co.request({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "x", context: { ...context, topicId: "nope" } }),
    ).toMatchObject({ ok: false, error: { code: "not_found" } });
    co.close();
  });
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function htmlFile(m: TestMember, groupId: string): Promise<string> {
  const bytes = new TextEncoder().encode("<!doctype html><p>hi</p>");
  const c = await api("/v1/files", {
    token: m.token,
    body: { groupId, name: "index.html", size: bytes.length, mime: "text/html", sha256: await sha(bytes) },
  });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, {
    method: "PUT",
    headers: { authorization: `Bearer ${m.token}` },
    body: bytes,
  });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

describe("产出 events", () => {
  it("keep declared events of bot versions and give page events to the version's creator, not the log", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot", name: "maker" });
    const gid = await createGroup(owner, [bot.id]);
    const co = (await Client.connect(owner.token)).c;
    const topicId = (await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "做个页面" })).topic.id;
    const events = [{ name: "pick", label: "选方案" }];
    const made = await api("/v1/artifacts", {
      token: bot.token,
      body: { groupId: gid, topicId, title: "方案", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: await htmlFile(bot, gid) }], events, runId: "not-a-run" },
    });
    expect(made.status, JSON.stringify(made.body)).toBe(200);
    expect(made.body.version.events).toEqual(events);
    expect(made.body.version.runId).toBeUndefined(); // not a run of this bot
    // A person's version gets no events.
    const mine = await api("/v1/artifacts", {
      token: owner.token,
      body: { groupId: gid, topicId, title: "我的", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: await htmlFile(owner, gid) }], events },
    });
    expect(mine.body.version.events).toBeUndefined();

    const id = made.body.artifact.id;
    const cb = (await Client.connect(bot.token, { features: ["interact/1"] })).c;
    const ack = await co.request({ t: "artifact.event", groupId: gid, artifactId: id, version: 1, name: "pick", payload: { option: "B" } });
    expect(ack).toMatchObject({ ok: true, result: { expect: { by: bot.id } } });
    const tr = await cb.next((f) => f.t === "transient" && f.event.type === "artifact.event");
    expect(tr.event).toMatchObject({ actorId: owner.id, artifactId: id, version: 1, name: "pick", kind: "emit", payload: { option: "B" }, eventId: ack.result.eventId });
    // Page events are not stored (V11-INC R5).
    expect(co.events().some((e: { type: string }) => e.type === "artifact.evented")).toBe(false);
    // A declared name with the wrong kind is refused.
    expect(await co.request({ t: "artifact.event", groupId: gid, artifactId: id, version: 1, name: "pick", kind: "ask" })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    cb.close();
    expect(await co.request({ t: "artifact.event", groupId: gid, artifactId: id, version: 1, name: "delete" })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    expect(await co.request({ t: "artifact.event", groupId: gid, artifactId: id, version: 9, name: "pick" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    expect(
      await co.request({ t: "artifact.event", groupId: gid, artifactId: mine.body.artifact.id, version: 1, name: "pick" }),
    ).toMatchObject({ ok: false, error: { code: "forbidden" } });
    co.close();
  });
});

describe("1.1 interaction loop boundary", () => {
  it("check causes, refs and card events of the new frames", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const co = (await Client.connect(owner.token)).c;
    const text = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "表" });
    // A cause that is not a recent event of this author's cards is refused.
    const badCause = await co.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "为什么？", causeEventId: "01JC0EV0000000000000000001" });
    expect(badCause).toMatchObject({ ok: false, error: { code: "invalid", reason: "interact.bad_cause" } });
    // A ref to a message of no group of this room is refused.
    const badRef = await co.request({
      t: "send",
      groupId: gid,
      clientMsgId: clientMsgId(),
      body: "为什么？",
      blockRefs: [{ messageId: "01JC0MSG00000000000000000A", blockId: "md:0", label: "表格 · 第 1 行" }],
    });
    expect(badRef).toMatchObject({ ok: false, error: { code: "invalid" } });
    // A malformed card.event is refused by the schema.
    const bad = await co.request({ t: "card.event", groupId: gid, messageId: text.message.id, rev: 1, clientEventId: "c1", blockId: "a", actionId: "b", event: "action.press" });
    expect(bad).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(bad.error.message).toContain("blockId");
    // A well-formed one on a text message: not a card.
    const good = await co.request({ t: "card.event", groupId: gid, messageId: text.message.id, rev: 1, clientEventId: "c2", blockId: "a", event: "choice.change", value: { value: "x" } });
    expect(good).toMatchObject({ ok: false, error: { code: "not_found" } });
    co.close();
  });
});
