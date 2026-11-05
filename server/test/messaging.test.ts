import { afterAll, describe, expect, it } from "vitest";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  // Let close handlers (presence fan-out) finish before the runtime shuts down.
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

async function setup(n = 3) {
  const members = await Promise.all(Array.from({ length: n }, (_, i) => createMember({ name: `M${i}` })));
  const gid = await createGroup(members[0], members.slice(1).map((m) => m.id));
  const clients = await Promise.all(members.map(async (m) => (await Client.connect(m.token)).c));
  return { members, gid, clients };
}

function closeAll(cs: Client[]) {
  for (const c of cs) c.close();
}

const card = (title = "Build", extra: Record<string, unknown> = {}) => ({
  v: 1,
  title,
  blocks: [{ type: "text", text: "**ok**" }],
  actions: [
    { id: "approve", label: "OK", value: { go: true } },
    { id: "reject", label: "No" },
  ],
  ...extra,
});

describe("topics: owner and waitingOn", () => {
  it("set the creator as owner, validate members, dedupe waitingOn and carry changes in topic.updated", async () => {
    const { members, gid, clients } = await setup();
    const [a, b, c] = members;
    const [ca, cb] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "plan" });
    expect(r.topic).toMatchObject({ ownerId: a.id, waitingOn: [], lastMessageAt: r.message.createdAt });
    const tid = r.topic.id;

    const up = await cb.ok({ t: "topic.update", groupId: gid, topicId: tid, waitingOn: [b.id, c.id, b.id], ownerId: b.id });
    expect(up.topic).toMatchObject({ waitingOn: [b.id, c.id], ownerId: b.id });
    const ev = await ca.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === tid);
    expect(ev.data).toEqual({ topicId: tid, waitingOn: [b.id, c.id], ownerId: b.id });

    const outsider = await createMember();
    for (const bad of [{ waitingOn: [outsider.id] }, { ownerId: outsider.id }]) {
      expect(await ca.request({ t: "topic.update", groupId: gid, topicId: tid, ...bad })).toMatchObject({
        ok: false,
        error: { code: "invalid" },
      });
    }
    const cleared = await ca.ok({ t: "topic.update", groupId: gid, topicId: tid, ownerId: null, waitingOn: [] });
    expect(cleared.topic).toMatchObject({ ownerId: null, waitingOn: [] });
    expect(await ca.request({ t: "topic.update", groupId: gid, topicId: tid })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    // A non-member of the group cannot update its topics.
    const { c: co } = await Client.connect(outsider.token);
    expect(await co.request({ t: "topic.update", groupId: gid, topicId: tid, status: "done" })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    closeAll([...clients, co]);
  });

  it("emit topic.updated with the recomputed lastMessageAt when the newest message is deleted", async () => {
    const { gid, clients } = await setup(2);
    const [ca, cb] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "one" });
    const tid = r.topic.id;
    await new Promise((res) => setTimeout(res, 5));
    const second = await ca.ok({ t: "send", groupId: gid, topicId: tid, clientMsgId: clientMsgId(), body: "two" });
    await ca.ok({ t: "delete", groupId: gid, messageId: second.message.id });
    const ev = await cb.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === tid);
    expect(ev.data).toEqual({ topicId: tid, lastMessageAt: r.message.createdAt });
    await ca.ok({ t: "delete", groupId: gid, messageId: r.message.id });
    const ev2 = await cb.waitEvent((e) => e.type === "topic.updated" && e.seq > ev.seq);
    expect(ev2.data).toEqual({ topicId: tid });
    closeAll(clients);
  });
});

