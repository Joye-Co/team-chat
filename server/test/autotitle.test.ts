// AI topic titles (src/autotitle): a topic started without a title gets one after its first
// message; a title given at creation stays. AUTOTITLE_FAKE stands in for Claude in tests.
import { mainTopicId } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { cleanTitle, titlePrompt } from "../src/autotitle";
import { Client, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

describe("autotitle", () => {
  it("names a topic opened from a 群聊 message", async () => {
    const a = await createMember({ name: "A" });
    const gid = await createGroup(a, [], "全员");
    const { c } = await Client.connect(a.token);
    const main = mainTopicId(gid);
    const src = await c.ok({ t: "send", groupId: gid, topicId: main, clientMsgId: clientMsgId(), body: "登录页报错了 [autotitle]" });
    const opened = await c.ok({
      t: "send",
      groupId: gid,
      clientMsgId: clientMsgId(),
      body: "跟一下 [autotitle]",
      replyTo: { messageId: src.message.id },
    });
    const topicId = opened.topic.id;
    expect(opened.topic.title).toBe("跟一下 [autotitle]");
    const titled = () => c.events(gid).find((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.title);
    await waitUntil(() => Boolean(titled()));
    expect(titled().data.title).toBe("AI 登录页报错了 [autotitle]");
  });

  it("opens a topic from a message with nothing added and names it", async () => {
    const a = await createMember({ name: "C" });
    const gid = await createGroup(a, [], "全员");
    const { c } = await Client.connect(a.token);
    const src = await c.ok({ t: "send", groupId: gid, topicId: mainTopicId(gid), clientMsgId: clientMsgId(), body: "周五发版 [autotitle]" });
    const opened = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", replyTo: { messageId: src.message.id } });
    expect(opened.topic.title).toBe("周五发版 [autotitle]");
    const topicId = opened.topic.id;
    const titled = () => c.events(gid).find((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.title);
    await waitUntil(() => Boolean(titled()));
    expect(titled().data.title).toBe("AI 周五发版 [autotitle]");
  });

  it("keeps a title given at creation", async () => {
    const a = await createMember({ name: "B" });
    const gid = await createGroup(a, [], "全员");
    const { c } = await Client.connect(a.token);
    const sent = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), title: "我起的", body: "内容 [autotitle]" });
    await new Promise((r) => setTimeout(r, 500));
    expect(sent.topic.title).toBe("我起的");
    const later = await c.ok({ t: "send", groupId: gid, topicId: sent.topic.id, clientMsgId: clientMsgId(), body: "再说一句" });
    expect(later.message.topicId).toBe(sent.topic.id);
    expect(c.events(gid).some((e: any) => e.type === "topic.updated" && e.data.topicId === sent.topic.id && e.data.title)).toBe(false);
  });

  it("cleans model text into one short line", () => {
    expect(cleanTitle("标题：「登录页报错排查」。\n多余")).toBe("登录页报错排查");
    expect(cleanTitle("   \n")).toBeNull();
    expect(titlePrompt({ body: "b", quote: "q" })).toContain("opened from this chat message:\nq");
  });
});
