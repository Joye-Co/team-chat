// 模 2.2, 2.3, 2.10, 3.1, 3.2, 3.5; 接 5.3; 盯 1.13; 机 2.5, 5.2: a failed run says what went wrong
// and what to do, a signed-out CLI gets no work, and the run keeps the model it really used.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import {
  type MachineView,
  TEXT,
  candidates,
  cliEntryName,
  clockText,
  failureKind,
  failureWords,
  whyNoMachine,
} from "../src/bots/dispatch";
import { SIGNED_OUT_MARK_MS, signedOutRuntimes } from "../src/bots/link";
import type { BotsEnv } from "../src/bots/env";
import { Daemon, HEALTH, downViolations, mention, world } from "./daemon";
import { Client, api, protocolViolations, waitUntil } from "./helpers";

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

const login = (runtime: string, state: string, confirmed?: boolean) => ({ runtime, state, ...(confirmed ? { confirmed } : {}) });

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
    const ctx = { machines: ["mac-mini"], cli: "Claude Code", now, zone: "Asia/Shanghai", signIn: { self: "admin" as const } };
    // A 负责人 signs in from 团队管理's 订阅账号, where the sign-in is.
    expect(failureWords("signed_out", ctx)).toEqual({
      sentence: "mac-mini 上的 Claude Code 没有登录，或者登录已经过期",
      next: "请到 团队管理 › 账号与密钥 › 订阅账号，选 mac-mini，点「重新登录」，然后再 @我。",
    });
    // The owner of a personal computer who cannot open 团队管理 signs in on the computer.
    expect(failureWords("signed_out", { ...ctx, signIn: { self: "machine" } }).next).toBe(
      "请在 mac-mini 上重新登录 Claude Code，或者请负责人在 团队管理 › 账号与密钥 › 订阅账号 里帮忙，然后再 @我。",
    );
    expect(failureWords("signed_out", { ...ctx, signIn: { names: ["Joye"] } }).next).toBe(TEXT.next.signInAsk(["Joye"]));
    expect(failureWords("signed_out", { ...ctx, signIn: { names: [] } }).next).toBe(TEXT.next.signInAdmin);
    expect(failureWords("limit", { ...ctx, resetsAt: now + 3 * 3600_000 }).next).toBe("额度在 今天 15:00 恢复，到时再 @我。");
    // A reset in the past, or none: no time.
    expect(failureWords("limit", { ...ctx, resetsAt: now - 1 }).next).toBe(TEXT.next.limitLater);
    expect(failureWords("model", ctx).next).toBe("请负责人到 设置 › 通用 › 模型 换一个模型，然后再 @我。");
    expect(failureWords("model", { ...ctx, ownModel: true }).next).toBe("请到 设置 › 成员 里我的 运行设置 › 模型 换一个模型，然后再 @我。");
    expect(failureWords("network", ctx).sentence).toContain("mac-mini");
    expect(failureWords("not_installed", ctx).sentence).toBe("mac-mini 上没有装 Claude Code");
    expect(failureWords("other", ctx)).toEqual({ sentence: TEXT.failure.other, next: TEXT.next.other });
    // No developer words reach the topic.
    for (const k of ["signed_out", "limit", "model", "network", "not_installed", "other"] as const) {
      const w = failureWords(k, ctx);
      expect(`${w.sentence}${w.next}`).not.toMatch(/CLI|session|token|runtime|会话|令牌/i);
    }
  });

  it("name a runtime entry that is not the kind's own, so two entries of one CLI are told apart", () => {
    expect(cliEntryName("codex", "codex")).toBe("Codex");
    expect(cliEntryName("codex", "codexout")).toBe("Codex（codexout）");
    expect(cliEntryName("claude", "claude")).toBe("Claude Code");
    expect(cliEntryName("acp", "gemini")).toBe("gemini");
    expect(cliEntryName(undefined, "codexout")).toBe("codexout");
  });

  it("take the runtime out while a heartbeat or a failed run says it is signed out", () => {
    expect(signedOutRuntimes([login("claude", "logged_out", true), login("codex", "logged_in")], 100, {}, 200)).toEqual(["claude"]);
    // The credential files alone (or an older daemon) do not know every way a CLI signs in.
    expect(signedOutRuntimes([login("claude", "logged_out")], 100, {}, 200)).toEqual([]);
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

  it("name the signed-out runtime entry when it is not the CLI's own (verify 2, problem 8)", async () => {
    const w = await world();
    const set = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codexout" } });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    const { d } = await Daemon.connect(w.machineToken, { runtime: "codexout" });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await d.acked(runId, d.done(runId, { status: "failed", error: "Not logged in", failure: { kind: "signed_out" } }));
    const msg = await waitUntil(() => botMessages(c, topicId, w.bot.id)[0], 5000, "signed-out note");
    expect(msg.body).toBe(TEXT.failedWith(TEXT.failure.signedOut("mac-mini", "Codex（codexout）"), TEXT.next.signInAsk(["Joye"])));
    c.close();
    d.close();
  });

  it("say to change the model (a limit waits for its reset: limit-wait.test.ts)", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topic2 = await mention(c, w, "@dev other");
    const r2 = await d.next((f) => f.t === "run.assign");
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
  it("end a queued run with the sign-in note when its computer's CLI is signed out while it waits", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 1 });
    const { c } = await Client.connect(w.human.token);
    const t1 = await mention(c, w, "@dev 第一件事");
    const first = await d.next((f) => f.t === "run.assign" && f.topicId === t1);
    d.send({ t: "run.accept", runId: first.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "running");
    const t2 = await mention(c, w, "@dev 第二件事");
    const q2 = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === t2 && e.data.run.queuePosition !== undefined)).data.run;
    await d.acked(first.runId, d.done(first.runId, { status: "failed", error: "Not logged in", failure: { kind: "signed_out" } }));
    const ended = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === q2.id && e.data.run.status === "failed", 20_000)).data.run;
    expect(ended.failure).toEqual({ kind: "signed_out" });
    const note = await waitUntil(() => botMessages(c, t2, w.bot.id)[0], 5000, "queued sign-in note");
    expect(note.body).toBe(TEXT.notStartedWith(TEXT.failure.signedOut("mac-mini", "Codex"), TEXT.next.signInAsk(["Joye"])));
    expect(note.mentions).toEqual([w.owner.id]);
    // It never went to the signed-out computer.
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    c.close();
    d.close();
  }, 40_000);

  it("still send work when only the credential files say signed out", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    // An older daemon, or one whose status check could not run: logged_out without `confirmed`.
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: { at: Date.now(), runs: [], logins: [{ runtime: "codex", state: "logged_out" }] },
    });
    await new Promise((r) => setTimeout(r, 100));
    const t = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign" && f.topicId === t);
    expect(a.runtime).toBe("codex");
    // The CLI's own check said so: no more work goes there.
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: { at: Date.now(), runs: [], logins: [{ runtime: "codex", state: "logged_out", confirmed: true }] },
    });
    await new Promise((r) => setTimeout(r, 100));
    const t2 = await mention(c, w, "@dev again");
    const note = await waitUntil(() => botMessages(c, t2, w.bot.id)[0], 5000, "signed-out note");
    expect(note.body).toBe(TEXT.notStartedWith(TEXT.failure.signedOut("mac-mini", "Codex"), TEXT.next.signInAsk(["Joye"])));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    c.close();
    d.close();
  });
});
