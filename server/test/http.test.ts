import { SELF, env } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { Client, api, createGroup, createMember } from "./helpers";

describe("http auth", () => {
  it("rejects requests without a token with 401 and the error shape", async () => {
    const r = await api("/v1/me");
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: { code: "unauthorized", message: expect.any(String) } });
    const bad = await api("/v1/me", { token: "tc_nope" });
    expect(bad.status).toBe(401);
  });

  it("rejects the websocket upgrade with 401 before upgrading", async () => {
    const res = await SELF.fetch("http://chat.test/v1/ws", { headers: { upgrade: "websocket" } });
    expect(res.status).toBe(401);
    expect(res.webSocket).toBeNull();
    const res2 = await SELF.fetch("http://chat.test/v1/ws", {
      headers: { upgrade: "websocket", authorization: "Bearer tc_invalid" },
    });
    expect(res2.status).toBe(401);
  });

  it("serves health without auth", async () => {
    const r = await api("/v1/health");
    expect(r).toEqual({ status: 200, body: { ok: true, version: expect.any(String) } });
  });

  it("returns me and members", async () => {
    const m = await createMember({ name: "Alice" });
    const me = await api("/v1/me", { token: m.token });
    expect(me.body).toMatchObject({ id: m.id, name: "Alice", kind: "human", level: "member" });
    const all = await api("/v1/members", { token: m.token });
    expect(all.body.some((x: any) => x.id === m.id)).toBe(true);
  });

  it("rejects revoked tokens and deactivated members, on HTTP and WebSocket", async () => {
    const m = await createMember();
    expect((await api("/v1/me", { token: m.token })).status).toBe(200);
    const { c } = await Client.connect(m.token);
    const r = await api("/v1/tokens/revoke", { token: m.token, body: {} });
    expect(r).toEqual({ status: 200, body: { revoked: 1 } });
    expect((await api("/v1/me", { token: m.token })).status).toBe(401);
    await c.next((f) => f.t === "error" && f.error.code === "unauthorized");
    const ws = await SELF.fetch("http://chat.test/v1/ws", {
      headers: { upgrade: "websocket", authorization: `Bearer ${m.token}` },
    });
    expect(ws.status).toBe(401);

    const d = await createMember();
    await env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(d.id).run();
    expect((await api("/v1/me", { token: d.token })).status).toBe(401);
    const ws2 = await SELF.fetch("http://chat.test/v1/ws", {
      headers: { upgrade: "websocket", authorization: `Bearer ${d.token}` },
    });
    expect(ws2.status).toBe(401);
  });

  it("lets an owner revoke all tokens of another member, and nobody else", async () => {
    const owner = await createMember({ level: "owner" });
    const a = await createMember();
    const b = await createMember();
    expect((await api("/v1/tokens/revoke", { token: b.token, body: { memberId: a.id, all: true } })).status).toBe(403);
    const r = await api("/v1/tokens/revoke", { token: owner.token, body: { memberId: a.id, all: true } });
    expect(r.body).toEqual({ revoked: 1 });
    expect((await api("/v1/me", { token: a.token })).status).toBe(401);
    const audit = await env.DB.prepare("SELECT * FROM audit WHERE action = 'token.revoke' AND target = ?")
      .bind(a.id)
      .first<any>();
    expect(audit).toMatchObject({ actor_id: owner.id, credential_id: owner.tokenId });
  });
});

