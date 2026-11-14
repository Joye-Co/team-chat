// A member's own AI member and the VM size of a machine (docs/plans/usable/design-personal-ai.zh.md;
// docs/protocol.md "A member's own AI member and the VM size of a machine").
import { env } from "cloudflare:test";
import { Machine, Member } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { deactivateOwnBots } from "../src/bots/deactivate";
import { mayTake } from "../src/bots/dispatch";
import { Daemon } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createGroup, createMember } from "./helpers";

async function team() {
  const admin = await createMember({ level: "owner" });
  const alice = await createMember();
  const bob = await createMember();
  return { admin, alice, bob };
}

async function machineOf(m: TestMember, body: Record<string, unknown> = {}) {
  const r = await api("/v1/machines", { token: m.token, body: { name: "vm-host", ...body } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return r.body as { machine: Machine; token: string };
}

async function newBot(m: TestMember, body: Record<string, unknown> = {}) {
  return api("/v1/bots", { token: m.token, body: { name: "小助手", ...body } });
}

describe("a member's own AI member", () => {
  it("lets a person make one of his own; an admin makes any, also for someone", async () => {
    const { admin, alice, bob } = await team();
    const mine = await newBot(alice, { character: "o03" });
    expect(mine.status, JSON.stringify(mine.body)).toBe(200);
    expect(Member.parse(mine.body.member)).toMatchObject({ kind: "bot", level: "member", ownerId: alice.id, character: "o03" });
    expect(mine.body.token).toEqual(expect.any(String));

    const second = await newBot(alice, { name: "第二个" });
    expect(second.status).toBe(403);
    expect(second.body.error?.reason).toBe("bot.own_limit");

    // Only an admin names someone else.
    expect((await newBot(bob, { ownerId: alice.id })).status).toBe(403);
    // An AI member has none of its own.
    const bot = await createMember({ kind: "bot" });
    expect((await newBot(bot)).status).toBe(403);

    // An admin: team AI members without limit, and one for Bob.
    expect((await newBot(admin)).body.member.ownerId).toBeUndefined();
    expect((await newBot(admin)).status).toBe(200);
    const forBob = await newBot(admin, { ownerId: bob.id });
    expect(forBob.body.member.ownerId).toBe(bob.id);
    expect((await newBot(admin, { ownerId: bot.id })).status).toBe(400);
    // Bob already has one now.
    expect((await newBot(bob)).body.error?.reason).toBe("bot.own_limit");

    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'bot.create' AND target = ?")
      .bind(forBob.body.member.id)
      .first<{ details: string }>();
    expect(JSON.parse(audit!.details)).toMatchObject({ ownerId: bob.id });
  });

  it("lets its person edit name, 岗位, avatar and use, nothing else; others not at all", async () => {
    const { alice, bob } = await team();
    const id = (await newBot(alice)).body.member.id as string;
    const patch = (m: TestMember, body: Record<string, unknown>) => api(`/v1/members/${id}`, { method: "PATCH", token: m.token, body });

    const r = await patch(alice, { name: "新名字", role: "前端", character: "o05" });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body).toMatchObject({ name: "新名字", role: "前端", character: "o05", ownerId: alice.id });
    expect((await patch(alice, { level: "owner" })).status).toBe(403);
    expect((await patch(alice, { weeklyBudgetCents: 100 })).status).toBe(403);
    expect((await patch(bob, { name: "x" })).status).toBe(403);

    // Stop using it, make another, and then the first may not come back.
    expect((await patch(alice, { deactivated: true })).body.deactivated).toBe(true);
    expect((await newBot(alice, { name: "另一个" })).status).toBe(200);
    const back = await patch(alice, { deactivated: false });
    expect(back.status).toBe(403);
    expect(back.body.error?.reason).toBe("bot.own_limit");
  });

  it("runs only on its person's computers", async () => {
    const { admin, alice, bob } = await team();
    const id = (await newBot(alice)).body.member.id as string;
    const aliceMac = (await machineOf(alice)).machine.id;
    const bobMac = (await machineOf(bob)).machine.id;
    const teamBox = (await machineOf(admin)).machine.id;
    const put = (m: TestMember, body: Record<string, unknown>) =>
      api(`/v1/bots/${id}/runner`, { method: "PUT", token: m.token, body: { runtime: "claude", ...body } });

    const none = await put(alice, {});
    expect(none.status).toBe(400);
    expect(none.body.error?.reason).toBe("bot.own_needs_machine");
    expect((await put(alice, { machineId: teamBox })).body.error?.reason).toBe("machine.not_yours");
    expect((await put(alice, { machineId: bobMac })).body.error?.reason).toBe("machine.not_yours");
    const ok = await put(alice, { machineId: aliceMac, model: "sonnet" });
    expect(ok.status, JSON.stringify(ok.body)).toBe(200);
    expect(ok.body).toMatchObject({ machineId: aliceMac, model: "sonnet" });
    // Absent machineId keeps the stored one.
    expect((await put(alice, { model: "opus" })).body.machineId).toBe(aliceMac);
    expect((await api(`/v1/bots/${id}/runner`, { token: alice.token })).status).toBe(200);
    // Bob may not read or change it; an admin may, but not move it to the team.
    expect((await api(`/v1/bots/${id}/runner`, { token: bob.token })).status).toBe(403);
    expect((await put(bob, { machineId: bobMac })).status).toBe(403);
    expect((await put(admin, { machineId: teamBox })).body.error?.reason).toBe("machine.not_yours");
    expect((await put(admin, { machineId: aliceMac, model: "haiku" })).status).toBe(200);
    // A team AI member stays the admins'.
    const teamBot = (await newBot(admin)).body.member.id as string;
    expect((await api(`/v1/bots/${teamBot}/runner`, { method: "PUT", token: alice.token, body: { runtime: "claude", machineId: aliceMac } })).status).toBe(403);
  });

  it("takes work for anyone on its person's computer and nowhere else", () => {
    const runner = { runtime: "claude", labels: [], machineId: "M1" };
    const ctx = { botId: "B", requestedFor: "BOB", botOwnerId: "ALICE" };
    expect(mayTake({ id: "M1", labels: [], ownerId: "ALICE" }, runner, ctx)).toBe(true);
    expect(mayTake({ id: "M1", labels: [], ownerId: "ALICE" }, runner, { ...ctx, requestedFor: null })).toBe(true);
    expect(mayTake({ id: "M1", labels: [] }, runner, ctx)).toBe(false);
    expect(mayTake({ id: "M2", labels: [], ownerId: "ALICE" }, runner, ctx)).toBe(false);
    // Without an owner, the rules of personal machines are as before: not for Bob.
    expect(mayTake({ id: "M1", labels: [], ownerId: "ALICE" }, runner, { botId: "B", requestedFor: "BOB" })).toBe(false);
  });

  it("is deactivated with its person", async () => {
    const { admin, alice } = await team();
    const id = (await newBot(alice)).body.member.id as string;
    const r = await api(`/v1/members/${alice.id}`, { method: "PATCH", token: admin.token, body: { deactivated: true } });
    expect(r.status).toBe(200);
    const row = await env.DB.prepare("SELECT deactivated FROM members WHERE id = ?").bind(id).first<{ deactivated: number }>();
    expect(row?.deactivated).toBe(1);
    const tokens = await env.DB.prepare("SELECT COUNT(*) AS n FROM tokens WHERE member_id = ? AND revoked_at IS NULL").bind(id).first<{ n: number }>();
    expect(tokens?.n).toBe(0);
    expect(await deactivateOwnBots(env as never, alice.id)).toEqual([]);
  });
});

describe("computers set up for someone and their VM size", () => {
  it("lets an admin add a computer for a person and see it in full", async () => {
    const { admin, alice, bob } = await team();
    expect((await api("/v1/machines", { token: bob.token, body: { name: "x", ownerId: alice.id } })).status).toBe(403);
    expect((await api("/v1/machines", { token: admin.token, body: { name: "x", ownerId: (await createMember({ kind: "bot" })).id } })).status).toBe(400);
    const { machine } = await machineOf(admin, { ownerId: alice.id, labels: ["vm"] });
    expect(machine).toMatchObject({ ownerId: alice.id, createdBy: admin.id });

    const byAdmin = (await api("/v1/machines", { token: admin.token })).body as Machine[];
    expect(byAdmin.find((m) => m.id === machine.id)).toMatchObject({ labels: ["vm"], ownerId: alice.id });
    const byAlice = (await api("/v1/machines", { token: alice.token })).body as Machine[];
    expect(byAlice.find((m) => m.id === machine.id)).toMatchObject({ ownerId: alice.id, personalBots: [] });
    // Bob sees it cut down.
    const byBob = (await api("/v1/machines", { token: bob.token })).body as Machine[];
    expect(byBob.find((m) => m.id === machine.id)?.labels).toEqual([]);
  });

  it("lets only an admin set the VM size, which each run carries", async () => {
    const { admin, alice } = await team();
    const { machine, token } = await machineOf(admin, { ownerId: alice.id, labels: ["vm"] });
    const patch = (m: TestMember, body: Record<string, unknown>) => api(`/v1/machines/${machine.id}`, { method: "PATCH", token: m.token, body });
    expect((await patch(alice, { vm: { vcpus: 8, memMib: 16384 } })).status).toBe(403);
    expect((await patch(admin, { vm: { vcpus: 64, memMib: 4096 } })).status).toBe(400);
    const set = await patch(admin, { vm: { vcpus: 2, memMib: 4096 } });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    expect(set.body.vm).toEqual({ vcpus: 2, memMib: 4096 });
    // The admin may not rename someone's computer.
    expect((await patch(admin, { name: "y" })).status).toBe(403);

    // Alice's own AI member on it gets the size in run.assign, also for Bob's work.
    const bot = (await newBot(alice)).body.member;
    expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: alice.token, body: { runtime: "codex", machineId: machine.id } })).status).toBe(200);
    const bob = await createMember();
    const gid = await createGroup(admin, [alice.id, bob.id, bot.id]);
    const { d } = await Daemon.connect(token);
    const { c } = await Client.connect(bob.token);
    await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@小助手 看一下", mentions: [bot.id] });
    const assign = await d.next((f) => f.t === "run.assign" && f.botMemberId === bot.id, 10000);
    expect(assign.vm).toEqual({ vcpus: 2, memMib: 4096 });
    c.close();
    d.close();

    // null goes back to the daemon's own size.
    expect((await patch(admin, { vm: null })).body.vm).toBeUndefined();
  });
});
