// Usable, three roles (docs/plans/usable/design-roles.zh.md, unit R1): 所有者 (teamOwner),
// 管理员 (level "owner") and 普通成员 (level "member").
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { BASE, api, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const patch = (token: string, id: string, body: unknown) => api(`/v1/members/${id}`, { method: "PATCH", token, body });
const reason = (r: { body: any }) => r.body?.error?.reason;
const row = (id: string) => env.DB.prepare("SELECT level, team_owner, deactivated FROM members WHERE id = ?").bind(id).first<{ level: string; team_owner: number; deactivated: number }>();

describe("migration 0017_roles", () => {
  it("make every human admin a 所有者 and leave AI members and members alone", async () => {
    const human = await createMember({ level: "owner", teamOwner: false });
    const bot = await createMember({ level: "owner", kind: "bot" });
    const member = await createMember();
    const step = env.TEST_MIGRATIONS.find((m) => m.name.startsWith("0017_roles"));
    expect(step).toBeDefined();
    // Re-run the data step (the column already exists).
    for (const q of step!.queries.filter((q) => q.trim().toUpperCase().startsWith("UPDATE"))) await env.DB.prepare(q).run();
    expect((await row(human.id))?.team_owner).toBe(1);
    expect((await row(bot.id))?.team_owner).toBe(0);
    expect((await row(member.id))?.team_owner).toBe(0);
    const listed = (await api("/v1/members", { token: member.token })).body as any[];
    expect(listed.find((m) => m.id === human.id)).toMatchObject({ level: "owner", teamOwner: true });
    expect(listed.find((m) => m.id === bot.id).teamOwner).toBeUndefined();
    expect(listed.find((m) => m.id === member.id).teamOwner).toBeUndefined();
  });
});

describe("roles", () => {
  it("send teamOwner on /v1/me, the directory and the team roster", async () => {
    const owner = await createMember({ level: "owner" });
    const admin = await createMember({ level: "owner", teamOwner: false });
    expect((await api("/v1/me", { token: owner.token })).body).toMatchObject({ level: "owner", teamOwner: true });
    const me = (await api("/v1/me", { token: admin.token })).body;
    expect(me.level).toBe("owner");
    expect(me.teamOwner).toBeUndefined();
    const roster = (await api("/v1/team/roster", { token: admin.token })).body;
    expect(roster.members.find((m: any) => m.memberId === owner.id).teamOwner).toBe(true);
    expect(roster.members.find((m: any) => m.memberId === admin.id).teamOwner).toBeUndefined();
  });

  it("let only a 所有者 change roles", async () => {
    const owner = await createMember({ level: "owner" });
    const admin = await createMember({ level: "owner", teamOwner: false });
    const m = await createMember();
    const bot = await createMember({ kind: "bot" });

    // An admin changes nobody's role, not even its own.
    for (const [id, body] of [[m.id, { level: "owner" }], [m.id, { teamOwner: true }], [bot.id, { level: "owner" }], [admin.id, { level: "member" }], [admin.id, { teamOwner: true }]] as const) {
      const r = await patch(admin.token, id, body);
      expect(r.status, JSON.stringify([id, body])).toBe(403);
      expect(reason(r)).toBe("role.owner_only");
    }
    // A member sends none of it.
    expect((await patch(m.token, m.id, { teamOwner: true })).status).toBe(403);

    // The 所有者: member -> admin -> 所有者 -> admin -> member.
    expect((await patch(owner.token, m.id, { level: "owner" })).body).toMatchObject({ level: "owner" });
    expect((await row(m.id))?.team_owner).toBe(0);
    const up = await patch(owner.token, m.id, { teamOwner: true });
    expect(up.body).toMatchObject({ level: "owner", teamOwner: true });
    const down = await patch(owner.token, m.id, { teamOwner: false });
    expect(down.body.level).toBe("owner");
    expect(down.body.teamOwner).toBeUndefined();
    // Straight from member to 所有者; level "member" clears it.
    expect((await patch(owner.token, m.id, { level: "member" })).body.level).toBe("member");
    expect((await patch(owner.token, m.id, { teamOwner: true })).body).toMatchObject({ level: "owner", teamOwner: true });
    const cleared = await patch(owner.token, m.id, { level: "member" });
    expect(cleared.body.level).toBe("member");
    expect(cleared.body.teamOwner).toBeUndefined();
    expect(await row(m.id)).toMatchObject({ level: "member", team_owner: 0 });
    expect((await patch(owner.token, m.id, { level: "member", teamOwner: true })).status).toBe(400);

    // AI members: admin yes (by the 所有者), 所有者 never.
    const botOwner = await patch(owner.token, bot.id, { teamOwner: true });
    expect(botOwner.status).toBe(400);
    expect(botOwner.body.error.code).toBe("invalid");
    expect((await patch(owner.token, bot.id, { level: "owner" })).body).toMatchObject({ level: "owner" });
    expect((await row(bot.id))?.team_owner).toBe(0);
    // An unchanged level is not a role change: an admin may send it back.
    expect((await patch(admin.token, admin.id, { level: "owner", name: "Admin" })).status).toBe(200);
  });

  it("audit a 所有者 change as a level change on the activity page", async () => {
    const owner = await createMember({ level: "owner" });
    const admin = await createMember({ level: "owner", teamOwner: false });
    expect((await patch(owner.token, admin.id, { teamOwner: true })).status).toBe(200);
    const audit = await env.DB.prepare("SELECT id, details FROM audit WHERE action = 'member.update' AND target = ? ORDER BY id DESC LIMIT 1")
      .bind(admin.id)
      .first<{ id: number; details: string }>();
    expect(JSON.parse(audit!.details)).toEqual({ level: ["owner", "owner"], teamOwner: [false, true] });
    const activity = await env.DB.prepare("SELECT kind FROM activity WHERE audit_id = ?").bind(audit!.id).first<{ kind: string }>();
    expect(activity?.kind).toBe("level");
  });

  it("keep at least one active human 所有者", async () => {
    const owner = await createMember({ level: "owner" });
    const others = (await env.DB.prepare("SELECT id FROM members WHERE team_owner = 1 AND deactivated = 0 AND id != ?").bind(owner.id).all<{ id: string }>()).results;
    for (const o of others) expect((await patch(owner.token, o.id, { teamOwner: false })).status).toBe(200);
    // An AI admin does not count.
    await createMember({ level: "owner", kind: "bot" });
    for (const body of [{ teamOwner: false }, { level: "member" }, { deactivated: true }]) {
      const r = await patch(owner.token, owner.id, body);
      expect(r.status, JSON.stringify(body)).toBe(409);
      expect(reason(r)).toBe("role.last_owner");
    }
    expect(await row(owner.id)).toMatchObject({ level: "owner", team_owner: 1, deactivated: 0 });
    // A second 所有者 lets the first one step down: a hand-over.
    const next = await createMember({ level: "owner", teamOwner: false });
    expect((await patch(owner.token, next.id, { teamOwner: true })).status).toBe(200);
    expect((await patch(owner.token, owner.id, { teamOwner: false })).status).toBe(200);
    expect((await patch(owner.token, owner.id, { teamOwner: true })).status).toBe(403);
    // Restore for the other tests.
    expect((await patch(next.token, owner.id, { teamOwner: true })).status).toBe(200);
  });

  it("let only a 所有者 deactivate, revoke or edit an admin or a 所有者", async () => {
    const owner = await createMember({ level: "owner" });
    const admin = await createMember({ level: "owner", teamOwner: false });
    const other = await createMember({ level: "owner", teamOwner: false });
    const m = await createMember();
    const bot = await createMember({ kind: "bot" });
    const opsBot = await createMember({ kind: "bot", level: "owner" });

    for (const id of [owner.id, other.id, opsBot.id]) {
      for (const body of [{ deactivated: true }, { name: "x" }, { weeklyBudgetCents: 100 }]) {
        const r = await patch(admin.token, id, body);
        expect(r.status, JSON.stringify([id, body])).toBe(403);
        expect(reason(r)).toBe("role.owner_only");
      }
      const revoke = await api("/v1/tokens/revoke", { token: admin.token, body: { memberId: id } });
      expect(revoke.status).toBe(403);
      expect(reason(revoke)).toBe("role.owner_only");
    }
    expect((await row(other.id))?.deactivated).toBe(0);

    // Avatars too: changing or removing an admin's or the 所有者's is the 所有者's.
    const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 1, 2, 3]);
    const avatar = (token: string, id: string, method: "PUT" | "DELETE") =>
      SELF.fetch(`${BASE}/v1/members/${id}/avatar`, { method, headers: { authorization: `Bearer ${token}` }, body: method === "PUT" ? PNG : undefined });
    for (const id of [owner.id, other.id]) {
      for (const method of ["PUT", "DELETE"] as const) {
        const r = await avatar(admin.token, id, method);
        expect(r.status, `${method} ${id}`).toBe(403);
        expect(((await r.json()) as any).error.reason).toBe("role.owner_only");
      }
    }
    expect((await avatar(admin.token, admin.id, "PUT")).status).toBe(200);
    expect((await avatar(admin.token, m.id, "PUT")).status).toBe(200);
    expect((await avatar(admin.token, m.id, "DELETE")).status).toBe(200);
    expect((await avatar(owner.token, other.id, "PUT")).status).toBe(200);
    expect((await avatar(owner.token, other.id, "DELETE")).status).toBe(200);

    // Members and AI members: any admin.
    expect((await patch(admin.token, m.id, { name: "Renamed", weeklyBudgetCents: 100 })).status).toBe(200);
    expect((await api("/v1/tokens/revoke", { token: admin.token, body: { memberId: bot.id } })).status).toBe(200);
    expect((await patch(admin.token, bot.id, { deactivated: true })).status).toBe(200);
    expect((await patch(admin.token, bot.id, { deactivated: false })).status).toBe(200);
    // Its own record and tokens.
    expect((await patch(admin.token, admin.id, { name: "Me" })).status).toBe(200);

    // The 所有者 does all of it.
    expect((await patch(owner.token, other.id, { name: "Renamed admin" })).status).toBe(200);
    expect((await api("/v1/tokens/revoke", { token: owner.token, body: { memberId: other.id } })).status).toBe(200);
    expect((await patch(owner.token, other.id, { deactivated: true })).status).toBe(200);
    expect((await patch(owner.token, other.id, { deactivated: false })).status).toBe(200);
  });
});

