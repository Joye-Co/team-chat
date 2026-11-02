import { afterAll, describe, expect, it } from "vitest";
import { Client, api, clientMsgId, createGroup, createMember, waitUntil } from "./helpers";
import { protocolViolations } from "./helpers";
import { afterAll as afterAllCheck } from "vitest";

afterAllCheck(() => {
  expect(protocolViolations).toEqual([]);
});

async function setup(n = 2) {
  const members = await Promise.all(Array.from({ length: n }, (_, i) => createMember({ name: `U${i}` })));
  const gid = await createGroup(members[0], members.slice(1).map((m) => m.id));
  return { members, gid };
}

describe("websocket", () => {
  // Let close handlers (presence fan-out) finish before the runtime shuts down.
  afterAll(() => new Promise((r) => setTimeout(r, 300)));

  it("answers hello with welcome", async () => {
    const { members, gid } = await setup();
    const { c, welcome } = await Client.connect(members[0].token);
    expect(welcome).toMatchObject({
      t: "welcome",
      protocol: 1,
      me: { id: members[0].id },
      serverTime: expect.any(Number),
    });
    expect(welcome.members.some((m: any) => m.id === members[1].id)).toBe(true);
    expect(welcome.groups.find((g: any) => g.group.id === gid)).toMatchObject({ lastSeq: 0, unread: 0 });
    c.close();
  });

  it("rejects frames before hello and unsupported protocols", async () => {
    const { members, gid } = await setup();
    const c = await Client.open(members[0].token);
    const r = await c.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(r).toMatchObject({ t: "ack", ok: false, error: { code: "invalid" } });
    c.send({ t: "hello", protocol: 2, client: { name: "x", version: "1" } });
    await c.next((f) => f.t === "error" && f.error.code === "unsupported_protocol");
    await waitUntil(() => c.closed, 5000, "close");
  });

  it("answers ping with pong", async () => {
    const { members } = await setup(1);
    const { c } = await Client.connect(members[0].token);
    c.send({ t: "ping" });
    await c.next((f) => f.t === "pong");
    c.close();
  });

  it("send without topicId creates topic.created then message.created with seq 1 and 2", async () => {
    const { members, gid } = await setup();
    const { c: a } = await Client.connect(members[0].token);
    const { c: b } = await Client.connect(members[1].token);
    const result = await a.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "Ship the build\nmore" });
    expect(result.topic).toMatchObject({ title: "Ship the build", status: "open", lastSeq: 2, messageCount: 1 });
    expect(result.message).toMatchObject({ body: "Ship the build\nmore", createdSeq: 2, authorId: members[0].id });
    const e1 = await b.waitEvent((e) => e.seq === 1);
    const e2 = await b.waitEvent((e) => e.seq === 2);
    expect(e1).toMatchObject({ type: "topic.created", groupId: gid, actorId: members[0].id });
    expect(e2).toMatchObject({ type: "message.created", data: { message: { id: result.message.id } } });
    expect(e1.data.topic.rootMessageId).toBe(result.message.id);
    // The sender's own sockets get the events too.
    await a.waitEvent((e) => e.seq === 2);
    a.close();
    b.close();
  });

  it("dedupes by clientMsgId and writes no event", async () => {
    const { members, gid } = await setup();
    const { c } = await Client.connect(members[0].token);
    const id = clientMsgId();
    const first = await c.ok({ t: "send", groupId: gid, clientMsgId: id, body: "hi" });
    const second = await c.ok({ t: "send", groupId: gid, clientMsgId: id, body: "hi" });
    expect(second.duplicate).toBe(true);
    expect(second.message.id).toBe(first.message.id);
    expect(second.topic.id).toBe(first.topic.id);
    const page = await c.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events).toHaveLength(2);
    expect(page.lastSeq).toBe(2);
    c.close();
  });

  it("fetch pages in seq order and history returns newest first", async () => {
    const { members, gid } = await setup();
    const { c } = await Client.connect(members[0].token);
    const first = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "m0" });
    const topicId = first.topic.id;
    for (let i = 1; i < 6; i++) await c.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: `m${i}` });
    // 1 topic.created + 6 message.created = seq 1..7
    const p1 = await c.request({ t: "fetch", groupId: gid, afterSeq: 0, limit: 3 });
    expect(p1).toMatchObject({ t: "page", groupId: gid, hasMore: true, lastSeq: 7 });
    expect(p1.events.map((e: any) => e.seq)).toEqual([1, 2, 3]);
    const p2 = await c.request({ t: "fetch", groupId: gid, afterSeq: 3, limit: 3 });
    expect(p2.events.map((e: any) => e.seq)).toEqual([4, 5, 6]);
    const p3 = await c.request({ t: "fetch", groupId: gid, afterSeq: 6, limit: 3 });
    expect(p3).toMatchObject({ hasMore: false });
    expect(p3.events.map((e: any) => e.seq)).toEqual([7]);
    const tooBig = await c.request({ t: "fetch", groupId: gid, afterSeq: 0, limit: 501 });
    expect(tooBig).toMatchObject({ ok: false, error: { code: "invalid" } });

    const h1 = await c.request({ t: "history", groupId: gid, topicId, limit: 4 });
    expect(h1).toMatchObject({ t: "history", topicId, hasMore: true });
    expect(h1.messages.map((m: any) => m.body)).toEqual(["m5", "m4", "m3", "m2"]);
    const h2 = await c.request({ t: "history", groupId: gid, topicId, beforeSeq: h1.messages[3].createdSeq, limit: 4 });
    expect(h2.messages.map((m: any) => m.body)).toEqual(["m1", "m0"]);
    expect(h2.hasMore).toBe(false);
    c.close();
  });

  it("enforces edit (author only) and delete (author or owner) permissions", async () => {
    const author = await createMember();
    const other = await createMember();
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(author, [other.id, owner.id]);
    const { c: a } = await Client.connect(author.token);
    const { c: o } = await Client.connect(other.token);
    const { c: w } = await Client.connect(owner.token);
    const sent = await a.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "secret v1" });
    const mid = sent.message.id;

    expect(await o.request({ t: "edit", groupId: gid, messageId: mid, body: "x" })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    const edited = await a.ok({ t: "edit", groupId: gid, messageId: mid, body: "secret v2" });
    expect(edited.message).toMatchObject({ body: "secret v2", editedAt: expect.any(Number) });
    await o.waitEvent((e) => e.type === "message.updated" && e.data.body === "secret v2");

    expect(await o.request({ t: "delete", groupId: gid, messageId: mid })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    const del = await w.ok({ t: "delete", groupId: gid, messageId: mid });
    expect(del).toMatchObject({ messageId: mid, deletedAt: expect.any(Number) });
    await o.waitEvent((e) => e.type === "message.deleted" && e.data.messageId === mid);
    // The deleted text is gone from history and from the stored log.
    const h = await o.request({ t: "history", groupId: gid, topicId: sent.topic.id });
    expect(h.messages[0]).toMatchObject({ body: "", deletedAt: expect.any(Number) });
    const page = await o.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const bodies = page.events.map((e: any) => e.data.message?.body ?? e.data.body).filter((b: any) => b !== undefined);
    expect(bodies).toEqual(["", ""]);
    expect(await a.request({ t: "edit", groupId: gid, messageId: mid, body: "y" })).toMatchObject({
      ok: false,
      error: { code: "conflict" },
    });

    const up = await o.ok({ t: "topic.update", groupId: gid, topicId: sent.topic.id, status: "in_progress" });
    expect(up.topic.status).toBe("in_progress");
    await a.waitEvent((e) => e.type === "topic.updated" && e.data.status === "in_progress");
    for (const c of [a, o, w]) c.close();
  });

  it("forbids every frame type for a non-member", async () => {
    const { members, gid } = await setup();
    const { c: m } = await Client.connect(members[0].token);
    const sent = await m.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    const outsider = await createMember();
    const { c } = await Client.connect(outsider.token);
    const topicId = sent.topic.id;
    const messageId = sent.message.id;
    const withRid = [
      { t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" },
      { t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "x" },
      { t: "edit", groupId: gid, messageId, body: "x" },
      { t: "delete", groupId: gid, messageId },
      { t: "topic.update", groupId: gid, topicId, status: "done" },
      { t: "fetch", groupId: gid, afterSeq: 0 },
      { t: "history", groupId: gid, topicId },
    ];
    for (const f of withRid) {
      expect(await c.request(f), f.t).toMatchObject({ t: "ack", ok: false, error: { code: "forbidden" } });
    }
    const noRid = [
      { t: "typing", groupId: gid, topicId },
      { t: "read", groupId: gid, seq: 1 },
      { t: "delta", groupId: gid, messageId, append: "x" },
    ];
    for (const f of noRid) {
      const before = c.frames.length;
      c.send(f);
      await waitUntil(() => c.frames.slice(before).find((x) => x.t === "error" && x.error.code === "forbidden"));
    }
    // Also a group id that does not exist.
    expect(await c.request({ t: "fetch", groupId: "01NOPE", afterSeq: 0 })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    m.close();
    c.close();
  });

  it("tracks the read cursor, unread and mentions", async () => {
    const { members, gid } = await setup();
    const [a, b] = members;
    const { c: ca } = await Client.connect(a.token);
    const first = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "hello" });
    const topicId = first.topic.id;
    await ca.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "@b look", mentions: [b.id] });
    await ca.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "three" });
    const bad = await ca.request({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "x", mentions: ["01OUTSIDER"] });
    expect(bad).toMatchObject({ ok: false, error: { code: "invalid" } });

    const { c: cb, welcome } = await Client.connect(b.token);
    expect(welcome.groups.find((g: any) => g.group.id === gid)).toMatchObject({
      lastSeq: 4,
      readSeq: 0,
      unread: 3,
      mentions: 1,
    });
    cb.send({ t: "read", groupId: gid, seq: 3 });
    const dir = await cb.next((f) => f.t === "reads" && f.groupId === gid);
    expect(dir).toMatchObject({ readSeq: 3, unread: 1, mentions: 0 });
    // Cursor only moves forward, and never past lastSeq.
    cb.send({ t: "read", groupId: gid, seq: 1 });
    cb.send({ t: "read", groupId: gid, seq: 999 });
    const dir2 = await cb.next((f) => f.t === "reads" && f.readSeq === 4);
    expect(dir2).toMatchObject({ unread: 0 });
    const list = await api("/v1/groups", { token: b.token });
    expect(list.body.find((s: any) => s.group.id === gid)).toMatchObject({ readSeq: 4, unread: 0 });
    // The author's own messages are never unread for the author.
    const own = await api("/v1/groups", { token: a.token });
    expect(own.body.find((s: any) => s.group.id === gid)).toMatchObject({ unread: 0 });
    ca.close();
    cb.close();
  });

  it("delivers typing as a transient event, throttled, never stored", async () => {
    const { members, gid } = await setup();
    const { c: a } = await Client.connect(members[0].token);
    const { c: b } = await Client.connect(members[1].token);
    const sent = await a.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    a.send({ t: "typing", groupId: gid, topicId: sent.topic.id });
    a.send({ t: "typing", groupId: gid, topicId: sent.topic.id });
    const tr = await b.next((f) => f.t === "transient" && f.event.type === "typing");
    expect(tr.event).toMatchObject({ groupId: gid, topicId: sent.topic.id, memberId: members[0].id });
    // A marker after the typing frames: once it arrives, any second typing would have too.
    await a.ok({ t: "send", groupId: gid, topicId: sent.topic.id, clientMsgId: clientMsgId(), body: "marker" });
    await b.waitEvent((e) => e.seq === 3);
    expect(b.frames.filter((f) => f.t === "transient" && f.event.type === "typing")).toHaveLength(1);
    const page = await b.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.map((e: any) => e.type)).toEqual(["topic.created", "message.created", "message.created"]);
    a.close();
    b.close();
  });

  it("delivers events to every socket of a member", async () => {
    const { members, gid } = await setup();
    const { c: a } = await Client.connect(members[0].token);
    const { c: b1 } = await Client.connect(members[1].token);
    const { c: b2 } = await Client.connect(members[1].token);
    await a.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    await b1.waitEvent((e) => e.seq === 2);
    await b2.waitEvent((e) => e.seq === 2);
    for (const c of [a, b1, b2]) c.close();
  });

  it("streams: send streaming, coalesced deltas, final edit stored", async () => {
    const bot = await createMember({ kind: "bot" });
    const human = await createMember();
    const gid = await createGroup(human, [bot.id]);
    const { c: cb } = await Client.connect(bot.token);
    const { c: ch } = await Client.connect(human.token);
    const sent = await cb.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", streaming: true, title: "Run" });
    expect(sent.message.streaming).toBe(true);
    const mid = sent.message.id;
    for (const part of ["Hel", "lo ", "wor", "ld"]) cb.send({ t: "delta", groupId: gid, messageId: mid, append: part });
    const final = await cb.ok({ t: "edit", groupId: gid, messageId: mid, body: "Hello world", streaming: false });
    expect(final.message.streaming).toBeUndefined();
    await ch.waitEvent((e) => e.type === "message.updated" && e.data.streaming === false);
    const deltas = ch.frames.filter((f) => f.t === "transient" && f.event.type === "message.delta");
    expect(deltas.length).toBeGreaterThanOrEqual(1);
    expect(deltas.length).toBeLessThan(4);
    expect(deltas.map((d) => d.event.append).join("")).toBe("Hello world");
    // Deltas arrive before the final edit.
    const idxFinal = ch.frames.findIndex((f) => f.t === "event" && f.event.type === "message.updated");
    const idxDelta = ch.frames.findIndex((f) => f.t === "transient" && f.event.type === "message.delta");
    expect(idxDelta).toBeLessThan(idxFinal);
    const page = await ch.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.map((e: any) => e.type)).toEqual(["topic.created", "message.created", "message.updated"]);
    // Only the streaming author may send deltas; a finished message takes none.
    ch.send({ t: "delta", groupId: gid, messageId: mid, append: "x" });
    await ch.next((f) => f.t === "error" && f.error.code === "forbidden");
    cb.send({ t: "delta", groupId: gid, messageId: mid, append: "x" });
    await cb.next((f) => f.t === "error" && f.error.code === "conflict");
    cb.close();
    ch.close();
  });

  it("rate limits sends per member", async () => {
    const { members, gid } = await setup(1);
    const { c } = await Client.connect(members[0].token);
    const first = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    const rids: string[] = [];
    for (let i = 0; i < 40; i++) {
      const rid = c.rid();
      rids.push(rid);
      c.send({ t: "send", rid, groupId: gid, topicId: first.topic.id, clientMsgId: clientMsgId(), body: `${i}` });
    }
    const acks = await waitUntil(() => {
      const a = c.frames.filter((f) => f.t === "ack" && rids.includes(f.rid));
      return a.length === rids.length ? a : undefined;
    }, 15000);
    expect(acks.filter((a) => !a.ok && a.error.code === "rate_limited").length).toBeGreaterThan(0);
    expect(acks.filter((a) => a.ok).length).toBeGreaterThan(5);
    c.close();
  });

  it("rejects frames over 1 MiB and invalid frames", async () => {
    const { members, gid } = await setup(1);
    const { c } = await Client.connect(members[0].token);
    c.send(JSON.stringify({ t: "send", rid: "big", groupId: gid, clientMsgId: "x", body: "a".repeat(1_100_000) }));
    await c.next((f) => f.t === "error" && f.error.code === "too_large");
    const r = await c.request({ t: "send", groupId: gid, clientMsgId: "x", body: "a".repeat(40_000) });
    expect(r).toMatchObject({ ok: false, error: { code: "invalid" } });
    c.send("not json");
    await c.next((f) => f.t === "error" && f.error.code === "invalid");
    const unknown = await c.request({ t: "nope" });
    expect(unknown).toMatchObject({ ok: false, error: { code: "invalid" } });
    c.close();
  });

  it("pushes directory frames and memberAdded events on membership changes", async () => {
    const { members, gid } = await setup();
    const newcomer = await createMember();
    const { c: n } = await Client.connect(newcomer.token);
    const { c: a } = await Client.connect(members[0].token);
    await api(`/v1/groups/${gid}/members`, { token: members[0].token, body: { memberId: newcomer.id } });
    const dir = await n.next((f) => f.t === "directory" && f.groups?.some((g: any) => g.group.id === gid));
    expect(dir.groups[0].group.memberIds).toContain(newcomer.id);
    await a.waitEvent((e) => e.type === "group.memberAdded" && e.data.memberId === newcomer.id);
    // The newcomer can now use the group.
    await n.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "hi" });
    await api(`/v1/groups/${gid}/members/${newcomer.id}`, { method: "DELETE", token: newcomer.token });
    await n.next((f) => f.t === "directory" && f.removedGroupIds?.includes(gid));
    expect(await n.request({ t: "fetch", groupId: gid, afterSeq: 0 })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    n.close();
    a.close();
  });

  it("sends presence when a member comes online and goes offline", async () => {
    const { members } = await setup();
    const { c: a } = await Client.connect(members[0].token);
    const { c: b } = await Client.connect(members[1].token);
    await a.next((f) => f.t === "transient" && f.event.type === "presence" && f.event.memberId === members[1].id && f.event.online);
    b.close();
    await a.next((f) => f.t === "transient" && f.event.type === "presence" && f.event.memberId === members[1].id && !f.event.online);
    // A new socket of a learns the presence snapshot after welcome.
    const { c: b2 } = await Client.connect(members[1].token);
    const { c: a2 } = await Client.connect(members[0].token);
    await a2.next((f) => f.t === "transient" && f.event.type === "presence" && f.event.memberId === members[1].id && f.event.online);
    for (const c of [a, a2, b2]) c.close();
  });
});
