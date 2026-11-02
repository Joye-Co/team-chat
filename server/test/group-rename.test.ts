import { afterAll, describe, expect, it } from "vitest";
import { env } from "cloudflare:test";
import { Client, api, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const rename = (id: string, token: string | undefined, body: unknown) =>
  api(`/v1/groups/${id}`, { method: "PATCH", token, body });

const renameEvents = (c: Client, gid: string) => c.events(gid).filter((e) => e.type === "group.updated");

describe("PATCH /v1/groups/:id", () => {
  it("lets a member rename a group, normalizes the name, updates D1 and every client", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id], "研发");
    const { c: cb } = await Client.connect(b.token);

    const r = await rename(gid, a.token, { name: "  前端　\t 小组  " });
    expect(r.status).toBe(200);
    expect(r.body.group).toMatchObject({ id: gid, kind: "group", name: "前端 小组" });
    expect(r.body.lastSeq).toBeGreaterThan(0);

    const ev = await cb.waitEvent((e) => e.groupId === gid && e.type === "group.updated");
    expect(ev).toMatchObject({ actorId: a.id, data: { name: "前端 小组" } });
    const row = await env.DB.prepare("SELECT name FROM groups WHERE id = ?").bind(gid).first<{ name: string }>();
    expect(row?.name).toBe("前端 小组");
    const list = await api("/v1/groups", { token: b.token });
    expect(list.body.find((s: any) => s.group.id === gid).group.name).toBe("前端 小组");
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'group.rename' AND target = ?")
      .bind(gid)
      .first<{ details: string }>();
    expect(JSON.parse(audit!.details)).toEqual({ from: "研发", to: "前端 小组" });

    // A catching-up client gets the event from the log too.
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.filter((e: any) => e.type === "group.updated").map((e: any) => e.data.name)).toEqual(["前端 小组"]);
    cb.close();
  });

  it("adds no event for the same name, also after normalization", async () => {
    const a = await createMember();
    const gid = await createGroup(a, [], "设计");
    const { c } = await Client.connect(a.token);
    const before = (await rename(gid, a.token, { name: "设计" })).body.lastSeq;
    expect((await rename(gid, a.token, { name: " 设计 " })).status).toBe(200);
    await rename(gid, a.token, { name: "设计 评审" });
    const after = (await rename(gid, a.token, { name: "设计  评审" })).body;
    expect(after.group.name).toBe("设计 评审");
    expect(after.lastSeq).toBe(before + 1);
    await waitUntil(() => renameEvents(c, gid).length === 1, 2000, "one rename event");
    await new Promise((r) => setTimeout(r, 200));
    expect(renameEvents(c, gid).map((e) => e.data.name)).toEqual(["设计 评审"]);
    c.close();
  });

  it("checks the name like group create", async () => {
    const a = await createMember();
    const gid = await createGroup(a, [], "x");
    for (const name of ["", "   ", "　\n", "群".repeat(65)]) {
      const r = await rename(gid, a.token, { name });
      expect(r.status, JSON.stringify(name)).toBe(400);
      expect(r.body.error.code).toBe("invalid");
    }
    expect((await rename(gid, a.token, { name: "群".repeat(64) })).status).toBe(200);
    expect((await rename(gid, a.token, {})).status).toBe(400);
    // Create trims and collapses the same way.
    const created = await api("/v1/groups", { token: a.token, body: { name: "  运维   值班 ", memberIds: [] } });
    expect(created.body.group.name).toBe("运维 值班");
    expect((await api("/v1/groups", { token: a.token, body: { name: "  ", memberIds: [] } })).status).toBe(400);
  });

  it("refuses non-members, unknown groups, DMs and missing tokens; lets an owner outside the group rename", async () => {
    const a = await createMember();
    const b = await createMember();
    const outsider = await createMember();
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(a, [b.id], "g");
    const { c: cb } = await Client.connect(b.token);

    const refused = await rename(gid, outsider.token, { name: "hijack" });
    expect(refused.status).toBe(403);
    expect(refused.body.error.code).toBe("forbidden");
    expect((await rename("01NOPE0000000000000000000", a.token, { name: "x" })).status).toBe(404);
    expect((await rename(gid, undefined, { name: "x" })).status).toBe(401);

    const dm = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    const dmId = dm.body.group.id;
    const dmRefused = await rename(dmId, a.token, { name: "我们俩" });
    expect(dmRefused.status).toBe(403);
    expect(dmRefused.body.error).toEqual({ code: "forbidden", message: "a DM cannot be renamed" });
    expect((await rename(dmId, owner.token, { name: "我们俩" })).status).toBe(403);
    const dmRow = await env.DB.prepare("SELECT name FROM groups WHERE id = ?").bind(dmId).first<{ name: string }>();
    expect(dmRow?.name).toBe("");

    const byOwner = await rename(gid, owner.token, { name: "全员" });
    expect(byOwner.status).toBe(200);
    expect(byOwner.body).toMatchObject({ group: { id: gid, name: "全员" }, readSeq: 0, unread: 0, mentions: 0 });
    expect(byOwner.body.group.memberIds).not.toContain(owner.id);
    const ev = await cb.waitEvent((e) => e.groupId === gid && e.type === "group.updated");
    expect(ev).toMatchObject({ actorId: owner.id, data: { name: "全员" } });
    expect(renameEvents(cb, gid).map((e) => e.data.name)).toEqual(["全员"]);
    cb.close();
  });

  it("finishes a rename whose D1 write did not happen without a second event", async () => {
    const a = await createMember();
    const gid = await createGroup(a, [], "旧名");
    const { c } = await Client.connect(a.token);
    expect((await rename(gid, a.token, { name: "新名" })).status).toBe(200);
    // Simulate a lost D1 write after the room appended the event.
    await env.DB.prepare("UPDATE groups SET name = '旧名' WHERE id = ?").bind(gid).run();
    const retry = await rename(gid, a.token, { name: "新名" });
    expect(retry.body.group.name).toBe("新名");
    const row = await env.DB.prepare("SELECT name FROM groups WHERE id = ?").bind(gid).first<{ name: string }>();
    expect(row?.name).toBe("新名");
    await waitUntil(() => renameEvents(c, gid).length === 1, 2000, "rename event");
    await new Promise((r) => setTimeout(r, 200));
    expect(renameEvents(c, gid)).toHaveLength(1);
    c.close();
  });
});
