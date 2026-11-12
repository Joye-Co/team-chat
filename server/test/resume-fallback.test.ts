// design-vm-state 3.2: a resumed run carries the full prompt for when the machine cannot find the
// session, and a run.done with resumeMissed makes the run context show the full prompt.
import { MAX_FRAME_BYTES, type RunAssignFrame } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { withFullPrompt } from "../src/bots/desk/runs";
import { Daemon, downViolations, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

async function finish(d: Daemon, runId: string, extra: Record<string, unknown>): Promise<void> {
  d.send({ t: "run.accept", runId });
  await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "done", ...extra }));
}

async function send(c: Client, gid: string, topicId: string | undefined, body: string, extra: Record<string, unknown> = {}) {
  return c.ok({ t: "send", groupId: gid, ...(topicId ? { topicId } : {}), clientMsgId: clientMsgId(), body, ...extra });
}

const frame = (prompt: string): RunAssignFrame => ({
  t: "run.assign",
  runId: "r1",
  botMemberId: "b1",
  groupId: "g1",
  topicId: "t1",
  runtime: "claude",
  prompt,
  envAllowlist: [],
  leaseMs: 600_000,
  resumeSessionId: "s1",
});

describe("withFullPrompt", () => {
  it("carries the full prompt when the frame stays within the limit", () => {
    expect(withFullPrompt(frame("new"), "old\n\nnew").fullPrompt).toBe("old\n\nnew");
  });

  it("leaves it out when the frame would pass the limit", () => {
    // Each prompt fits alone; both together do not.
    const big = "x".repeat(MAX_FRAME_BYTES * 0.6);
    expect(withFullPrompt(frame(big), big).fullPrompt).toBeUndefined();
    // Bytes, not characters: 400k characters of Chinese are 1.2 MB.
    const zh = "长".repeat(200_000);
    expect(withFullPrompt(frame(zh), zh).fullPrompt).toBeUndefined();
  });
});

describe("resume fallback", () => {
  it("sends the full prompt with resumeSessionId only, and resumeMissed shows the full prompt", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume", "context"] });
    const { c } = await Client.connect(w.human.token);
    const first = await send(c, w.gid, undefined, "需求：登录页要支持手机号");
    const topicId = first.message.topicId as string;
    await send(c, w.gid, topicId, "@dev 按第 1 条做", { mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.resumeSessionId).toBeUndefined();
    expect(a.fullPrompt).toBeUndefined();
    await finish(d, a.runId, { sessionId: "thread-1" });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    await send(c, w.gid, topicId, "@dev 再加一下邮箱", { mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBe("thread-1");
    expect(b.prompt).not.toContain("需求：登录页要支持手机号");
    expect(b.fullPrompt).toContain("需求：登录页要支持手机号");
    expect(b.fullPrompt).toContain("再加一下邮箱");
    const before = await api(`/v1/runs/${b.runId}/context`, { token: w.owner.token });
    expect(before.body.prompt).toBe(b.prompt);
    expect(before.body.manifest).toMatchObject({ resumed: true });

    // The machine could not find the session and used the full prompt.
    await finish(d, b.runId, { sessionId: "thread-2", resumeMissed: true });
    const after = await api(`/v1/runs/${b.runId}/context`, { token: w.owner.token });
    expect(after.status).toBe(200);
    expect(after.body.prompt).toBe(b.fullPrompt);
    expect(after.body.manifest).toMatchObject({ resumed: false });
    d.close();
    c.close();
  });
});
