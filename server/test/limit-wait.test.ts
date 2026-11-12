// 模 3.4, 3.3; 盯 1.5 (usable work line 4, acceptance path 7; decided: wait for the reset): a run
// that stops at the subscription limit waits until the limit resets and then goes on in its CLI
// session on the same computer; new work for that computer's runtime waits the same way unless
// another computer can take it; a person can stop a waiting run.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { LIMIT_NOTE, LIMIT_REACHED, MAX_LIMIT_WAITS } from "../src/agent/recovery";
import { LIMIT_SLACK_MS, type MachineView, TEXT, candidates, clockText, limitOf, whyNoMachine } from "../src/bots/dispatch";
import type { BotsEnv } from "../src/bots/env";
import { LIMIT_BACKOFF_MAX_MS, LIMIT_BACKOFF_MS, limitMark, limitedRuntimes } from "../src/bots/link";
import { Daemon, HEALTH, downViolations, mention, world } from "./daemon";
import { Client, api, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const linkOf = (machineId: string) => benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId));
const ZONE = "Asia/Shanghai";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

/** The limit of the machine's runtimes is over now (as if the reset time came). */
async function resetLimit(machineId: string): Promise<void> {
  await runInDurableObject(linkOf(machineId), (_o, state: DurableObjectState) => {
    const row = state.storage.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'limited'").toArray()[0];
    const marks = JSON.parse(row?.value ?? "{}") as Record<string, { until: number }>;
    for (const m of Object.values(marks)) m.until = Date.now() - 1;
    state.storage.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('limited', ?)", JSON.stringify(marks));
  });
}

describe("limit wait: pure rules", () => {
  const now = Date.UTC(2026, 10, 12, 4, 0);

  it("wait until the CLI's reset, else the full usage windows' end, else back off", () => {
    expect(limitMark(now + 3_600_000, undefined, 0, undefined, now)).toEqual({ until: now + 3_600_000 + LIMIT_SLACK_MS, known: true, tries: 0 });
    // No reset from the CLI: the fullest windows the daemon read (machine clock 5 s behind).
    const windows = [
      { usedPct: 100, resetsAt: now + 2 * 3_600_000 },
      { usedPct: 97, resetsAt: now + 5 * 3_600_000 },
      { usedPct: 40, resetsAt: now + 90 * 3_600_000 },
    ];
    expect(limitMark(undefined, windows, 5000, undefined, now)).toEqual({ until: now + 5 * 3_600_000 + 5000 + LIMIT_SLACK_MS, known: true, tries: 0 });
    // A reset in the past, or none full: a guess, twice as long each time in a row, at most 2 h.
    const a = limitMark(now - 1, [{ usedPct: 60, resetsAt: now + 3_600_000 }], 0, undefined, now);
    expect(a).toEqual({ until: now + LIMIT_BACKOFF_MS, known: false, tries: 1 });
    const b = limitMark(undefined, undefined, 0, a, a.until + 1000);
    expect(b).toEqual({ until: a.until + 1000 + 2 * LIMIT_BACKOFF_MS, known: false, tries: 2 });
    const c = limitMark(undefined, undefined, 0, { until: now, known: false, tries: 6 }, now);
    expect(c.until - now).toBe(LIMIT_BACKOFF_MAX_MS);
    // A guess long ago starts over.
    expect(limitMark(undefined, undefined, 0, { until: now - LIMIT_BACKOFF_MAX_MS - 1, known: false, tries: 4 }, now).tries).toBe(1);
    expect(limitedRuntimes({ codex: { until: now + 1, known: true, tries: 0 }, claude: { until: now, known: false, tries: 1 } }, now)).toEqual([
      { runtime: "codex", until: now + 1, known: true },
    ]);
  });

  it("route no run to a runtime at its limit, and wait for the earliest reset when none is left", () => {
    const m = (id: string, limited?: { until: number; known: boolean }): MachineView => ({
      id,
      name: id,
      labels: ["mac"],
      revoked: false,
      connected: true,
      lastHeartbeatAt: now - 1000,
      info: { name: id, labels: [], daemonVersion: "1", os: "macos", arch: "arm64", maxParallelRuns: 2 },
      health: HEALTH,
      clis: [{ runtime: "codex", kind: "codex", command: "codex", available: true }],
      runningRuns: [],
      ...(limited ? { limited: [{ runtime: "codex", ...limited }] } : {}),
    });
    const runner = { runtime: "codex", labels: ["mac"] };
    expect(limitOf(m("a", { until: now + 10, known: true }), "codex", now)).toEqual({ until: now + 10, known: true });
    expect(limitOf(m("a", { until: now, known: true }), "codex", now)).toBeNull();
    expect(limitOf(m("a", { until: now + 10, known: true }), "claude", now)).toBeNull();
    // Another computer free of the limit still takes it.
    expect(candidates([m("a", { until: now + 10, known: true }), m("b")], runner, now).map((x) => x.id)).toEqual(["b"]);
    expect(whyNoMachine([m("a", { until: now + 10, known: true }), { ...m("b"), runningRuns: ["r1", "r2"] }], runner, now).why).toBe("busy");
    const none = whyNoMachine([m("a", { until: now + 500, known: true }), m("b", { until: now + 100, known: false })], runner, now);
    expect(none).toMatchObject({ why: "limit", until: now + 100, known: false });
  });

  it("say until when on the team's clock, or when it tries again", () => {
    const when = clockText(now + 3 * 3_600_000, now, ZONE);
    expect(TEXT.waitLimit("Claude Code", when, true)).toBe("Claude Code 的订阅额度用完了，今天 15:00 恢复后接着做；不需要了可以停止。");
    expect(TEXT.waitLimit("Codex", when, false)).toBe("Codex 的订阅额度用完了，今天 15:00 恢复后开始；不需要了可以停止。");
    expect(TEXT.waitLimitRetry("Codex", 15)).toBe("Codex 的订阅额度用完了，不知道几点恢复，15 分钟后再试；不需要了可以停止。");
    for (const t of [TEXT.waitLimit("Codex", when, true), TEXT.waitLimitRetry("Codex", 15), TEXT.limitBack(true), TEXT.limitBack(false)]) {
      expect(t).not.toMatch(/CLI|session|token|runtime|会话|令牌/i);
    }
  });
});

