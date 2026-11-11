// Personal and team machines (docs/plans/usable/design-roles.zh.md 3.3-3.5; docs/protocol.md
// "Roles: 所有者 and personal machines"): who adds, sees, renames, moves and removes a machine,
// the checked AI members of a personal machine, and a deactivated person's machines.
import { SELF, env } from "cloudflare:test";
import { Machine } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { revokePersonalMachines } from "../src/bots/deactivate";
import { BASE, type TestMember, api, createMember, waitUntil } from "./helpers";
import { Daemon } from "./daemon";

async function addMachine(m: TestMember, body: Record<string, unknown>) {
  return api("/v1/machines", { token: m.token, body });
}

async function list(m: TestMember): Promise<Machine[]> {
  const r = await api("/v1/machines", { token: m.token });
  expect(r.status).toBe(200);
  return r.body.map((x: unknown) => Machine.parse(x));
}

async function runnerBot(admin: TestMember, body: Record<string, unknown> = {}) {
  const bot = await createMember({ kind: "bot" });
  const r = await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: admin.token, body: { runtime: "codex", ...body } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return bot;
}

async function team() {
  const admin = await createMember({ level: "owner" });
  const admin2 = await createMember({ level: "owner" });
  const alice = await createMember();
  const bob = await createMember();
  return { admin, admin2, alice, bob };
}