describe("invites", () => {
  it("are owner only, single use and expire", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    const denied = await api("/v1/invites", { token: member.token, body: { name: "X", kind: "human", level: "member" } });
    expect(denied.status).toBe(403);
    expect(denied.body.error.code).toBe("forbidden");

    const inv = await api("/v1/invites", {
      token: owner.token,
      body: { name: "Intern", kind: "human", level: "member", role: "dev" },
    });
    expect(inv.status).toBe(200);
    expect(inv.body.code).toMatch(/^[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}$/);
    expect(inv.body.expiresAt).toBeGreaterThan(Date.now() + 6 * 24 * 3600 * 1000);
    const stored = await env.DB.prepare("SELECT code_hash FROM invites WHERE created_by = ?").bind(owner.id).first<any>();
    expect(stored.code_hash).toMatch(/^[0-9a-f]{64}$/);

    const red = await api("/v1/auth/redeem", {
      body: { inviteCode: inv.body.code.toLowerCase(), deviceName: "mac" },
    });
    expect(red.status).toBe(200);
    expect(red.body.token).toMatch(/^tc_[A-Za-z0-9_-]{43}$/);
    expect(red.body.member).toMatchObject({ name: "Intern", kind: "human", level: "member", role: "dev" });
    const me = await api("/v1/me", { token: red.body.token });
    expect(me.body.id).toBe(red.body.member.id);

    const again = await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "mac" } });
    expect(again.status).toBe(401);

    const inv2 = await api("/v1/invites", { token: owner.token, body: { name: "Late", kind: "human", level: "member" } });
    await env.DB.prepare("UPDATE invites SET expires_at = ? WHERE name = 'Late'").bind(Date.now() - 1).run();
    const expired = await api("/v1/auth/redeem", { body: { inviteCode: inv2.body.code, deviceName: "mac" } });
    expect(expired.status).toBe(401);
  });

  it("creates bots with a token (owner only)", async () => {
    const owner = await createMember({ level: "owner" });
    const r = await api("/v1/bots", { token: owner.token, body: { name: "Builder", role: "CI" } });
    expect(r.status).toBe(200);
    expect(r.body.member).toMatchObject({ kind: "bot", name: "Builder", level: "member" });
    expect((await api("/v1/me", { token: r.body.token })).body.kind).toBe("bot");
    const member = await createMember();
    expect((await api("/v1/bots", { token: member.token, body: { name: "B" } })).status).toBe(403);
  });
});

describe("validation and limits", () => {
  it("rejects invalid bodies and bodies over 32 KiB", async () => {
    const owner = await createMember({ level: "owner" });
    const bad = await api("/v1/groups", { token: owner.token, body: { name: "" } });
    expect(bad.status).toBe(400);
    expect(bad.body.error.code).toBe("invalid");
    const big = await api("/v1/groups", {
      token: owner.token,
      body: { name: "x", memberIds: ["a".repeat(40 * 1024)] },
    });
    expect(big.status).toBe(413);
    expect(big.body.error.code).toBe("too_large");
    const notJson = await api("/v1/groups", { token: owner.token, raw: "{", method: "POST" });
    expect(notJson.status).toBe(400);
  });

  it("returns not_found for unknown routes", async () => {
    const r = await api("/v1/nope");
    expect(r.status).toBe(404);
    expect(r.body.error.code).toBe("not_found");
  });
});

describe("groups", () => {
  it("creates groups, adds and removes members, and finds or creates DMs", async () => {
    const a = await createMember({ name: "A" });
    const b = await createMember({ name: "B" });
    const c = await createMember({ name: "C" });
    const outsider = await createMember();
    const gid = await createGroup(a, [b.id], "Team");
    const list = await api("/v1/groups", { token: b.token });
    expect(list.body.find((s: any) => s.group.id === gid)).toMatchObject({
      group: { name: "Team", kind: "group", memberIds: expect.arrayContaining([a.id, b.id]) },
      lastSeq: 0,
      readSeq: 0,
      unread: 0,
      mentions: 0,
    });

    expect((await api(`/v1/groups/${gid}/members`, { token: outsider.token, body: { memberId: outsider.id } })).status).toBe(403);
    const added = await api(`/v1/groups/${gid}/members`, { token: b.token, body: { memberId: c.id } });
    expect(added.status).toBe(200);
    expect(added.body.group.memberIds).toContain(c.id);
    expect(added.body.lastSeq).toBe(1);

    // A member may not remove another (not owner); self removal works.
    expect((await api(`/v1/groups/${gid}/members/${c.id}`, { method: "DELETE", token: b.token })).status).toBe(403);
    const left = await api(`/v1/groups/${gid}/members/${c.id}`, { method: "DELETE", token: c.token });
    expect(left).toEqual({ status: 200, body: { ok: true } });
    expect((await api("/v1/groups", { token: c.token })).body.some((s: any) => s.group.id === gid)).toBe(false);

    const dm1 = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    expect(dm1.body.group).toMatchObject({ kind: "dm", name: "B" });
    const dm2 = await api("/v1/dms", { token: b.token, body: { memberId: a.id } });
    expect(dm2.body.group).toMatchObject({ id: dm1.body.group.id, name: "A" });
    expect((await api("/v1/dms", { token: a.token, body: { memberId: a.id } })).status).toBe(400);

    const audits = await env.DB.prepare("SELECT action FROM audit WHERE target = ? ORDER BY id").bind(gid).all<any>();
    expect(audits.results.map((r) => r.action)).toEqual(["group.create", "group.member.add", "group.member.remove"]);
  });
});
