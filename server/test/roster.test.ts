// 成员 of 团队管理: joined, added by, devices and pending invites (owners only).
import { TeamRoster } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { api, createMember } from "./helpers";

describe("team roster", () => {
  it("lists when members joined, who added them, their devices and open invites", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const inv = await api("/v1/invites", { token: owner.token, body: { name: "Lina", kind: "human", level: "member", role: "设计" } });
    expect(inv.status).toBe(200);
    expect((await api("/v1/team/roster", { token: alice.token })).status).toBe(403);
    const r = TeamRoster.parse((await api("/v1/team/roster", { token: owner.token })).body);
    const a = r.members.find((x) => x.memberId === alice.id);
    expect(a?.joinedAt).toBeGreaterThan(0);
    expect(a?.devices.length).toBeGreaterThanOrEqual(1);
    expect(r.invites.find((i) => i.name === "Lina")).toMatchObject({ kind: "human", level: "member", role: "设计", createdBy: owner.id });
    expect(JSON.stringify(r)).not.toContain("hash");
  });

  it("lets an owner make an invite that ends sooner", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const before = Date.now();
    const inv = await api("/v1/invites", { token: owner.token, body: { name: "Lina", kind: "human", level: "member", expiresInHours: 36 } });
    expect(inv.status).toBe(200);
    expect(inv.body.expiresAt).toBeGreaterThanOrEqual(before + 36 * 3_600_000);
    expect(inv.body.expiresAt).toBeLessThan(before + 37 * 3_600_000);
    const r = TeamRoster.parse((await api("/v1/team/roster", { token: owner.token })).body);
    expect(r.invites.find((i) => i.name === "Lina")?.expiresAt).toBe(inv.body.expiresAt);
    const week = await api("/v1/invites", { token: owner.token, body: { name: "Mo", kind: "human", level: "member" } });
    expect(week.body.expiresAt).toBeGreaterThanOrEqual(before + 7 * 86_400_000);
    expect((await api("/v1/invites", { token: owner.token, body: { name: "Al", kind: "human", level: "member", expiresInHours: 169 } })).status).toBe(400);
  });
});