describe("personal and team machines", () => {
  it("lets every person add a personal machine; only admins add team machines", async () => {
    const { admin, alice } = await team();
    // An admin without `personal` adds a team machine, as before.
    const t = await addMachine(admin, { name: "linux", labels: ["x"] });
    expect(t.status).toBe(200);
    expect(t.body.machine.ownerId).toBeUndefined();
    expect(t.body.machine.personalBots).toBeUndefined();
    // An admin may add his own.
    const own = await addMachine(admin, { name: "admin-mac", personal: true });
    expect(own.body.machine).toMatchObject({ ownerId: admin.id, personalBots: [] });
    // A member always adds his own; asking for a team machine is forbidden.
    const mine = await addMachine(alice, { name: "alice-mac" });
    expect(mine.status, JSON.stringify(mine.body)).toBe(200);
    expect(mine.body.token).toMatch(/^tm_[A-Za-z0-9_-]{43}$/);
    expect(mine.body.machine).toMatchObject({ ownerId: alice.id, createdBy: alice.id, personalBots: [] });
    expect((await addMachine(alice, { name: "alice-mac-2", personal: true })).body.machine.ownerId).toBe(alice.id);
    const denied = await addMachine(alice, { name: "team", personal: false });
    expect(denied.status).toBe(403);
    expect(denied.body.error?.reason).toBe("machine.team_admin_only");
    // An AI member has no personal machine.
    const bot = await createMember({ kind: "bot" });
    expect((await addMachine(bot, { name: "b" })).status).toBe(403);
    // Its daemon connects with the token as any machine's does.
    const { d, welcome } = await Daemon.connect(mine.body.token);
    expect(welcome).toMatchObject({ machineId: mine.body.machine.id });
    d.close();
    // Activity lists the new machine with `personal`.
    const row = await env.DB.prepare("SELECT details FROM audit WHERE action = 'machine.create' AND target = ?")
      .bind(mine.body.machine.id)
      .first<{ details: string }>();
    expect(JSON.parse(row!.details)).toMatchObject({ personal: true });
  });

  it("shows each person what he may see", async () => {
    const { admin, alice, bob } = await team();
    const teamId = (await addMachine(admin, { name: "linux", labels: ["x"] })).body.machine.id as string;
    const aliceId = (await addMachine(alice, { name: "alice-mac", labels: ["a"] })).body.machine.id as string;
    const bot = await runnerBot(admin);
    expect((await api(`/v1/machines/${aliceId}/bots/${bot.id}`, { method: "PUT", token: alice.token, body: { repoPath: "/r" } })).status).toBe(200);

    const byAdmin = await list(admin);
    expect(byAdmin.find((m) => m.id === teamId)).toMatchObject({ labels: ["x"] });
    // Someone else's personal machine: id, name, owner and state only, also for an admin.
    expect(byAdmin.find((m) => m.id === aliceId)).toEqual({
      id: aliceId,
      name: "alice-mac",
      labels: [],
      createdBy: alice.id,
      createdAt: expect.any(Number),
      ownerId: alice.id,
      connected: false,
      healthy: false,
      runningRuns: [],
    });

    const byAlice = await list(alice);
    // A team machine for a member: cut down, as before.
    expect(byAlice.find((m) => m.id === teamId)).toEqual({
      id: teamId,
      name: "linux",
      labels: [],
      createdBy: admin.id,
      createdAt: expect.any(Number),
      connected: false,
      healthy: false,
      runningRuns: [],
    });
    // My own machine in full, with my checked AI members.
    expect(byAlice.find((m) => m.id === aliceId)).toMatchObject({
      labels: ["a"],
      ownerId: alice.id,
      personalBots: [{ botId: bot.id, repoPath: "/r" }],
    });
    const byBob = (await list(bob)).find((m) => m.id === aliceId);
    expect(byBob?.ownerId).toBe(alice.id);
    expect(byBob?.personalBots).toBeUndefined();
    expect(byBob?.labels).toEqual([]);
  });

  it("checks the machine's manager on every machine route", async () => {
    const { admin, alice, bob } = await team();
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    const aliceId = (await addMachine(alice, { name: "alice-mac" })).body.machine.id as string;
    const routes = (id: string) => [
      { path: `/v1/machines/${id}/monitor?range=15m` },
      { path: `/v1/machines/${id}/secrets` },
      { path: `/v1/machines/${id}/secrets/OPENAI_API_KEY`, method: "PUT", body: { sealed: "x", members: [] } },
      { path: `/v1/machines/${id}/secrets/OPENAI_API_KEY`, method: "DELETE" },
      { path: `/v1/machines/${id}/logins`, body: { runtime: "codex" } },
      { path: `/v1/machines/${id}/logins/abc` },
      { path: `/v1/machines/${id}`, method: "PATCH", body: { name: "x" } },
      { path: `/v1/machines/${id}`, method: "DELETE" },
    ];
    const denied = async (m: TestMember, id: string) => {
      for (const r of routes(id)) {
        const res = await api(r.path, { token: m.token, method: r.method, body: r.body });
        expect(res.status, `${r.method ?? "GET"} ${r.path}`).toBe(403);
        expect(res.body.error?.reason).toBe("machine.not_yours");
      }
    };
    // An admin cannot touch someone else's personal machine; a member cannot touch a team machine
    // or someone else's.
    await denied(admin, aliceId);
    await denied(alice, teamId);
    await denied(bob, aliceId);
    // The owner reaches his machine's monitor and keys (offline: the daemon is not connected).
    expect((await api(`/v1/machines/${aliceId}/monitor?range=15m`, { token: alice.token })).status).toBe(200);
    const keys = await api(`/v1/machines/${aliceId}/secrets`, { token: alice.token });
    expect(keys.status, JSON.stringify(keys.body)).toBe(200);
    const off = await api(`/v1/machines/${aliceId}/logins`, { token: alice.token, body: { runtime: "codex" } });
    expect(off.body.error?.reason).toBe("machine.offline");
    // An admin still manages a team machine.
    expect((await api(`/v1/machines/${teamId}/monitor?range=15m`, { token: admin.token })).status).toBe(200);
    // Unknown machines are not_found for everyone.
    expect((await api("/v1/machines/01JBX0NOPE0000000000000000", { method: "DELETE", token: alice.token })).status).toBe(404);
  });

  it("renames, gives to the team and makes a team machine mine", async () => {
    const { admin, admin2, alice } = await team();
    // Rename: the owner of a personal machine, an admin for a team machine.
    const aliceId = (await addMachine(alice, { name: "alice-mac" })).body.machine.id as string;
    const renamed = await api(`/v1/machines/${aliceId}`, { method: "PATCH", token: alice.token, body: { name: "Alice 的 Mac" } });
    expect(renamed.status, JSON.stringify(renamed.body)).toBe(200);
    expect(renamed.body).toMatchObject({ id: aliceId, name: "Alice 的 Mac", ownerId: alice.id });
    expect((await api(`/v1/machines/${aliceId}`, { method: "PATCH", token: alice.token, body: {} })).status).toBe(400);
    // A member cannot give his machine to the team.
    const give = await api(`/v1/machines/${aliceId}`, { method: "PATCH", token: alice.token, body: { owner: null } });
    expect(give.status).toBe(403);
    expect(give.body.error?.reason).toBe("machine.team_admin_only");
    // Nor make a team machine his.
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    expect((await api(`/v1/machines/${teamId}`, { method: "PATCH", token: alice.token, body: { owner: "me" } })).status).toBe(403);
    expect((await api(`/v1/machines/${teamId}`, { method: "PATCH", token: admin.token, body: { name: "主机" } })).body.name).toBe("主机");

    // An admin's own machine with a checked AI member goes to the team: the checks go.
    const mineId = (await addMachine(admin, { name: "admin-mac", personal: true })).body.machine.id as string;
    const bot = await runnerBot(admin);
    await api(`/v1/machines/${mineId}/bots/${bot.id}`, { method: "PUT", token: admin.token, body: {} });
    const toTeam = await api(`/v1/machines/${mineId}`, { method: "PATCH", token: admin.token, body: { owner: null } });
    expect(toTeam.status, JSON.stringify(toTeam.body)).toBe(200);
    expect(toTeam.body.ownerId).toBeUndefined();
    expect(toTeam.body.personalBots).toBeUndefined();
    expect((await env.DB.prepare("SELECT count(*) AS n FROM machine_bots WHERE machine_id = ?").bind(mineId).first<{ n: number }>())!.n).toBe(0);
    // Now any admin manages it.
    expect((await api(`/v1/machines/${mineId}/monitor?range=15m`, { token: admin2.token })).status).toBe(200);

    // A team machine becomes admin2's: AI members someone else placed there are unpinned and
    // checked for admin2; the ones admin2 placed stay.
    const byAdmin = await runnerBot(admin, { machineId: mineId });
    const byAdmin2 = await runnerBot(admin2, { machineId: mineId });
    const toMine = await api(`/v1/machines/${mineId}`, { method: "PATCH", token: admin2.token, body: { owner: "me" } });
    expect(toMine.status, JSON.stringify(toMine.body)).toBe(200);
    expect(toMine.body).toMatchObject({ ownerId: admin2.id, personalBots: [{ botId: byAdmin.id }] });
    expect((await api(`/v1/bots/${byAdmin.id}/runner`, { token: admin.token })).body.machineId).toBeUndefined();
    expect((await api(`/v1/bots/${byAdmin2.id}/runner`, { token: admin.token })).body.machineId).toBe(mineId);
    // The other admin cannot manage it any more.
    const other = await api(`/v1/machines/${mineId}/secrets`, { token: admin.token });
    expect(other.body.error?.reason).toBe("machine.not_yours");
    // `owner: "me"` on someone else's machine is not possible.
    expect((await api(`/v1/machines/${aliceId}`, { method: "PATCH", token: admin.token, body: { owner: "me" } })).status).toBe(403);

    // All of it is in the activity log as machine.update.
    const acts = await env.DB.prepare("SELECT action, details FROM activity WHERE machine_id = ? AND action = 'machine.update' ORDER BY id")
      .bind(mineId)
      .all<{ action: string; details: string }>();
    expect(acts.results.map((r) => JSON.parse(r.details))).toEqual([{ owner: null }, { owner: "me" }]);
  });

  it("lets only one of two concurrent ownership changes win", async () => {
    const { admin, admin2 } = await team();
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    const bot = await runnerBot(admin, { machineId: teamId });
    // admin2 makes it his between admin's check and admin's write: admin's write changes nothing.
    const batch = env.DB.batch.bind(env.DB);
    let raced = false;
    env.DB.batch = (async (stmts: D1PreparedStatement[]) => {
      if (!raced) {
        raced = true;
        await env.DB.prepare("UPDATE machines SET owner_id = ? WHERE id = ?").bind(admin2.id, teamId).run();
      }
      return batch(stmts);
    }) as typeof env.DB.batch;
    let r;
    try {
      r = await api(`/v1/machines/${teamId}`, { method: "PATCH", token: admin.token, body: { owner: "me", name: "renamed" } });
    } finally {
      env.DB.batch = batch;
    }
    expect(raced).toBe(true);
    expect(r.status, JSON.stringify(r.body)).toBe(409);
    const row = await env.DB.prepare("SELECT owner_id, name FROM machines WHERE id = ?").bind(teamId).first<{ owner_id: string; name: string }>();
    expect(row).toEqual({ owner_id: admin2.id, name: "linux" });
    expect((await env.DB.prepare("SELECT count(*) AS n FROM machine_bots WHERE machine_id = ?").bind(teamId).first<{ n: number }>())!.n).toBe(0);
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: admin.token })).body.machineId).toBe(teamId);
    const acts = await env.DB.prepare("SELECT count(*) AS n FROM activity WHERE machine_id = ? AND action = 'machine.update'")
      .bind(teamId)
      .first<{ n: number }>();
    expect(acts!.n).toBe(0);
  });

  it("keeps personal machines for people: an admin AI member cannot make a team machine its own", async () => {
    const { admin } = await team();
    const botAdmin = await createMember({ kind: "bot", level: "owner" });
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    const r = await api(`/v1/machines/${teamId}`, { method: "PATCH", token: botAdmin.token, body: { owner: "me" } });
    expect(r.status).toBe(403);
    const row = await env.DB.prepare("SELECT owner_id FROM machines WHERE id = ?").bind(teamId).first<{ owner_id: string | null }>();
    expect(row!.owner_id).toBeNull();
  });

  it("checks AI members on a personal machine, by its owner only", async () => {
    const { admin, alice, bob } = await team();
    const aliceId = (await addMachine(alice, { name: "alice-mac" })).body.machine.id as string;
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    const bot = await runnerBot(admin);
    const put = (m: TestMember, id: string, botId: string, body: Record<string, unknown> = {}) =>
      api(`/v1/machines/${id}/bots/${botId}`, { method: "PUT", token: m.token, body });

    const set = await put(alice, aliceId, bot.id, { repoPath: "/Users/alice/code" });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    expect(set.body.personalBots).toEqual([{ botId: bot.id, repoPath: "/Users/alice/code" }]);
    // Absent keeps the folder, null clears it.
    expect((await put(alice, aliceId, bot.id)).body.personalBots).toEqual([{ botId: bot.id, repoPath: "/Users/alice/code" }]);
    expect((await put(alice, aliceId, bot.id, { repoPath: null })).body.personalBots).toEqual([{ botId: bot.id }]);
    // Not someone else, not an admin, not on a team machine.
    expect((await put(bob, aliceId, bot.id)).body.error?.reason).toBe("machine.not_yours");
    expect((await put(admin, aliceId, bot.id)).body.error?.reason).toBe("machine.not_yours");
    expect((await put(admin, teamId, bot.id)).status).toBe(403);
    // Only active runner-backed AI members.
    expect((await put(alice, aliceId, bob.id)).status).toBe(404);
    const plain = await createMember({ kind: "bot" });
    expect((await put(alice, aliceId, plain.id)).status).toBe(400);
    // The audit keeps no local path.
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'machine.bots.set' AND target = ? ORDER BY id")
      .bind(aliceId)
      .all<{ details: string }>();
    expect(audit.results.map((r) => JSON.parse(r.details))).toEqual([{ botId: bot.id, repoPath: true }, { botId: bot.id }, { botId: bot.id, repoPath: false }]);

    expect((await api(`/v1/machines/${aliceId}/bots/${bot.id}`, { method: "DELETE", token: bob.token })).status).toBe(403);
    const del = await api(`/v1/machines/${aliceId}/bots/${bot.id}`, { method: "DELETE", token: alice.token });
    expect(del.status).toBe(200);
    expect(del.body.personalBots).toEqual([]);
    const acts = await env.DB.prepare("SELECT action FROM activity WHERE machine_id = ? AND action LIKE 'machine.bots.%' ORDER BY id")
      .bind(aliceId)
      .all<{ action: string }>();
    expect(acts.results.map((r) => r.action)).toEqual(["machine.bots.set", "machine.bots.set", "machine.bots.set", "machine.bots.delete"]);
  });

  it("places an AI member on a personal machine only for its owner", async () => {
    const { admin, alice } = await team();
    const adminOwn = (await addMachine(admin, { name: "admin-mac", personal: true })).body.machine.id as string;
    const aliceId = (await addMachine(alice, { name: "alice-mac" })).body.machine.id as string;
    const bot = await createMember({ kind: "bot" });
    const put = (body: Record<string, unknown>) => api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: admin.token, body: { runtime: "codex", ...body } });
    // Onto my own personal machine: fine.
    expect((await put({ machineId: adminOwn })).body.machineId).toBe(adminOwn);
    // Onto someone else's: forbidden.
    const r = await put({ machineId: aliceId });
    expect(r.status).toBe(403);
    expect(r.body.error?.reason).toBe("machine.not_yours");
    // Stored on Alice's machine (her own placing, say before she was a member), an admin may still
    // edit the model by sending the stored machine back, or leave it out.
    await env.DB.prepare("UPDATE bot_runners SET machine_id = ? WHERE member_id = ?").bind(aliceId, bot.id).run();
    const edit = await put({ machineId: aliceId, model: "gpt-5" });
    expect(edit.status, JSON.stringify(edit.body)).toBe(200);
    expect(edit.body).toMatchObject({ machineId: aliceId, model: "gpt-5" });
    expect((await put({ model: "gpt-5.1" })).body.machineId).toBe(aliceId);
  });

  it("removes a machine by its manager, with its checks and pins", async () => {
    const { admin, alice } = await team();
    const created = await addMachine(alice, { name: "alice-mac" });
    const aliceId = created.body.machine.id as string;
    const bot = await runnerBot(admin);
    await api(`/v1/machines/${aliceId}/bots/${bot.id}`, { method: "PUT", token: alice.token, body: {} });
    const { d } = await Daemon.connect(created.body.token);
    expect((await api(`/v1/machines/${aliceId}`, { method: "DELETE", token: admin.token })).status).toBe(403);
    expect((await api(`/v1/machines/${aliceId}`, { method: "DELETE", token: alice.token })).status).toBe(200);
    await waitUntil(() => d.closed, 5000, "removed link closed");
    expect((await env.DB.prepare("SELECT count(*) AS n FROM machine_bots WHERE machine_id = ?").bind(aliceId).first<{ n: number }>())!.n).toBe(0);
    expect((await list(alice)).find((m) => m.id === aliceId)?.revoked).toBe(true);
    // Removed: further management is not_found.
    expect((await api(`/v1/machines/${aliceId}`, { method: "DELETE", token: alice.token })).status).toBe(404);
    expect((await api(`/v1/machines/${aliceId}`, { method: "PATCH", token: alice.token, body: { name: "x" } })).status).toBe(404);
  });

  it("removes a deactivated person's personal machines, audited as who deactivated him", async () => {
    const { admin, alice } = await team();
    const created = await addMachine(alice, { name: "alice-mac" });
    const aliceId = created.body.machine.id as string;
    const teamId = (await addMachine(admin, { name: "linux" })).body.machine.id as string;
    const bot = await runnerBot(admin);
    await api(`/v1/machines/${aliceId}/bots/${bot.id}`, { method: "PUT", token: alice.token, body: {} });
    // Alice placed a bot on her own machine before she lost the right (the stored pin goes).
    await env.DB.prepare("UPDATE bot_runners SET machine_id = ? WHERE member_id = ?").bind(aliceId, bot.id).run();
    const { d } = await Daemon.connect(created.body.token);

    const r = await api(`/v1/members/${alice.id}`, { method: "PATCH", token: admin.token, body: { deactivated: true } });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const row = await env.DB.prepare("SELECT revoked_at FROM machines WHERE id = ?").bind(aliceId).first<{ revoked_at: number | null }>();
    expect(row?.revoked_at).not.toBeNull();
    expect((await env.DB.prepare("SELECT revoked_at FROM machines WHERE id = ?").bind(teamId).first<{ revoked_at: number | null }>())?.revoked_at).toBeNull();
    expect((await env.DB.prepare("SELECT count(*) AS n FROM machine_bots WHERE machine_id = ?").bind(aliceId).first<{ n: number }>())!.n).toBe(0);
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: admin.token })).body.machineId).toBeUndefined();
    const audit = await env.DB.prepare("SELECT actor_id FROM audit WHERE action = 'machine.revoke' AND target = ?")
      .bind(aliceId)
      .first<{ actor_id: string | null }>();
    expect(audit?.actor_id).toBe(admin.id);
    await waitUntil(() => d.closed, 5000, "deactivated owner's link closed");
    const again = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${created.body.token}` } });
    expect(again.status).toBe(401);
  });

  it("keeps the removal when closing the link fails", async () => {
    const alice = await createMember();
    const ids = [
      (await addMachine(alice, { name: "a" })).body.machine.id as string,
      (await addMachine(alice, { name: "b" })).body.machine.id as string,
    ];
    let calls = 0;
    const removed = await revokePersonalMachines(env, alice.id, {
      closeLink: async () => {
        calls++;
        throw new Error("link unreachable");
      },
    });
    expect(removed.sort()).toEqual([...ids].sort());
    // Tried once more each, then logged.
    expect(calls).toBe(4);
    for (const id of ids) {
      const row = await env.DB.prepare("SELECT revoked_at FROM machines WHERE id = ?").bind(id).first<{ revoked_at: number | null }>();
      expect(row?.revoked_at).not.toBeNull();
    }
    // Nothing left to remove the second time.
    expect(await revokePersonalMachines(env, alice.id)).toEqual([]);
  });
});
