// The 群聊 (docs/protocol.md, "群聊: the group's own chat"): one chat topic per group with a fixed
// id, made by its first message; 开个话题 from one of its messages; no work status, no
// auto-follow and no responsible bot.
import { mainTopicId } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { Daemon, downViolations, world } from "./daemon";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

async function setup(n = 2) {
  const members = await Promise.all(Array.from({ length: n }, (_, i) => createMember({ name: `M${i}` })));
  const gid = await createGroup(members[0], members.slice(1).map((m) => m.id), "全员");
  const clients = await Promise.all(members.map(async (m) => (await Client.connect(m.token)).c));
  return { members, gid, clients };
}

describe("群聊", () => {
  it("is made by its first message and takes later messages without new topics", async () => {
    const { members, gid, clients } = await setup();
    const [a] = members;
    const [ca, cb] = clients;
    const main = mainTopicId(gid);

    // Before the first message: an empty history; typing is accepted (no ack).
    const empty = await ca.request({ t: "history", groupId: gid, topicId: main });
    expect(empty).toMatchObject({ t: "history", messages: [], hasMore: false });
    ca.send({ t: "typing", groupId: gid, topicId: main });

    const first = await ca.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: "大家早" });
    expect(first.topic).toMatchObject({ id: main, groupId: gid, main: true, title: "全员", status: "open", ownerId: null, rootMessageId: first.message.id });
    expect(first.message.topicId).toBe(main);
    const created = await cb.waitEvent((e) => e.type === "topic.created" && e.data.topic.id === main);
    expect(created.data.topic.main).toBe(true);

    const second = await cb.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: "早" });
    expect(second.topic).toBeUndefined();
    expect(second.message.topicId).toBe(main);
    expect(ca.events(gid).filter((e) => e.type === "topic.created")).toHaveLength(1);

    const page = await ca.request({ t: "history", groupId: gid, topicId: main });
    expect(page.messages.map((m: any) => m.body)).toEqual(["早", "大家早"]);

    // A resend of the first message is a duplicate that names the topic again.
    const dup = await ca.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: first.message.clientMsgId, body: "大家早" });
    expect(dup).toMatchObject({ duplicate: true, topic: { id: main, main: true } });

    // Another group's 群聊 id is not a topic here.
    const other = await createGroup(a, []);
    expect(await ca.request({ t: "send", groupId: gid, topicId: mainTopicId(other), clientMsgId: clientMsgId(), body: "x" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    for (const c of clients) c.close();
  });

  it("keeps no status, owner or follow of its own", async () => {
    const { members, gid, clients } = await setup();
    const [a, b] = members;
    const [ca] = clients;
    const main = mainTopicId(gid);
    await ca.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: `@M1 看一下`, mentions: [b.id] });

    const up = await ca.ok({ t: "topic.update", groupId: gid, topicId: main, status: "done", waitingOn: [b.id], ownerId: a.id, title: "改名" });
    expect(up.topic).toMatchObject({ id: main, status: "open", waitingOn: [], ownerId: null, title: "全员" });
    // Nothing was written: the next event is the next message.
    const next = await ca.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: "好" });
    const updated = ca.events(gid).filter((e) => e.type === "topic.updated" && e.data.topicId === main);
    expect(updated).toEqual([]);
    expect(next.message.createdSeq).toBeGreaterThan(0);

    // Neither the author nor the mentioned member follows it.
    for (const m of [a, b]) {
      const groups = await api("/v1/groups", { token: m.token });
      const g = groups.body.find((x: any) => x.group.id === gid);
      const state = (g.topics ?? []).find((t: any) => t.topicId === main);
      expect(state?.following ?? false).toBe(false);
    }
    // A mention there still counts for the mentioned member.
    const bGroups = await api("/v1/groups", { token: b.token });
    expect(bGroups.body.find((x: any) => x.group.id === gid).mentions).toBe(1);
    for (const c of clients) c.close();
  });

  it("opens a 话题 from one of its messages (开个话题)", async () => {
    const { gid, clients } = await setup();
    const [ca, cb] = clients;
    const main = mainTopicId(gid);
    const chat = await cb.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: "发布前要过一遍检查清单\n第二行" });
    const opened = await ca.ok({
      t: "send",
      groupId: gid,
      clientMsgId: clientMsgId(),
      title: "发布检查清单",
      body: "这里接着聊",
      replyTo: { messageId: chat.message.id },
    });
    expect(opened.topic).toMatchObject({ title: "发布检查清单", status: "open", rootMessageId: opened.message.id });
    expect(opened.topic.main).toBeUndefined();
    expect(opened.topic.id).not.toBe(main);
    expect(opened.message.replyTo).toMatchObject({ messageId: chat.message.id, excerpt: "发布前要过一遍检查清单\n第二行" });
    for (const c of clients) c.close();
  });

  it("has an empty title in a DM", async () => {
    const a = await createMember({ name: "A" });
    const b = await createMember({ name: "B" });
    const dm = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    const gid = dm.body.group.id as string;
    const { c } = await Client.connect(a.token);
    const r = await c.ok({ t: "send", groupId: gid, topicId: mainTopicId(gid), clientMsgId: clientMsgId(), body: "hi" });
    expect(r.topic).toMatchObject({ id: mainTopicId(gid), main: true, title: "" });
    c.close();
  });

  it("sends each @mention to that bot, with no responsible bot and no topic status", async () => {
    const w = await world();
    const other = await createMember({ kind: "bot", name: "ops" });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: other.id } });
    await api(`/v1/bots/${other.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex" } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const main = mainTopicId(w.gid);

    // Plain chat reaches no bot.
    await c.ok({ t: "send", groupId: w.gid, topicId: main, clientMsgId: clientMsgId(), body: "今天先对一下发布" });
    await c.ok({ t: "send", groupId: w.gid, topicId: main, clientMsgId: clientMsgId(), body: "@dev 修一下测试", mentions: [w.bot.id] });
    const first = await d.next((f) => f.t === "run.assign");
    expect(first).toMatchObject({ botMemberId: w.bot.id, topicId: main });
    d.send({ t: "run.accept", runId: first.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "running");

    // A mention of another bot starts its own run in the 群聊; unmentioned chat goes to neither run.
    await c.ok({ t: "send", groupId: w.gid, topicId: main, clientMsgId: clientMsgId(), body: "顺便聊点别的" });
    await c.ok({ t: "send", groupId: w.gid, topicId: main, clientMsgId: clientMsgId(), body: "@ops 看一下告警", mentions: [other.id] });
    const second = await d.next((f) => f.t === "run.assign" && f.botMemberId === other.id);
    expect(second.topicId).toBe(main);
    expect(d.frames.filter((f) => f.t === "run.input")).toEqual([]);

    // A later mention of dev goes to dev's active run.
    await c.ok({ t: "send", groupId: w.gid, topicId: main, clientMsgId: clientMsgId(), body: "@dev 还有 lint", mentions: [w.bot.id] });
    const input = await d.next((f) => f.t === "run.input");
    expect(input).toMatchObject({ runId: first.runId, text: "@dev 还有 lint" });

    // The run's status reports change nothing on the 群聊.
    const s = d.done(first.runId, { status: "succeeded", exitCode: 0, resultText: "修好了", summary: "修好了" });
    await d.acked(first.runId, s);
    await waitUntil(
      () => c.events(w.gid).find((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.topicId === main),
      5000,
      "result in the 群聊",
    );
    expect(c.events(w.gid).filter((e) => e.type === "topic.updated" && e.data.topicId === main)).toEqual([]);
    c.close();
    d.close();
  });
});