describe("read positions per topic", () => {
  it("read one topic without marking the others, sync reads to every connection and keep totals right", async () => {
    const { members, gid, clients } = await setup(2);
    const [a, b] = members;
    const [ca, cb] = clients;
    const t1 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "t1" });
    const t1b = await ca.ok({
      t: "send",
      groupId: gid,
      topicId: t1.topic.id,
      clientMsgId: clientMsgId(),
      body: "@b",
      mentions: [b.id],
    });
    const t2 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "t2" });

    const { c: cb2, welcome } = await Client.connect(b.token);
    const s = welcome.groups.find((g: any) => g.group.id === gid);
    expect(s).toMatchObject({ unread: 3, mentions: 1, readSeq: 0, muted: false, notify: "mentions" });
    const byId = Object.fromEntries(s.topics.map((t: any) => [t.topicId, t]));
    expect(byId[t1.topic.id]).toEqual({ topicId: t1.topic.id, readSeq: 0, unread: 2, mentions: 1, following: true });
    expect(byId[t2.topic.id]).toEqual({ topicId: t2.topic.id, readSeq: 0, unread: 1, mentions: 0 });

    cb.send({ t: "read", groupId: gid, topicId: t1.topic.id, seq: t1b.message.createdSeq });
    for (const c of [cb, cb2]) {
      const f = await c.next((f) => f.t === "reads" && f.groupId === gid);
      expect(f).toEqual({
        t: "reads",
        groupId: gid,
        readSeq: 0,
        unread: 1,
        mentions: 0,
        topics: [{ topicId: t1.topic.id, readSeq: t1b.message.createdSeq, unread: 0, mentions: 0, following: true }],
      });
    }
    // Positions never move back; a seq past lastSeq is cut.
    cb.send({ t: "read", groupId: gid, topicId: t1.topic.id, seq: 1 });
    cb.send({ t: "read", groupId: gid, topicId: t2.topic.id, seq: 9999 });
    const f2 = await cb.next((f) => f.t === "reads" && f.topics[0]?.topicId === t2.topic.id);
    expect(f2).toMatchObject({ unread: 0, mentions: 0, topics: [{ readSeq: t2.message.createdSeq, unread: 0 }] });
    expect(cb.frames.filter((f) => f.t === "reads")).toHaveLength(2);
    expect(await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 })).toMatchObject({ t: "page" });

    const list = await api("/v1/groups", { token: b.token });
    const s2 = list.body.find((g: any) => g.group.id === gid);
    expect(s2).toMatchObject({ readSeq: 0, unread: 0, mentions: 0 });
    expect(s2.topics).toEqual([{ topicId: t1.topic.id, readSeq: t1b.message.createdSeq, unread: 0, mentions: 0, following: true }]);

    // A new message counts again; the group-wide read clears it.
    const t3 = await ca.ok({ t: "send", groupId: gid, topicId: t2.topic.id, clientMsgId: clientMsgId(), body: "more" });
    cb.send({ t: "read", groupId: gid, seq: t3.message.createdSeq });
    const f3 = await cb.next((f) => f.t === "reads" && f.readSeq === t3.message.createdSeq);
    expect(f3).toMatchObject({ unread: 0, mentions: 0 });
    // Unknown topic is not found; a non-member is forbidden.
    cb.send({ t: "read", groupId: gid, topicId: "01NOPE", seq: 1 });
    await cb.next((f) => f.t === "error" && f.error.code === "not_found");
    void a;
    closeAll([...clients, cb2]);
  });

  it("follow and unfollow a topic; auto-follow on mention, waitingOn and owner", async () => {
    const { members, gid, clients } = await setup(3);
    const [, b, c] = members;
    const [ca, cb, cc] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    const tid = r.topic.id;
    const st = await cb.ok({ t: "topic.follow", groupId: gid, topicId: tid, follow: true });
    expect(st).toMatchObject({ topicId: tid, unread: 1, following: true });
    await cb.next((f) => f.t === "reads" && f.topics[0]?.following === true);
    const off = await cb.ok({ t: "topic.follow", groupId: gid, topicId: tid, follow: false });
    expect(off.following).toBeUndefined();
    // Mention re-follows after an explicit unfollow.
    await ca.ok({ t: "send", groupId: gid, topicId: tid, clientMsgId: clientMsgId(), body: "@b", mentions: [b.id] });
    await ca.ok({ t: "topic.update", groupId: gid, topicId: tid, waitingOn: [c.id] });
    const list = async (token: string) =>
      (await api("/v1/groups", { token })).body.find((g: any) => g.group.id === gid).topics;
    expect((await list(b.token))[0]).toMatchObject({ topicId: tid, following: true });
    expect((await list(c.token))[0]).toMatchObject({ topicId: tid, following: true });
    expect(await cc.request({ t: "topic.follow", groupId: gid, topicId: "01NOPE", follow: true })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    const outsider = await createMember();
    const { c: co } = await Client.connect(outsider.token);
    expect(await co.request({ t: "topic.follow", groupId: gid, topicId: tid, follow: true })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    closeAll([...clients, co]);
  });
});

