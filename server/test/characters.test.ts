import { afterAll, describe, expect, it } from "vitest";
import { env } from "cloudflare:test";
import { CHARACTER_IDS } from "@chat/protocol";
import { Client, api, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

async function lastAudit(action: string, target: string) {
  const row = await env.DB.prepare("SELECT actor_id, details FROM audit WHERE action = ? AND target = ? ORDER BY id DESC LIMIT 1")
    .bind(action, target)
    .first<{ actor_id: string; details: string }>();
  return row ? { actorId: row.actor_id, details: JSON.parse(row.details) } : null;
}

describe("member characters", () => {
  it("knows the 26 character ids", () => {
    expect(CHARACTER_IDS).toHaveLength(26);
    expect(new Set(CHARACTER_IDS).size).toBe(26);
  });

  it("let an owner create a bot with a character, audited and listed", async () => {
    const owner = await createMember({ level: "owner" });
    const r = await api("/v1/bots", { method: "POST", token: owner.token, body: { name: "Tata", role: "产品负责人", character: "o03" } });
    expect(r.status).toBe(200);
    expect(r.body.member).toMatchObject({ kind: "bot", name: "Tata", role: "产品负责人", character: "o03" });
    const id = r.body.member.id as string;
    expect(await lastAudit("bot.create", id)).toMatchObject({ actorId: owner.id, details: { name: "Tata", character: "o03" } });

    const list = await api("/v1/members", { token: owner.token });
    expect(list.body.find((m: any) => m.id === id)).toMatchObject({ character: "o03" });
    const me = await api("/v1/me", { token: r.body.token });
    expect(me.body).toMatchObject({ id, character: "o03" });

    // Without a character the field stays absent.
    const plain = await api("/v1/bots", { method: "POST", token: owner.token, body: { name: "Plain" } });
    expect(plain.status).toBe(200);
    expect(plain.body.member).not.toHaveProperty("character");
  });

  it("reject unknown ids and non-owners", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    for (const character of ["o21", "JOJO", "", null, 3]) {
      const r = await api("/v1/bots", { method: "POST", token: owner.token, body: { name: "Bad", character } });
      expect(r.status).toBe(400);
    }
    expect((await api("/v1/bots", { method: "POST", token: m.token, body: { name: "X", character: "jojo" } })).status).toBe(403);
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { character: "o99" } })).status).toBe(400);
    // A member may change its own name and role, not its character.
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body: { character: "jojo" } })).status).toBe(403);
    expect((await api(`/v1/members/${m.id}`, { method: "PATCH", token: m.token, body: { character: null } })).status).toBe(403);
  });

  it("let an owner set and clear a character and role, push directory and welcome", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot" });
    const viewer = await createMember();
    await createGroup(owner, [bot.id, viewer.id]);
    const { c } = await Client.connect(viewer.token);

    const set = await api(`/v1/members/${bot.id}`, {
      method: "PATCH",
      token: owner.token,
      body: { character: "o05", role: "幕僚长" },
    });
    expect(set.status).toBe(200);
    expect(set.body).toMatchObject({ id: bot.id, character: "o05", role: "幕僚长" });
    await c.next((f) => f.t === "directory" && f.members?.some((x: any) => x.id === bot.id && x.character === "o05"));
    expect(await lastAudit("member.update", bot.id)).toMatchObject({
      actorId: owner.id,
      details: { character: [null, "o05"], role: [null, "幕僚长"] },
    });

    const { c: c2, welcome } = await Client.connect(viewer.token);
    expect(welcome.members.find((m: any) => m.id === bot.id)).toMatchObject({ character: "o05" });

    // The same value again changes nothing.
    const same = await api(`/v1/members/${bot.id}`, { method: "PATCH", token: owner.token, body: { character: "o05" } });
    expect(same.body).toMatchObject({ character: "o05" });

    const cleared = await api(`/v1/members/${bot.id}`, { method: "PATCH", token: owner.token, body: { character: null } });
    expect(cleared.status).toBe(200);
    expect(cleared.body).not.toHaveProperty("character");
    await c2.next((f) => f.t === "directory" && f.members?.some((x: any) => x.id === bot.id && x.character === undefined));
    expect(await lastAudit("member.update", bot.id)).toMatchObject({ details: { character: ["o05", null] } });
    c.close();
    c2.close();
  });

  it("drop an unknown stored id instead of sending it", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    await env.DB.prepare("UPDATE members SET character = 'zz9' WHERE id = ?").bind(m.id).run();
    const list = await api("/v1/members", { token: owner.token });
    expect(list.body.find((x: any) => x.id === m.id)).not.toHaveProperty("character");
  });
});
