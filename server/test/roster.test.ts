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
});
