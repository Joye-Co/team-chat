import { env } from "cloudflare:test";
import { beforeAll, describe, expect, it } from "vitest";
import { Client, api } from "./helpers";
import { protocolViolations } from "./helpers";
import { afterAll as afterAllCheck } from "vitest";

afterAllCheck(() => {
  expect(protocolViolations).toEqual([]);
});

describe("bootstrap", () => {
  beforeAll(async () => {
    // Files run one at a time; start this one from an empty directory.
    for (const t of ["audit", "group_members", "groups", "invites", "tokens", "members"]) {
      await env.DB.prepare(`DELETE FROM ${t}`).run();
    }
  });

  it("needs the admin secret and works only once", async () => {
    const wrong = await api("/v1/admin/bootstrap", {
      body: { ownerName: "Joye" },
      headers: { "x-admin-secret": "wrong" },
    });
    expect(wrong.status).toBe(401);
    const none = await api("/v1/admin/bootstrap", { body: { ownerName: "Joye" } });
    expect(none.status).toBe(401);

    const ok = await api("/v1/admin/bootstrap", {
      body: { ownerName: "Joye" },
      headers: { "x-admin-secret": "test-admin-secret" },
    });
    expect(ok.status).toBe(200);
    expect(ok.body.member).toMatchObject({ name: "Joye", level: "owner", kind: "human", teamOwner: true });
    expect(ok.body.group).toMatchObject({ name: "全员", kind: "group", memberIds: [ok.body.member.id] });

    const again = await api("/v1/admin/bootstrap", {
      body: { ownerName: "Eve" },
      headers: { "x-admin-secret": "test-admin-secret" },
    });
    expect(again.status).toBe(409);
    expect(again.body.error.code).toBe("conflict");

    const stored = await env.DB.prepare("SELECT token_hash FROM tokens").all<any>();
    expect(stored.results).toHaveLength(1);
    expect(stored.results[0].token_hash).not.toContain("tc_");

    // A redeemed member joins 全员 and the owner sees it live.
    const { c } = await Client.connect(ok.body.token);
    const inv = await api("/v1/invites", { token: ok.body.token, body: { name: "Ann", kind: "human", level: "member" } });
    const red = await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "mac" } });
    const ev = await c.waitEvent((e) => e.type === "group.memberAdded");
    expect(ev).toMatchObject({ groupId: ok.body.group.id, seq: 1, data: { memberId: red.body.member.id } });
    await c.next((f) => f.t === "directory" && f.members?.some((m: any) => m.id === red.body.member.id));
    const groups = await api("/v1/groups", { token: red.body.token });
    expect(groups.body.map((s: any) => s.group.name)).toEqual(["全员"]);
    c.close();
  });

  it("starts a new person with a DM with Jojo, else the first bot", async () => {
    for (const t of ["audit", "group_members", "groups", "invites", "tokens", "members"]) {
      await env.DB.prepare(`DELETE FROM ${t}`).run();
    }
    const ok = await api("/v1/admin/bootstrap", {
      body: { ownerName: "Joye" },
      headers: { "x-admin-secret": "test-admin-secret" },
    });
    const owner = ok.body.token;
    const bot = async (name: string) => {
      const inv = await api("/v1/invites", { token: owner, body: { name, kind: "bot", level: "member" } });
      return (await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: name } })).body;
    };
    const person = async (name: string) => {
      const inv = await api("/v1/invites", { token: owner, body: { name, kind: "human", level: "member" } });
      return (await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "mac" } })).body;
    };
    const dmPeers = async (token: string, me: string) =>
      (await api("/v1/groups", { token })).body
        .filter((s: any) => s.group.kind === "dm")
        .map((s: any) => s.group.memberIds.find((id: string) => id !== me));

    // No bot yet: nothing to open.
    const early = await person("Ann");
    expect(await dmPeers(early.token, early.member.id)).toEqual([]);

    const forge = await bot("Forge");
    // A bot itself gets no DM.
    expect(await dmPeers(forge.token, forge.member.id)).toEqual([]);
    const bea = await person("Bea");
    expect(await dmPeers(bea.token, bea.member.id)).toEqual([forge.member.id]);

    const jojo = await bot("Jojo");
    const cal = await person("Cal");
    expect(await dmPeers(cal.token, cal.member.id)).toEqual([jojo.member.id]);
    // The bot has it too, so its first message lands in the same DM.
    expect(await dmPeers(jojo.token, jojo.member.id)).toEqual([cal.member.id]);
  });
});