describe("invites", () => {
  it("bring people in as members; a new admin invite is the 所有者's", async () => {
    const owner = await createMember({ level: "owner" });
    const admin = await createMember({ level: "owner", teamOwner: false });
    const m = await createMember();
    const invite = (token: string, body: unknown) => api("/v1/invites", { token, body });

    expect((await invite(admin.token, { name: "New", kind: "human", level: "member" })).status).toBe(200);
    for (const kind of ["human", "bot"]) {
      const r = await invite(admin.token, { name: "New", kind, level: "owner" });
      expect(r.status).toBe(403);
      expect(reason(r)).toBe("role.owner_only");
    }
    expect((await invite(m.token, { name: "New", kind: "human", level: "member" })).status).toBe(403);

    // A 所有者 may still make an admin invite (old clients); it redeems as an admin, not a 所有者.
    const code = (await invite(owner.token, { name: "Lead", kind: "human", level: "owner" })).body.code;
    const redeemed = await api("/v1/auth/redeem", { body: { inviteCode: code, deviceName: "Mac" } });
    expect(redeemed.status).toBe(200);
    expect(redeemed.body.member.level).toBe("owner");
    expect(redeemed.body.member.teamOwner).toBeUndefined();

    // A sign-in code signs in as its target, so one for another admin or the 所有者 is the
    // 所有者's; an admin still gets one for members and for itself. The role stays.
    const other = await createMember({ level: "owner", teamOwner: false });
    for (const target of [owner, other]) {
      const r = await invite(admin.token, { name: "x", kind: "human", level: "member", memberId: target.id });
      expect(r.status).toBe(403);
      expect(reason(r)).toBe("role.owner_only");
    }
    expect((await invite(admin.token, { name: "x", kind: "human", level: "member", memberId: m.id })).status).toBe(200);
    for (const [caller, target] of [[owner, admin], [owner, owner], [admin, admin]]) {
      const r = await invite(caller.token, { name: "x", kind: "human", level: "member", memberId: target.id });
      expect(r.status).toBe(200);
      const back = await api("/v1/auth/redeem", { body: { inviteCode: r.body.code, deviceName: "Mac 2" } });
      expect(back.status).toBe(200);
      expect(back.body.member.id).toBe(target.id);
    }
    expect(await row(owner.id)).toMatchObject({ level: "owner", team_owner: 1 });
    expect(await row(admin.id)).toMatchObject({ level: "owner", team_owner: 0 });
  });
});

