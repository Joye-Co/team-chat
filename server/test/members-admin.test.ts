import { afterAll, describe, expect, it } from "vitest";
import { env } from "cloudflare:test";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

describe("PATCH /v1/members/:id", () => {
  it("let members change their own name and role only", async () => {
    const m = await createMember();
    const other = await createMember();
    const r = await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body: { name: "New", role: "设计" } });
    expect(r.status).toBe(200);
    expect(r.body).toMatchObject({ id: m.id, name: "New", role: "设计" });
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body: { role: null } })).body.role).toBeNull();
    for (const body of [{ level: "owner" }, { deactivated: true }]) {
      expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body })).status).toBe(403);
    }
    expect((await api(`/v1/members/${other.id}`, { method: "PATCH", token: m.token, body: { name: "x" } })).status).toBe(403);
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body: { role: "x".repeat(65) } })).status).toBe(400);
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", body: { name: "x" } })).status).toBe(401);
  });

  it("let an owner change level, keep one active owner and push directory frames", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    await createGroup(owner, [m.id]);
    const { c: cm } = await Client.connect(m.token);
    const up = await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { level: "owner" } });
    expect(up.body).toMatchObject({ level: "owner" });
    await cm.next((f) => f.t === "directory" && f.members?.some((x: any) => x.id === m.id && x.level === "owner"));
    expect((await api(`/v1/members/01NOPE`, { method: "PATCH", token: owner.token, body: { name: "x" } })).status).toBe(404);

    // Demote every owner but one, then the last one cannot go.
    const owners = (await env.DB.prepare("SELECT id FROM members WHERE level = 'owner' AND deactivated = 0").all<{ id: string }>())
      .results.map((r) => r.id)
      .filter((id) => id !== owner.id);
    for (const id of owners) {
      expect((await api(`/v1/members/${id}`, { method: "PATCH", token: owner.token, body: { level: "member" } })).status).toBe(200);
    }
    const last = await api(`/v1/members/${owner.id}`, { method: "PATCH", token: owner.token, body: { level: "member" } });
    expect(last).toMatchObject({ status: 409, body: { error: { code: "conflict" } } });
    const self = await api(`/v1/members/${owner.id}`, { method: "PATCH", token: owner.token, body: { deactivated: true } });
    expect(self.status).toBe(409);
    // The demoted member's open socket loses owner rights at once.
    const { c: co } = await Client.connect(owner.token);
    const sent = await co.ok({ t: "send", groupId: (await createGroup(owner, [m.id])), clientMsgId: clientMsgId(), body: "x" });
    expect(
      await cm.request({ t: "delete", groupId: sent.message.groupId, messageId: sent.message.id }),
    ).toMatchObject({ ok: false, error: { code: "forbidden" } });
    // Restore an owner for the other tests.
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { level: "owner" } })).status).toBe(200);
    expect((await cm.request({ t: "delete", groupId: sent.message.groupId, messageId: sent.message.id })).ok).toBe(true);
    cm.close();
    co.close();
  });

  it("deactivate: revoke tokens, close sockets and take presence away", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    const gid = await createGroup(owner, [m.id]);
    const { c: co } = await Client.connect(owner.token);
    const { c: cm } = await Client.connect(m.token);
    await co.next((f) => f.t === "transient" && f.event.type === "presence" && f.event.memberId === m.id && f.event.online);
    const r = await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { deactivated: true } });
    expect(r.body).toMatchObject({ id: m.id, deactivated: true });
    await waitUntil(() => cm.closed, 5000, "socket close");
    await co.next((f) => f.t === "transient" && f.event.type === "presence" && f.event.memberId === m.id && !f.event.online);
    await co.next((f) => f.t === "directory" && f.members?.some((x: any) => x.id === m.id && x.deactivated));
    expect((await api("/v1/me", { token: m.token })).status).toBe(401);
    const tokens = await env.DB.prepare("SELECT revoked_at FROM tokens WHERE member_id = ?").bind(m.id).all<{ revoked_at: number | null }>();
    expect(tokens.results.every((t) => t.revoked_at !== null)).toBe(true);
    void gid;
    co.close();
  });
});