describe("limit wait in the topic", () => {
  it("wait for the reset, queue new work for that computer, and go on in the same session", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const resetsAt = Date.now() + 2 * 3_600_000;
    const topicId = await mention(c, w);
    const first = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: first.runId });
    d.event(first.runId, { type: "session", sessionId: "thread-1", runtime: "codex" });
    await d.acked(first.runId, d.done(first.runId, { status: "failed", error: "You've hit your usage limit.", sessionId: "thread-1", failure: { kind: "limit", resetsAt } }));

    // The run does not fail: it ends quietly and its work waits in a new run.
    const old = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.endedAt !== undefined)).data.run;
    expect(old).toMatchObject({ status: "cancelled", summary: `restarted: ${LIMIT_REACHED}`, failure: { kind: "limit", resetsAt } });
    const waiting = (
      await c.waitEvent((e) => e.type === "run.updated" && e.data.run.retryOf === first.runId && e.data.run.waitingFor === "limit")
    ).data.run;
    expect(waiting).toMatchObject({ status: "created", phase: "waiting_computer", waitingFor: "limit", topicId });
    expect(waiting.queuedAt).toBeGreaterThan(0);
    const note = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body.includes("额度")), 5000, "limit note");
    expect(note.body).toBe(TEXT.waitLimit("Codex", clockText(resetsAt, Date.now(), ZONE), true));
    // No failure note.
    expect(botMessages(c, topicId, w.bot.id).some((m) => m.body.startsWith("没有完成"))).toBe(false);

    // New work for this computer waits the same way instead of failing.
    const t2 = await mention(c, w, "@dev 另一件事");
    const q2 = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === t2 && e.data.run.waitingFor === "limit")).data.run;
    const note2 = await waitUntil(() => botMessages(c, t2, w.bot.id).find((m) => m.body.includes("额度")), 5000, "queued limit note");
    expect(note2.body).toBe(TEXT.waitLimit("Codex", clockText(resetsAt, Date.now(), ZONE), false));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    // The desk's alarm before the reset changes nothing.
    await runDurableObjectAlarm(deskOf(w.bot.id));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);

    // At the reset: the waiting work continues the session with the short note; the new work starts.
    await resetLimit(w.machineId);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const cont = await d.next((f) => f.t === "run.assign" && f.runId === waiting.id);
    expect(cont).toMatchObject({ topicId, resumeSessionId: "thread-1" });
    expect(cont.prompt).toContain(LIMIT_NOTE);
    expect(cont.prompt).not.toContain("Recovery note");
    await d.next((f) => f.t === "run.assign" && f.runId === q2.id);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body === TEXT.limitBack(true)), 5000, "back note");
    await waitUntil(() => botMessages(c, t2, w.bot.id).find((m) => m.body === TEXT.limitBack(false)), 5000, "back note 2");
    const placed = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === waiting.id && e.data.run.machineId !== undefined)).data.run;
    expect(placed.waitingFor).toBeUndefined();

    // It goes on and ends well; a success clears the computer's limit.
    d.send({ t: "run.accept", runId: cont.runId });
    await d.acked(cont.runId, d.done(cont.runId, { status: "succeeded", resultText: "做完了" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === cont.runId && e.data.run.status === "done");
    c.close();
    d.close();
  }, 40_000);

  it("without a reset time use the usage windows, else try again later and say so", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    // The daemon's usage: the week window is full.
    const weekEnd = Date.now() + 30 * 3_600_000;
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: { at: Date.now(), runs: [], usage: [{ runtime: "codex", at: Date.now(), windows: [{ kind: "secondary", usedPct: 100, resetsAt: weekEnd }] }] },
    });
    await new Promise((r) => setTimeout(r, 100));
    const t1 = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "failed", error: "usage limit", sessionId: "s-a", failure: { kind: "limit" } }));
    const n1 = await waitUntil(() => botMessages(c, t1, w.bot.id).find((m) => m.body.includes("额度")), 5000, "window note");
    // The machine clock and the server's are about the same here; the minute's slack is shown off.
    expect(n1.body).toBe(TEXT.waitLimit("Codex", clockText(weekEnd, Date.now(), ZONE), true));

    // Nothing known at all (a fresh machine): a guess, and the topic says when it tries again.
    const w2 = await world();
    const { d: d2 } = await Daemon.connect(w2.machineToken, { features: ["resume"] });
    const { c: c2 } = await Client.connect(w2.human.token);
    const t2 = await mention(c2, w2);
    const b = await d2.next((f) => f.t === "run.assign");
    d2.send({ t: "run.accept", runId: b.runId });
    await d2.acked(b.runId, d2.done(b.runId, { status: "failed", error: "usage limit", sessionId: "s-b", failure: { kind: "limit" } }));
    const n2 = await waitUntil(() => botMessages(c2, t2, w2.bot.id).find((m) => m.body.includes("额度")), 5000, "retry note");
    expect(n2.body).toBe(TEXT.waitLimitRetry("Codex", LIMIT_BACKOFF_MS / 60_000));
    // The try after the guess hits the limit again: the next wait is twice as long.
    await resetLimit(w2.machineId);
    await runDurableObjectAlarm(deskOf(w2.bot.id));
    const b2 = await d2.next((f) => f.t === "run.assign" && f.runId !== b.runId);
    expect(b2.resumeSessionId).toBe("s-b");
    d2.send({ t: "run.accept", runId: b2.runId });
    await d2.acked(b2.runId, d2.done(b2.runId, { status: "failed", error: "usage limit", sessionId: "s-b", failure: { kind: "limit" } }));
    await waitUntil(
      () => botMessages(c2, t2, w2.bot.id).find((m) => m.body === TEXT.waitLimitRetry("Codex", (2 * LIMIT_BACKOFF_MS) / 60_000)),
      5000,
      "second retry note",
    );
    c.close();
    d.close();
    c2.close();
    d2.close();
  }, 40_000);

  it("let a person stop a run while it waits, and send new work to another computer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const first = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: first.runId });
    await d.acked(first.runId, d.done(first.runId, { status: "failed", error: "limit", failure: { kind: "limit", resetsAt: Date.now() + 3_600_000 } }));
    const waiting = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.retryOf === first.runId && e.data.run.waitingFor === "limit")).data.run;

    // A second computer with the same labels takes new work at once.
    const runner = await api(`/v1/bots/${w.bot.id}/runner`, { method: "GET", token: w.owner.token });
    const m2 = await api("/v1/machines", { token: w.owner.token, body: { name: "linux-box", labels: runner.body.labels } });
    expect(m2.status).toBe(200);
    const { d: d2 } = await Daemon.connect(m2.body.token);
    await mention(c, w, "@dev 新的事");
    const other = await d2.next((f) => f.t === "run.assign");
    expect(other.runId).not.toBe(waiting.id);
    // The waiting work stays with its own computer (its session is there).
    expect(d2.frames.some((f) => f.t === "run.assign" && f.runId === waiting.id)).toBe(false);

    // 停止 while it waits.
    const stop = await api(`/v1/runs/${waiting.id}/cancel`, { token: w.human.token, body: {} });
    expect(stop.status, JSON.stringify(stop.body)).toBe(200);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === waiting.id && e.data.run.status === "cancelled");
    await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body === TEXT.cancelled), 5000, "stop note");
    await resetLimit(w.machineId);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.some((f) => f.t === "run.assign" && f.runId === waiting.id)).toBe(false);
    c.close();
    d.close();
    d2.close();
  }, 40_000);

  it("fail with the limit note after too many waits in a row", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const first = await d.next((f) => f.t === "run.assign");
    // The work already waited for the limit MAX_LIMIT_WAITS times in a row.
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      let prev: string | null = null;
      for (let i = 0; i < MAX_LIMIT_WAITS; i++) {
        const id = `01OLDLIMIT${String(i).padStart(16, "0")}`;
        state.storage.sql.exec(
          `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at, ended_at, summary, retry_of)
           VALUES (?, ?, ?, 'cancelled', ?, ?, 1, 1, 1, 1, ?, ?)`,
          id,
          w.gid,
          topicId,
          w.human.id,
          `old:${i}`,
          `restarted: ${LIMIT_REACHED}`,
          prev,
        );
        prev = id;
      }
      state.storage.sql.exec("UPDATE runs SET retry_of = ? WHERE id = ?", prev, first.runId);
    });
    const resetsAt = Date.now() + 3_600_000;
    d.send({ t: "run.accept", runId: first.runId });
    await d.acked(first.runId, d.done(first.runId, { status: "failed", error: "limit", failure: { kind: "limit", resetsAt } }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "failed");
    const msg = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body.startsWith("没有完成")), 5000, "failure note");
    expect(msg.body).toBe(TEXT.failedWith(TEXT.failure.limit("Codex"), TEXT.next.limitAt(clockText(resetsAt, Date.now(), ZONE))));
    c.close();
    d.close();
  }, 40_000);
});
