import { afterAll, describe, expect, it } from "vitest";
import { Client, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

describe("reactions", () => {
  it("toggle one reaction per member and emoji, fan out reaction.changed and keep the list on the message", async () => {
    const members = await Promise.all([0, 1, 2].map((i) => createMember({ name: `R${i}` })));
    const [a, b] = members;
    const gid = await createGroup(a, members.slice(1).map((m) => m.id));
    const [ca, cb, cc] = await Promise.all(members.map(async (m) => (await Client.connect(m.token)).c));
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "ship it?" });
    const mid = r.message.id;
    const react = (c: Client, emoji: string, on: boolean) => c.ok({ t: "react", groupId: gid, messageId: mid, emoji, on });

    expect(await react(cb, "👍", true)).toEqual({ changed: true });
    const ev = await cc.waitEvent((e) => e.type === "reaction.changed");
    expect(ev).toMatchObject({ actorId: b.id, data: { messageId: mid, emoji: "👍", on: true } });
    expect(await react(cb, "👍", true)).toEqual({ changed: false });
    expect(await react(ca, "👍", true)).toEqual({ changed: true });
    expect(await react(ca, "✅", true)).toEqual({ changed: true });

    const h1 = await cc.request({ t: "history", groupId: gid, topicId: r.topic.id });
    expect(h1.messages[0].reactions).toEqual([
      { emoji: "👍", memberIds: [b.id, a.id] },
      { emoji: "✅", memberIds: [a.id] },
    ]);

    expect(await react(cb, "👍", false)).toEqual({ changed: true });
    expect(await react(ca, "✅", false)).toEqual({ changed: true });
    expect(await react(ca, "✅", false)).toEqual({ changed: false });
    const h2 = await cc.request({ t: "history", groupId: gid, topicId: r.topic.id });
    expect(h2.messages[0].reactions).toEqual([{ emoji: "👍", memberIds: [a.id] }]);

    // An edit keeps reactions; bad emoji and deleted messages are refused.
    const edited = await ca.ok({ t: "edit", groupId: gid, messageId: mid, body: "ship it now?" });
    expect(edited.message.reactions).toEqual([{ emoji: "👍", memberIds: [a.id] }]);
    expect(await cb.request({ t: "react", groupId: gid, messageId: mid, emoji: "a b", on: true })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    expect(await cb.request({ t: "react", groupId: gid, messageId: "nope", emoji: "👍", on: true })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    await ca.ok({ t: "delete", groupId: gid, messageId: mid });
    expect(await cb.request({ t: "react", groupId: gid, messageId: mid, emoji: "👍", on: true })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });

    // The log has every change in order.
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const changes = page.events.filter((e: any) => e.type === "reaction.changed").map((e: any) => [e.data.emoji, e.data.on]);
    expect(changes).toEqual([
      ["👍", true],
      ["👍", true],
      ["✅", true],
      ["👍", false],
      ["✅", false],
    ]);
    for (const c of [ca, cb, cc]) c.close();
  });

  it("refuse a member outside the group", async () => {
    const [a, b, out] = await Promise.all([0, 1, 2].map((i) => createMember({ name: `O${i}` })));
    const gid = await createGroup(a, [b.id]);
    const ca = (await Client.connect(a.token)).c;
    const co = (await Client.connect(out.token)).c;
    const r = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "x" });
    expect(await co.request({ t: "react", groupId: gid, messageId: r.message.id, emoji: "👍", on: true })).toMatchObject({
      ok: false,
      error: { code: "forbidden" },
    });
    ca.close();
    co.close();
  });
});
