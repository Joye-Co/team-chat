// 活动记录: admin events of the audit log copied by the 0013 trigger, filtered, owners only, 90 days.
import { env } from "cloudflare:test";
import { ActivityResponse } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { api, createMember } from "./helpers";

async function activity(token: string, query = "") {
  const r = await api(`/v1/activity${query ? `?${query}` : ""}`, { token });
  return r;
}

describe("activity", () => {
  it("records level and budget changes, invites, machines and keys; owners read and filter them", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const level = await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { level: "owner" } });
    expect(level.status, JSON.stringify(level.body)).toBe(200);
    await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { weeklyBudgetCents: 5000 } });
    await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { level: "member" } });
    const m = await api("/v1/machines", { token: owner.token, body: { name: "runner-x", labels: [] } });
    expect(m.status).toBe(200);
    const machineId = m.body.machine.id as string;
    // Writes that are not admin events stay out.
    await api("/v1/invites", { token: owner.token, body: { name: "Lina", kind: "human", level: "member" } });

    expect((await activity(alice.token)).status).toBe(403);
    const all = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}`)).body);
    const kinds = all.entries.map((e) => [e.kind, e.action]);
    expect(kinds).toEqual([
      ["invite", "invite.create"],
      ["machine", "machine.create"],
      ["level", "member.update"],
      ["budget", "member.update"],
      ["level", "member.update"],
    ]);
    expect(all.entries[2]).toMatchObject({ target: alice.id, details: { level: ["owner", "member"] } });
    expect(all.entries[1]).toMatchObject({ machineId, target: machineId });

    const budget = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&kind=budget`)).body);
    expect(budget.entries.map((e) => e.details)).toEqual([{ weeklyBudgetCents: [null, 5000] }]);
    const onMachine = ActivityResponse.parse((await activity(owner.token, `machine=${machineId}`)).body);
    expect(onMachine.entries.map((e) => e.action)).toEqual(["machine.create"]);
    const page = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&limit=2`)).body);
    expect(page.hasMore).toBe(true);
    const next = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&limit=2&before=${page.entries[1].id}`)).body);
    expect(next.entries[0].id).toBeLessThan(page.entries[1].id);
    expect((await activity(owner.token, "kind=chat")).status).toBe(400);
  });

  it("leaves out entries older than 90 days", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const old = Date.now() - 91 * 86_400_000;
    await env.DB.prepare("INSERT INTO audit (actor_id, action, target, details, at) VALUES (?, 'machine.revoke', 'm-old', '{}', ?)")
      .bind(owner.id, old)
      .run();
    const r = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}`)).body);
    expect(r.entries.some((e) => e.target === "m-old")).toBe(false);
  });
});