describe("team settings", () => {
  it("let admins (所有者 included) change the AI service, team models and approval rules; members read", async () => {
    const admin = await createMember({ level: "owner", teamOwner: false });
    const m = await createMember();
    const ai = { provider: "deepseek", model: "deepseek-chat", apiKey: "sk-roles-test-0000" };

    expect((await api("/v1/settings/ai", { token: m.token })).status).toBe(403);
    expect((await api("/v1/settings/ai", { method: "PUT", token: m.token, body: ai })).status).toBe(403);
    expect((await api("/v1/settings/ai", { method: "DELETE", token: m.token })).status).toBe(403);
    expect((await api("/v1/settings/ai/test", { method: "POST", token: m.token, body: {} })).status).toBe(403);
    expect((await api("/v1/settings/ai", { method: "PUT", token: admin.token, body: ai })).status).toBe(200);
    expect((await api("/v1/settings/ai", { token: admin.token })).body.keySet).toBe(true);
    expect((await api("/v1/settings/ai", { method: "DELETE", token: admin.token })).status).toBe(200);

    expect((await api("/v1/settings/models", { token: m.token })).status).toBe(200);
    expect((await api("/v1/settings/models/rolestest", { method: "PUT", token: m.token, body: { model: "x" } })).status).toBe(403);
    expect((await api("/v1/settings/models/rolestest", { method: "PUT", token: admin.token, body: { model: "x" } })).status).toBe(200);
    await env.DB.prepare("DELETE FROM worker_models WHERE runtime = 'rolestest'").run();

    expect((await api("/v1/settings/approvals", { token: m.token })).status).toBe(200);
    expect((await api("/v1/settings/approvals", { method: "PATCH", token: m.token, body: {} })).status).toBe(403);
    expect((await api("/v1/settings/approvals", { method: "PATCH", token: admin.token, body: {} })).status).toBe(200);
  });
});