describe("quotes and cards", () => {
  it("fill quotes from the quoted message and reject deleted or foreign ones", async () => {
    const { members, gid, clients } = await setup(2);
    const [a] = members;
    const [ca, cb] = clients;
    const long = "长".repeat(250);
    const q = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: long });
    const r = await cb.ok({
      t: "send",
      groupId: gid,
      topicId: q.topic.id,
      clientMsgId: clientMsgId(),
      body: "re",
      replyTo: { messageId: q.message.id, authorId: "forged", excerpt: "forged" },
    });
    expect(r.message.replyTo).toEqual({ messageId: q.message.id, authorId: a.id, excerpt: "长".repeat(200) });
    expect(r.message.kind).toBe("text");
    const other = await createGroup(a, []);
    const foreign = await ca.ok({ t: "send", groupId: other, clientMsgId: clientMsgId(), body: "elsewhere" });
    expect(
      await cb.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", replyTo: { messageId: foreign.message.id } }),
    ).toMatchObject({ ok: false, error: { code: "not_found" } });
    await ca.ok({ t: "delete", groupId: gid, messageId: q.message.id });
    expect(
      await cb.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x", replyTo: { messageId: q.message.id } }),
    ).toMatchObject({ ok: false, error: { code: "invalid" } });
    closeAll(clients);
  });

  it("store cards with kind card and the title as fallback body; enforce size and author-only card edits", async () => {
    const { gid, clients } = await setup(2);
    const [ca, cb] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", card: card() });
    expect(r.message).toMatchObject({ kind: "card", body: "Build", card: { title: "Build" } });
    expect(r.topic.title).toBe("Build");
    const quote = await cb.ok({
      t: "send",
      groupId: gid,
      topicId: r.topic.id,
      clientMsgId: clientMsgId(),
      body: "?",
      replyTo: { messageId: r.message.id },
    });
    expect(quote.message.replyTo.excerpt).toBe("Build");

    const huge = card("Big", { blocks: Array.from({ length: 10 }, () => ({ type: "text", text: "x".repeat(7000) })) });
    expect(await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", card: huge })).toMatchObject({
      ok: false,
      error: { code: "too_large" },
    });
    expect(
      await ca.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", card: { v: 1, title: "x", blocks: [{ type: "nope" }] } }),
    ).toMatchObject({ ok: false, error: { code: "invalid" } });

    // Edits: author only, card messages only.
    const next = card("Build done", { tone: "success", actions: [] });
    expect(await cb.request({ t: "edit", groupId: gid, messageId: r.message.id, body: "", card: next })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    expect(await cb.request({ t: "edit", groupId: gid, messageId: quote.message.id, body: "?", card: next })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    const ed = await ca.ok({ t: "edit", groupId: gid, messageId: r.message.id, body: "", card: next });
    expect(ed.message).toMatchObject({ kind: "card", body: "Build done", card: { title: "Build done", tone: "success" } });
    const ev = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === r.message.id);
    expect(ev.data).toMatchObject({ body: "Build done", card: { title: "Build done" } });
    // A text edit keeps the card.
    const kept = await ca.ok({ t: "edit", groupId: gid, messageId: r.message.id, body: "fallback" });
    expect(kept.message).toMatchObject({ body: "fallback", card: { title: "Build done" } });
    closeAll(clients);
  });

  it("store card.action as card.actioned with the action's own value, rate limited", async () => {
    const { members, gid, clients } = await setup(2);
    const [, b] = members;
    const [ca, cb] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "deploy?", card: card() });
    const press = await cb.request({ t: "card.action", groupId: gid, messageId: r.message.id, actionId: "approve", value: { go: false } });
    expect(press).toEqual({ t: "ack", rid: press.rid, ok: true });
    const ev = await ca.waitEvent((e) => e.type === "card.actioned");
    expect(ev).toMatchObject({ actorId: b.id, data: { messageId: r.message.id, actionId: "approve", value: { go: true } } });
    expect(await cb.request({ t: "card.action", groupId: gid, messageId: r.message.id, actionId: "reject" })).toMatchObject({
      ok: false,
      error: { code: "rate_limited" },
    });
    expect(await ca.request({ t: "card.action", groupId: gid, messageId: r.message.id, actionId: "missing" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    const text = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "plain" });
    expect(await ca.request({ t: "card.action", groupId: gid, messageId: text.message.id, actionId: "approve" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    // The stored log has the event; a deleted card cannot be pressed and its card is scrubbed.
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.some((e: any) => e.type === "card.actioned" && e.data.value?.go === true)).toBe(true);
    await ca.ok({ t: "delete", groupId: gid, messageId: r.message.id });
    expect(await ca.request({ t: "card.action", groupId: gid, messageId: r.message.id, actionId: "approve" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    const page2 = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const created = page2.events.find((e: any) => e.type === "message.created" && e.data.message.id === r.message.id);
    expect(created.data.message.card).toBeUndefined();
    closeAll(clients);
  });
});

describe("notifications and prefs", () => {
  it("mute and unmute a topic: ack, reads to my other connections, only mentions notify", async () => {
    const { members, gid, clients } = await setup(3);
    const [, b, c] = members;
    const [ca, cb, cc] = clients;
    const { c: cc2 } = await Client.connect(c.token);
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    const tid = r.topic.id;
    await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: c.token, body: { notify: "all" } });
    await cc.next((f) => f.t === "directory" && f.groups?.[0]?.notify === "all");
    await cc.ok({ t: "topic.follow", groupId: gid, topicId: tid, follow: true });

    // Mute: the ack is my TopicState, my other connection gets reads, following stays.
    const st = await cc.ok({ t: "topic.mute", groupId: gid, topicId: tid, muted: true });
    expect(st).toMatchObject({ topicId: tid, muted: true, following: true });
    const reads = await cc2.next((f) => f.t === "reads" && f.topics[0]?.muted === true);
    expect(reads).toMatchObject({ groupId: gid, topics: [{ topicId: tid, muted: true, following: true }] });
    const list = async () => (await api("/v1/groups", { token: c.token })).body.find((g: any) => g.group.id === gid).topics;
    expect(await list()).toContainEqual(expect.objectContaining({ topicId: tid, muted: true }));

    // In the muted topic: message, waiting and result are suppressed; a mention still notifies.
    const m1 = await ca.ok({ t: "send", groupId: gid, topicId: tid, clientMsgId: clientMsgId(), body: "plain" });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m1.message.createdSeq)).notify).toBeUndefined();
    const w = await ca.ok({ t: "topic.update", groupId: gid, topicId: tid, waitingOn: [c.id] });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === w.topic.lastSeq)).notify).toBeUndefined();
    const done = await ca.ok({ t: "topic.update", groupId: gid, topicId: tid, status: "done" });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === done.topic.lastSeq)).notify).toBeUndefined();
    const m2 = await ca.ok({ t: "send", groupId: gid, topicId: tid, clientMsgId: clientMsgId(), body: "@c", mentions: [c.id] });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m2.message.createdSeq)).notify).toBe("mention");
    // Other members are not affected by my mute.
    expect((await cb.next((f) => f.t === "event" && f.event.seq === w.topic.lastSeq)).notify).toBeUndefined();
    // Auto-follow (mention, waitingOn) does not unmute.
    expect(await list()).toContainEqual(expect.objectContaining({ topicId: tid, muted: true, following: true }));

    // Unmute: messages notify again at level all; following is unchanged.
    const off = await cc.ok({ t: "topic.mute", groupId: gid, topicId: tid, muted: false });
    expect(off.muted).toBeUndefined();
    expect(off.following).toBe(true);
    await cc2.next((f) => f.t === "reads" && f.topics[0]?.topicId === tid && f.topics[0]?.muted === undefined);
    const m3 = await ca.ok({ t: "send", groupId: gid, topicId: tid, clientMsgId: clientMsgId(), body: "again" });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m3.message.createdSeq)).notify).toBe("message");

    // Validation: missing muted is invalid, an unknown topic not found, an outsider forbidden.
    expect(await cc.request({ t: "topic.mute", groupId: gid, topicId: tid })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await cc.request({ t: "topic.mute", groupId: gid, topicId: "01NOPE", muted: true })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    const outsider = await createMember();
    const { c: co } = await Client.connect(outsider.token);
    expect(await co.request({ t: "topic.mute", groupId: gid, topicId: tid, muted: true })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    void b;
    closeAll([...clients, cc2, co]);
  });

  it("mark live events with notify per member and honor mute and notify levels", async () => {
    const { members, gid, clients } = await setup(3);
    const [a, b, c] = members;
    const [ca, cb, cc] = clients;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b", mentions: [b.id] });
    const fb = await cb.next((f) => f.t === "event" && f.event.seq === r.message.createdSeq);
    expect(fb.notify).toBe("mention");
    const fc = await cc.next((f) => f.t === "event" && f.event.seq === r.message.createdSeq);
    expect(fc.notify).toBeUndefined();
    const fa = await ca.next((f) => f.t === "event" && f.event.seq === r.message.createdSeq);
    expect(fa.notify).toBeUndefined();

    // c follows, then the topic is done: result. c is added to waitingOn: waiting.
    await cc.ok({ t: "topic.follow", groupId: gid, topicId: r.topic.id, follow: true });
    const w = await ca.ok({ t: "topic.update", groupId: gid, topicId: r.topic.id, waitingOn: [c.id] });
    const fw = await cc.next((f) => f.t === "event" && f.event.seq === w.topic.lastSeq);
    expect(fw.notify).toBe("waiting");
    const again = await ca.ok({ t: "topic.update", groupId: gid, topicId: r.topic.id, waitingOn: [c.id, b.id] });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === again.topic.lastSeq)).notify).toBeUndefined();
    expect((await cb.next((f) => f.t === "event" && f.event.seq === again.topic.lastSeq)).notify).toBe("waiting");
    const done = await ca.ok({ t: "topic.update", groupId: gid, topicId: r.topic.id, status: "done" });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === done.topic.lastSeq)).notify).toBe("result");

    // Prefs: c sets level all, then mutes; mentions still come through a mute.
    const p = await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: c.token, body: { notify: "all" } });
    expect(p.status).toBe(200);
    expect(p.body).toMatchObject({ group: { id: gid }, muted: false, notify: "all" });
    await cc.next((f) => f.t === "directory" && f.groups?.[0]?.notify === "all");
    const m1 = await ca.ok({ t: "send", groupId: gid, topicId: r.topic.id, clientMsgId: clientMsgId(), body: "fyi" });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m1.message.createdSeq)).notify).toBe("message");
    expect((await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: c.token, body: { muted: true } })).body).toMatchObject(
      { muted: true, notify: "all" },
    );
    const m2 = await ca.ok({ t: "send", groupId: gid, topicId: r.topic.id, clientMsgId: clientMsgId(), body: "fyi2" });
    const m3 = await ca.ok({ t: "send", groupId: gid, topicId: r.topic.id, clientMsgId: clientMsgId(), body: "@c", mentions: [c.id] });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m2.message.createdSeq)).notify).toBeUndefined();
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m3.message.createdSeq)).notify).toBe("mention");
    await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: c.token, body: { muted: false, notify: "none" } });
    const m4 = await ca.ok({ t: "send", groupId: gid, topicId: r.topic.id, clientMsgId: clientMsgId(), body: "@c", mentions: [c.id] });
    expect((await cc.next((f) => f.t === "event" && f.event.seq === m4.message.createdSeq)).notify).toBeUndefined();
    // Pages never carry notify.
    const page = await cc.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(JSON.stringify(page)).not.toContain('"notify"');

    // Prefs need group membership and a valid body; the change is audited.
    const outsider = await createMember();
    expect((await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: outsider.token, body: { muted: true } })).status).toBe(403);
    expect((await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: c.token, body: { notify: "loud" } })).status).toBe(400);
    expect((await api(`/v1/groups/01NOPE/prefs`, { method: "PATCH", token: c.token, body: {} })).status).toBe(404);
    void a;
    closeAll(clients);
  });

  it("default a DM to level all", async () => {
    const a = await createMember();
    const b = await createMember();
    const dm = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    expect(dm.body).toMatchObject({ notify: "all", muted: false });
    const gid = dm.body.group.id;
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token);
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "hi" });
    expect((await cb.next((f) => f.t === "event" && f.event.seq === r.message.createdSeq)).notify).toBe("message");
    await waitUntil(() => true);
    closeAll([ca, cb]);
  });
});