describe("GET /v1/audit", () => {
  it("list audited writes newest first for owners only, with paging", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { role: "QA" } });
    const gid = await createGroup(owner, [m.id]);
    await api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: m.token, body: { muted: true } });

    expect((await api("/v1/audit", { token: m.token })).status).toBe(403);
    const r = await api("/v1/audit?limit=3", { token: owner.token });
    expect(r.status).toBe(200);
    expect(r.body.entries).toHaveLength(3);
    expect(r.body.hasMore).toBe(true);
    const [prefs, create, update] = r.body.entries;
    expect(prefs).toMatchObject({
      action: "prefs.update",
      actorId: m.id,
      credential: { kind: "token", id: m.tokenId },
      target: { kind: "group", id: gid },
      groupId: gid,
      details: { muted: [false, true] },
    });
    expect(create).toMatchObject({ action: "group.create", target: { kind: "group", id: gid } });
    expect(update).toMatchObject({
      action: "member.update",
      actorId: owner.id,
      target: { kind: "member", id: m.id },
      details: { role: [null, "QA"] },
    });
    const older = await api(`/v1/audit?limit=1&before=${update.id}`, { token: owner.token });
    expect(Number(older.body.entries[0].id)).toBeLessThan(Number(update.id));
    expect(JSON.stringify(r.body)).not.toContain(m.token);
    for (const q of ["limit=0", "limit=201", "before=abc"]) {
      expect((await api(`/v1/audit?${q}`, { token: owner.token })).status).toBe(400);
    }
  });
});

describe("POST /v1/groups/:id/messages", () => {
  it("run the send path: events, dedupe, quotes, cards and audit", async () => {
    const bot = await createMember({ kind: "bot" });
    const h = await createMember();
    const gid = await createGroup(h, [bot.id]);
    const { c: ch } = await Client.connect(h.token);
    const id = clientMsgId();
    const post = (body: unknown, token = bot.token, g = gid) => api(`/v1/groups/${g}/messages`, { token, body });
    const r = await post({ clientMsgId: id, body: "构建完成", mentions: [h.id] });
    expect(r.status).toBe(200);
    expect(r.body).toMatchObject({ message: { authorId: bot.id, body: "构建完成", kind: "text" }, topic: { ownerId: bot.id } });
    const f = await ch.next((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.id === r.body.message.id);
    expect(f.notify).toBe("mention");
    const dup = await post({ clientMsgId: id, body: "构建完成" });
    expect(dup.body).toMatchObject({ duplicate: true, message: { id: r.body.message.id } });

    const withCard = await post({
      clientMsgId: clientMsgId(),
      topicId: r.body.topic.id,
      body: "",
      card: { v: 1, title: "CI", blocks: [{ type: "progress", value: 0.5 }] },
      replyTo: { messageId: r.body.message.id },
    });
    expect(withCard.body.message).toMatchObject({ kind: "card", body: "CI", replyTo: { authorId: bot.id } });

    const audit = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action = 'message.post' AND actor_id = ?")
      .bind(bot.id)
      .first<{ n: number }>();
    expect(audit?.n).toBe(2);

    // Failures: not a member, unknown group, bad body, oversized card, bad mention.
    const outsider = await createMember({ kind: "bot" });
    expect((await post({ clientMsgId: clientMsgId(), body: "x" }, outsider.token)).status).toBe(403);
    expect((await post({ clientMsgId: clientMsgId(), body: "x" }, bot.token, "01NOPE")).status).toBe(404);
    expect((await post({ body: "no id" })).status).toBe(400);
    expect((await post({ clientMsgId: clientMsgId(), body: "x" }, "tc_bad")).status).toBe(401);
    const huge = { v: 1, title: "x", blocks: Array.from({ length: 10 }, () => ({ type: "text", text: "y".repeat(7000) })) };
    expect((await post({ clientMsgId: clientMsgId(), body: "", card: huge })).status).toBe(413);
    expect((await post({ clientMsgId: clientMsgId(), body: "x", mentions: [outsider.id] })).status).toBe(400);
    ch.close();
  });

  it("rate limit posts per credential", async () => {
    const bot = await createMember({ kind: "bot" });
    const gid = await createGroup(bot, []);
    const statuses: number[] = [];
    for (let i = 0; i < 45; i++) {
      statuses.push((await api(`/v1/groups/${gid}/messages`, { token: bot.token, body: { clientMsgId: clientMsgId(), body: `n${i}` } })).status);
    }
    expect(statuses).toContain(429);
    expect(statuses.filter((s) => s === 200).length).toBeGreaterThanOrEqual(20);
  });
});
