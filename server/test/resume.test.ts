// 1.1 resume (docs/machine-protocol.md "resume"): BotDesk keeps the CLI session of a run and
// gives the topic's next run on the same machine `run.assign.resumeSessionId`.
import { afterAll, describe, expect, it } from "vitest";
import { asideRuntime, resumesRuntime } from "../src/bots/dispatch";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

/** Runs one assignment to the end with the session `sessionId`. */
async function finish(d: Daemon, runId: string, sessionId: string, viaEvent = false): Promise<void> {
  d.send({ t: "run.accept", runId });
  if (viaEvent) d.event(runId, { type: "session", sessionId, runtime: "codex" });
  const seq = d.done(runId, { status: "succeeded", resultText: "done", ...(viaEvent ? {} : { sessionId }) });
  await d.acked(runId, seq);
}

describe("resume", () => {
  it("give the topic's next run on the same machine the last session, only to a daemon with feature resume", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const first = await d.next((f) => f.t === "run.assign");
    expect(first.resumeSessionId).toBeUndefined();
    await finish(d, first.runId, "thread-1");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "done");

    // An answer in the topic after the CLI exited: the next run continues the session.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 再加一个测试", mentions: [w.bot.id] });
    const second = await d.next((f) => f.t === "run.assign" && f.runId !== first.runId);
    expect(second).toMatchObject({ topicId, resumeSessionId: "thread-1" });
    // The session from the `session` event counts too, and the newest run wins.
    await finish(d, second.runId, "thread-2", true);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === second.runId && e.data.run.status === "done");
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 还有文档", mentions: [w.bot.id] });
    const third = await d.next((f) => f.t === "run.assign" && f.runId !== first.runId && f.runId !== second.runId);
    expect(third.resumeSessionId).toBe("thread-2");
    d.send({ t: "run.accept", runId: third.runId });
    await d.acked(third.runId, d.done(third.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();

    // Another topic starts fresh; a daemon without the feature never gets the field.
    const w2 = await world();
    const old = await Daemon.connect(w2.machineToken, {});
    const { c: c2 } = await Client.connect(w2.human.token);
    const t2 = await mention(c2, w2);
    const a = await old.d.next((f) => f.t === "run.assign");
    await finish(old.d, a.runId, "thread-9");
    await c2.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");
    await c2.ok({ t: "send", groupId: w2.gid, topicId: t2, clientMsgId: clientMsgId(), body: "@dev 再来", mentions: [w2.bot.id] });
    const b = await old.d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBeUndefined();
    old.d.close();
    c2.close();
  });

  // 接 3.6: an ACP agent continues its session only when the machine says its CLI can
  // (session/load); otherwise the next run gets the full prompt, as a first run does.
  for (const [name, cli, resumes] of [
    ["an ACP agent with session/load", { kind: "acp", command: "opencode", resume: true }, true],
    ["an ACP agent without it", { kind: "acp", command: "gemini", resume: false }, false],
    ["an ACP agent of a daemon that does not say", { kind: "acp", command: "gemini" }, false],
  ] as const) {
    it(`continue the session of ${name}: ${resumes}`, async () => {
      const w = await world({ runtime: "agent" });
      const { d } = await Daemon.connect(w.machineToken, { features: ["resume"], runtime: "agent", cli });
      const { c } = await Client.connect(w.human.token);
      const topicId = await mention(c, w, "@dev 第一件事：修好登录");
      const first = await d.next((f) => f.t === "run.assign");
      expect(first.prompt).toContain("修好登录");
      await finish(d, first.runId, "ses_1");
      await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "done");

      await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 接着加测试", mentions: [w.bot.id] });
      const second = await d.next((f) => f.t === "run.assign" && f.runId !== first.runId);
      expect(second.prompt).toContain("接着加测试");
      if (resumes) {
        expect(second.resumeSessionId).toBe("ses_1");
        // Only what the session has not seen.
        expect(second.prompt).not.toContain("修好登录");
      } else {
        expect(second.resumeSessionId).toBeUndefined();
        // The full topic, as for a first run.
        expect(second.prompt).toContain("修好登录");
      }
      const ctx = await api(`/v1/runs/${second.runId}/context`, { token: w.owner.token });
      expect(ctx.status).toBe(200);
      expect(ctx.body.manifest.resumed).toBe(resumes);
      d.send({ t: "run.accept", runId: second.runId });
      await d.acked(second.runId, d.done(second.runId, { status: "succeeded", resultText: "ok" }));
      d.close();
      c.close();
    });
  }

  it("know per CLI which runtimes continue sessions and run side chats read-only", () => {
    const m = (features: string[], cli: Record<string, unknown>) => ({
      info: { features } as never,
      clis: [{ runtime: "rt", command: "x", available: true, ...cli } as never],
    });
    // A daemon that reports the fields.
    expect(resumesRuntime(m(["resume"], { kind: "acp", resume: true }), "rt")).toBe(true);
    expect(resumesRuntime(m(["resume"], { kind: "acp", resume: false }), "rt")).toBe(false);
    expect(resumesRuntime(m(["resume"], { kind: "claude", resume: false }), "rt")).toBe(false);
    expect(asideRuntime(m(["aside"], { kind: "acp", aside: false }), "rt")).toBe(false);
    expect(asideRuntime(m(["aside"], { kind: "codex", aside: true }), "rt")).toBe(true);
    // An older daemon: by kind.
    expect(resumesRuntime(m(["resume"], { kind: "codex" }), "rt")).toBe(true);
    expect(resumesRuntime(m(["resume"], { kind: "acp" }), "rt")).toBe(false);
    expect(resumesRuntime(m(["resume"], { kind: "pty" }), "rt")).toBe(false);
    expect(asideRuntime(m(["aside"], { kind: "claude" }), "rt")).toBe(true);
    expect(asideRuntime(m(["aside"], { kind: "acp" }), "rt")).toBe(false);
    // The feature is still needed, and the runtime must be on the machine.
    expect(resumesRuntime(m([], { kind: "acp", resume: true }), "rt")).toBe(false);
    expect(asideRuntime(m([], { kind: "claude", aside: true }), "rt")).toBe(false);
    expect(resumesRuntime(m(["resume"], { kind: "claude" }), "other")).toBe(false);
  });
});
