// Core fixes from the V1 review: delete scrubbing, reads of new members, streamed text, member
// removal retries, hello, write order and page sizes.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { roomStub } from "../src/core/env";
import { scrubArchivedEvent } from "../src/ops/archive";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const actor = (id: string) => ({ id, level: "member" as const, kind: "human" as const });

describe("core review fixes", () => {
  it("remove a deleted message's text from quotes, reactions and the log", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token);
    const secret = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "my password is hunter2" });
    const topicId = secret.message.topicId;
    await cb.ok({ t: "react", groupId: gid, messageId: secret.message.id, emoji: "👍", on: true });
    const quote = await cb.ok({
      t: "send",
      groupId: gid,
      topicId,
      clientMsgId: clientMsgId(),
      body: "看这个",
      replyTo: { messageId: secret.message.id },
    });
    expect(quote.message.replyTo.excerpt).toBe("my password is hunter2");
    await ca.ok({ t: "delete", groupId: gid, messageId: secret.message.id });

    const h = await cb.request({ t: "history", groupId: gid, topicId });
    const q = h.messages.find((m: any) => m.id === quote.message.id);
    expect(q.replyTo).toEqual({ messageId: secret.message.id, authorId: a.id, excerpt: "" });
    const gone = h.messages.find((m: any) => m.id === secret.message.id);
    expect(gone.reactions).toBeUndefined();
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const log = JSON.stringify(page.events.filter((e: any) => e.type === "message.created"));
    expect(log).not.toContain("hunter2");
    ca.close();
    cb.close();
  });

  it("count history from before a member joined as read", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, []);
    const { c: ca } = await Client.connect(a.token);
    for (let i = 0; i < 5; i++) await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: `old ${i}` });
    expect((await api(`/v1/groups/${gid}/members`, { token: a.token, body: { memberId: b.id } })).status).toBe(200);
    const { c: cb, welcome } = await Client.connect(b.token);
    const g = welcome.groups.find((x: any) => x.group.id === gid);
    expect(g).toMatchObject({ unread: 0, mentions: 0 });
    expect(g.readSeq).toBe(g.lastSeq);
    // New messages after the join are unread.
    await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "new" });
    const again = (await api("/v1/groups", { token: b.token })).body.find((x: any) => x.group.id === gid);
    expect(again.unread).toBe(1);
    ca.close();
    cb.close();
  });

  it("store streamed text so a member who loads the message later sees it", async () => {
    const [a, b] = await Promise.all([createMember({ kind: "bot" }), createMember()]);
    const gid = await createGroup(b, [a.id]);
    const { c: ca } = await Client.connect(a.token);
    const sent = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "start", streaming: true });
    for (let i = 0; i < 5; i++) ca.send({ t: "delta", groupId: gid, messageId: sent.message.id, append: ` d${i}` });
    const room = roomStub(env, gid);
    let body = "";
    for (let i = 0; i < 100 && body !== "start d0 d1 d2 d3 d4"; i++) {
      await new Promise((r) => setTimeout(r, 50));
      const h = await room.history(gid, actor(b.id), sent.message.topicId);
      body = h.ok ? h.value.messages[0].body : "";
    }
    expect(body).toBe("start d0 d1 d2 d3 d4");
    const page = await room.fetchEvents(gid, actor(b.id), 0);
    const created = page.ok && page.value.events.find((e: any) => e.type === "message.created");
    expect((created as any).data.message.body).toBe("start d0 d1 d2 d3 d4");
    ca.close();
  });

  it("finish a member removal whose room call failed when the owner retries", async () => {
    const [owner, x] = await Promise.all([createMember({ level: "owner" }), createMember()]);
    const gid = await createGroup(owner, [x.id]);
    // D1 already removed the member, the room was never told (the room RPC failed).
    await env.DB.prepare("DELETE FROM group_members WHERE group_id = ? AND member_id = ?").bind(gid, x.id).run();
    const before = await roomStub(env, gid).memberIds(gid);
    expect(before.ok && before.value).toContain(x.id);
    const r = await api(`/v1/groups/${gid}/members/${x.id}`, { method: "DELETE", token: owner.token });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const after = await roomStub(env, gid).memberIds(gid);
    expect(after.ok && after.value).not.toContain(x.id);
    // Once both agree, a further removal is a 404.
    expect((await api(`/v1/groups/${gid}/members/${x.id}`, { method: "DELETE", token: owner.token })).status).toBe(404);
  });

  it("answer one hello per socket", async () => {
    const m = await createMember();
    const { c } = await Client.connect(m.token);
    c.send({ t: "hello", protocol: 1, client: { name: "test", version: "0" } });
    const err = await c.next((f) => f.t === "error");
    expect(err).toMatchObject({ error: { code: "invalid", message: "hello was already sent" } });
    expect(c.frames.filter((f) => f.t === "welcome")).toHaveLength(1);
    c.close();
  });

  it("send events committed while hello loads the directory after welcome", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const cb = await Client.open(b.token);
    // Hold b's directory load until a's message is committed and delivered.
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    const ns = (env as unknown as { MEMBER_GATEWAY: DurableObjectNamespace }).MEMBER_GATEWAY;
    const gw = ns.get(ns.idFromName(b.id));
    const inDo = runInDurableObject as unknown as (stub: unknown, fn: (inst: any) => void) => Promise<void>;
    await inDo(gw, (inst: any) => {
      const load = inst.loadDirectory.bind(inst);
      inst.loadDirectory = async (...args: unknown[]) => {
        const dir = await load(...args);
        await held;
        inst.loadDirectory = load;
        return dir;
      };
    });
    cb.send({ t: "hello", protocol: 1, client: { name: "test", version: "0" } });
    await new Promise((r) => setTimeout(r, 200));
    const sent = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "during hello" });
    await new Promise((r) => setTimeout(r, 200));
    release();
    const welcome = await cb.next((f) => f.t === "welcome");
    const got = await cb.waitEvent((e) => e.type === "message.created" && e.data.message.id === sent.message.id);
    expect(got.seq).toBeGreaterThan(welcome.groups.find((g: any) => g.group.id === gid).lastSeq);
    ca.close();
    cb.close();
  });

  it("run a socket's first writes in its order even when a later one arrives first", async () => {
    const m = await createMember();
    const gid = await createGroup(m, []);
    const room = roomStub(env, gid);
    const key = `test:${crypto.randomUUID()}`;
    const frame = (body: string) => ({ t: "send", rid: "x", groupId: gid, clientMsgId: clientMsgId(), body }) as never;
    const second = room.send(gid, actor(m.id), frame("second"), { key, n: 2 });
    await new Promise((r) => setTimeout(r, 50));
    const first = room.send(gid, actor(m.id), frame("first"), { key, n: 1 });
    const [r1, r2] = await Promise.all([first, second]);
    expect(r1.ok && r2.ok).toBe(true);
    if (r1.ok && r2.ok) expect(r1.value.message.createdSeq).toBeLessThan(r2.value.message.createdSeq);
  });

  it("cut fetch and history pages by size", async () => {
    const m = await createMember({ kind: "bot" });
    const gid = await createGroup(m, []);
    const room = roomStub(env, gid);
    const big = "x".repeat(30_000);
    const root = await room.send(gid, actor(m.id), { t: "send", rid: "x", groupId: gid, clientMsgId: clientMsgId(), body: big } as never);
    const topicId = root.ok ? root.value.message.topicId : "";
    for (let i = 0; i < 200; i++) {
      await room.send(gid, actor(m.id), { t: "send", rid: "x", groupId: gid, topicId, clientMsgId: clientMsgId(), body: big } as never);
    }
    const page = await room.fetchEvents(gid, actor(m.id), 0, 500);
    expect(page.ok).toBe(true);
    if (!page.ok) return;
    expect(page.value.hasMore).toBe(true);
    expect(page.value.events.length).toBeGreaterThan(50);
    expect(page.value.events.length).toBeLessThan(200);
    expect(JSON.stringify(page.value).length).toBeLessThan(5 * 1024 * 1024);
    const h = await room.history(gid, actor(m.id), topicId, undefined, 500);
    expect(h.ok && h.value.hasMore).toBe(true);
    expect(h.ok && h.value.messages.length).toBeLessThan(200);
  });

  it("sign an existing member in again with a code for that member", async () => {
    const owner = await createMember({ level: "owner" });
    const x = await createMember({ name: "小王" });
    const gid = await createGroup(owner, [x.id]);
    const inv = await api("/v1/invites", {
      token: owner.token,
      body: { name: "ignored", kind: "bot", level: "owner", memberId: x.id },
    });
    expect(inv.status, JSON.stringify(inv.body)).toBe(200);
    const red = await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "new mac" } });
    expect(red.status, JSON.stringify(red.body)).toBe(200);
    // Same member, its own name and level, not the ones in the request.
    expect(red.body.member).toMatchObject({ id: x.id, name: "小王", kind: "human", level: "member" });
    const { c, welcome } = await Client.connect(red.body.token);
    expect(welcome.me.id).toBe(x.id);
    expect(welcome.groups.map((g: any) => g.group.id)).toContain(gid);
    c.close();
    // The old token still works (another device), the code is single use.
    expect((await api("/v1/me", { token: x.token })).status).toBe(200);
    expect((await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "again" } })).status).toBe(401);
    const n = await env.DB.prepare("SELECT count(*) AS n FROM members WHERE name = '小王'").first<{ n: number }>();
    expect(n?.n).toBe(1);

    // Not for a deactivated member: neither making the code nor redeeming it.
    const inv2 = await api("/v1/invites", { token: owner.token, body: { name: "x", kind: "human", level: "member", memberId: x.id } });
    await env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(x.id).run();
    expect((await api("/v1/auth/redeem", { body: { inviteCode: inv2.body.code, deviceName: "d" } })).status).toBe(401);
    const inv3 = await api("/v1/invites", { token: owner.token, body: { name: "x", kind: "human", level: "member", memberId: x.id } });
    expect(inv3.status).toBe(400);
    const member = await createMember();
    expect((await api("/v1/invites", { token: member.token, body: { name: "x", kind: "human", level: "member", memberId: x.id } })).status).toBe(403);
  });

  it("scrub archived events of a deleted message like a live delete, quotes included", () => {
    const ids = new Set(["m1"]);
    const created = {
      seq: 1,
      type: "message.created",
      at: 1,
      actorId: "a",
      messageId: "m1",
      data: { message: { id: "m1", body: "secret", attachments: [{ fileId: "f" }], replyTo: { messageId: "m0", excerpt: "x" }, reactions: [] } },
    };
    const quote = {
      seq: 2,
      type: "message.created",
      at: 2,
      actorId: "b",
      messageId: "m2",
      data: { message: { id: "m2", body: "看", replyTo: { messageId: "m1", authorId: "a", excerpt: "secret" } } },
    };
    expect(scrubArchivedEvent(created, ids)).toBe(true);
    expect(created.data.message).toEqual({ id: "m1", body: "" });
    expect(scrubArchivedEvent(quote, ids)).toBe(true);
    expect(quote.data.message.replyTo.excerpt).toBe("");
    expect(scrubArchivedEvent(quote, ids)).toBe(false);
  });
});
