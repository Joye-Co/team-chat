// 模 2.2, 2.3, 2.10, 3.1, 3.2, 3.5; 接 5.3; 盯 1.13; 机 2.5, 5.2: a failed run says what went wrong
// and what to do, a signed-out CLI gets no work, and the run keeps the model it really used.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import {
  type MachineView,
  TEXT,
  candidates,
  clockText,
  failureKind,
  failureWords,
  whyNoMachine,
} from "../src/bots/dispatch";
import { SIGNED_OUT_MARK_MS, signedOutRuntimes } from "../src/bots/link";
import type { BotsEnv } from "../src/bots/env";
import { Daemon, HEALTH, downViolations, mention, world } from "./daemon";
import { Client, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;

afterAll(() => {
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

const login = (runtime: string, state: string) => ({ runtime, state });

describe("failure kinds: pure rules", () => {
  const now = Date.UTC(2026, 10, 12, 4, 0); // 12:00 in Shanghai

  it("read the daemon's kind, an unknown one as other, and #103's model_not_found from older daemons", () => {
    expect(failureKind({ kind: "signed_out" }, "")).toBe("signed_out");
    expect(failureKind({ kind: "quota_paused" }, "")).toBe("other");
    expect(failureKind(undefined, '{"error":{"message":"x","code":"model_not_found"}}')).toBe("model");
    expect(failureKind(undefined, "Not logged in")).toBe("other");
  });

  it("say when a limit resets on the team's clock", () => {
    expect(clockText(now + 3 * 3600_000, now, "Asia/Shanghai")).toBe("今天 15:00");
    expect(clockText(now + 24 * 3600_000, now, "Asia/Shanghai")).toBe("11 月 13 日 12:00");
  });

  it("give one sentence and one next step per kind", () => {
    const ctx = { machines: ["mac-mini"], cli: "Claude Code", now, zone: "Asia/Shanghai", signIn: { self: true as const } };
    expect(failureWords("signed_out", ctx)).toEqual({
      sentence: "mac-mini 上的 Claude Code 没有登录，或者登录已经过期",
      next: "请到 团队管理 › 机器 › mac-mini 重新登录，然后再 @我。",
    });
    expect(failureWords("signed_out", { ...ctx, signIn: { names: ["Joye"] } }).next).toBe(TEXT.next.signInAsk(["Joye"]));
    expect(failureWords("signed_out", { ...ctx, signIn: { names: [] } }).next).toBe(TEXT.next.signInAdmin);
    expect(failureWords("limit", { ...ctx, resetsAt: now + 3 * 3600_000 }).next).toBe("额度在 今天 15:00 恢复，到时再 @我。");
    // A reset in the past, or none: no time.
    expect(failureWords("limit", { ...ctx, resetsAt: now - 1 }).next).toBe(TEXT.next.limitLater);
    expect(failureWords("model", ctx).next).toBe(TEXT.next.model);
    expect(failureWords("network", ctx).sentence).toContain("mac-mini");
    expect(failureWords("not_installed", ctx).sentence).toBe("mac-mini 上没有装 Claude Code");
    expect(failureWords("other", ctx)).toEqual({ sentence: TEXT.failure.other, next: TEXT.next.other });
    // No developer words reach the topic.
    for (const k of ["signed_out", "limit", "model", "network", "not_installed", "other"] as const) {
      const w = failureWords(k, ctx);
      expect(`${w.sentence}${w.next}`).not.toMatch(/CLI|session|token|runtime|会话|令牌/i);
    }
  });

  it("take the runtime out while a heartbeat or a failed run says it is signed out", () => {
    expect(signedOutRuntimes([login("claude", "logged_out"), login("codex", "logged_in")], 100, {}, 200)).toEqual(["claude"]);
    // A run failed to sign in after the last heartbeat: out, until a later heartbeat says signed in.
    expect(signedOutRuntimes([login("codex", "logged_in")], 100, { codex: 150 }, 200)).toEqual(["codex"]);
    expect(signedOutRuntimes([login("codex", "logged_in")], 160, { codex: 150 }, 200)).toEqual([]);
    expect(signedOutRuntimes([login("codex", "logged_out")], 160, { codex: 150 }, 200)).toEqual(["codex"]);
    // An old mark leaves it to the heartbeat; "unknown" is no reason to hold work back.
    expect(signedOutRuntimes(undefined, null, { codex: 0 }, SIGNED_OUT_MARK_MS + 1)).toEqual([]);
    expect(signedOutRuntimes([login("codex", "unknown")], 100, {}, 200)).toEqual([]);
  });

  it("route no run to a signed-out runtime and say so when none is left (模 3.1)", () => {
    const at = 1_000_000_000;
    const m = (id: string, signedOut?: string[]): MachineView => ({
      id,
      name: id,
      labels: ["mac"],
      revoked: false,
      connected: true,
      lastHeartbeatAt: at - 1000,
      info: { name: id, labels: [], daemonVersion: "1", os: "macos", arch: "arm64", maxParallelRuns: 2 },
      health: HEALTH,
      clis: [{ runtime: "codex", kind: "codex", command: "codex", available: true }],
      runningRuns: [],
      ...(signedOut ? { signedOut } : {}),
    });
    const runner = { runtime: "codex", labels: ["mac"] };
    expect(candidates([m("a", ["codex"]), m("b")], runner, at).map((x) => x.id)).toEqual(["b"]);
    // Another runtime signed out does not matter.
    expect(candidates([m("a", ["claude"])], runner, at).map((x) => x.id)).toEqual(["a"]);
    const none = whyNoMachine([m("a", ["codex"]), m("b", ["codex"])], runner, at);
    expect(none.why).toBe("signed_out");
    expect(none.why === "signed_out" && none.machines.map((x) => x.id)).toEqual(["a", "b"]);
    // One signed in but busy: wait for it.
    expect(whyNoMachine([m("a", ["codex"]), { ...m("b"), runningRuns: ["r1", "r2"] }], runner, at).why).toBe("busy");
  });
});

describe("failure kinds in the topic", () => {
  it("say a signed-out CLI in plain words, ask the admin, keep the kind and the model, and send no more work there", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    d.event(runId, { type: "session", sessionId: "s-1", runtime: "codex", model: "gpt-test-1", effort: "high" });
    const seq = d.done(runId, {
      status: "failed",
      error: "workspace routing discovery unauthorized (401)",
      failure: { kind: "signed_out" },
    });
    await d.acked(runId, seq);
    const ended = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "failed");
    expect(ended.data.run).toMatchObject({ model: "gpt-test-1", effort: "high", failure: { kind: "signed_out" } });
    const [msg] = botMessages(c, topicId, w.bot.id);
    // Alice is no admin: the topic names the computer and asks Joye, who is mentioned.
    expect(msg.body).toBe(TEXT.failedWith(TEXT.failure.signedOut("mac-mini", "Codex"), TEXT.next.signInAsk(["Joye"])));
    expect(msg.mentions).toEqual([w.owner.id]);
    // The CLI's own words stay in the run log for the work record.
    const row = await runInDurableObject(benv.BOT_DESK.get(benv.BOT_DESK.idFromName(w.bot.id)), (_o, state) =>
      state.storage.sql.exec("SELECT frame FROM run_log WHERE run_id = ? AND seq = ?", runId, seq).one(),
    );
    expect(JSON.parse(row.frame as string).error).toBe("workspace routing discovery unauthorized (401)");

    // The next request goes nowhere: the only computer's CLI is signed out (before any heartbeat).
    const assigns = d.frames.filter((f) => f.t === "run.assign").length;
    const { c: oc } = await Client.connect(w.owner.token);
    const res = await oc.ok({ t: "send", groupId: w.gid, clientMsgId: crypto.randomUUID(), body: "@dev again", mentions: [w.bot.id] });
    const topic2 = res.message.topicId as string;
    const note = await waitUntil(() => botMessages(oc, topic2, w.bot.id)[0], 5000, "signed-out note");
    // Joye is an admin: pointed to the sign-in herself.
    expect(note.body).toBe(TEXT.notStartedWith(TEXT.failure.signedOut("mac-mini", "Codex"), TEXT.next.signInSelf(["mac-mini"])));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(assigns);

    // A heartbeat that says signed in again lets work through.
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: { at: Date.now(), runs: [], logins: [{ runtime: "codex", state: "logged_in", method: "subscription" }] },
    });
    await new Promise((r) => setTimeout(r, 100));
    await oc.ok({ t: "send", groupId: w.gid, clientMsgId: crypto.randomUUID(), body: "@dev third", mentions: [w.bot.id] });
    await d.next((f) => f.t === "run.assign" && d.frames.filter((x) => x.t === "run.assign").length > assigns);
    c.close();
    oc.close();
    d.close();
  });

  it("say when a limit resets, and to change the model", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const resetsAt = Date.now() + 2 * 3600_000;
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await d.acked(runId, d.done(runId, { status: "failed", error: "You've hit your usage limit.", failure: { kind: "limit", resetsAt } }));
    const ended = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "failed");
    expect(ended.data.run.failure).toEqual({ kind: "limit", resetsAt });
    const [msg] = botMessages(c, topicId, w.bot.id);
    expect(msg.body).toBe(TEXT.failedWith(TEXT.failure.limit("Codex"), TEXT.next.limitAt(clockText(resetsAt, Date.now(), "Asia/Shanghai"))));

    const topic2 = await mention(c, w, "@dev other");
    const r2 = await d.next((f) => f.t === "run.assign" && f.runId !== runId);
    d.send({ t: "run.accept", runId: r2.runId });
    // An older daemon: no kind, the #103 JSON body still reads as the model.
    await d.acked(
      r2.runId,
      d.done(r2.runId, { status: "failed", error: '{"error":{"message":"The model x is not supported","code":"model_not_found"}}' }),
    );
    await waitUntil(() => botMessages(c, topic2, w.bot.id)[0], 5000, "model note");
    expect(botMessages(c, topic2, w.bot.id)[0].body).toBe(TEXT.failedWith(TEXT.failure.model("Codex"), TEXT.next.model));
    c.close();
    d.close();
  });
});
