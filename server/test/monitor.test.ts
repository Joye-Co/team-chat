// The owner's machine monitor: heartbeat metrics stored by MachineLink, history series, and the
// per-member rollup of runs (docs/protocol.md "Machine monitor", machine-protocol.md "Metrics").
import { MachineMonitor } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { buildMonitor, canTake, reserveBytes } from "../src/monitor";
import { Daemon, HEALTH, mention, world } from "./daemon";
import { Client, api } from "./helpers";

const GIB = 1024 ** 3;

function metrics(runs: { runId: string; phase: string; memBytes?: number; cpuCores?: number }[], over: Record<string, unknown> = {}) {
  return {
    at: Date.now(),
    uptimeSecs: 10800,
    cpuPct: 62.4,
    cpuThreads: 12,
    memTotalBytes: 64 * GIB,
    memAvailableBytes: 30 * GIB,
    netRxBps: 3_500_000,
    netTxBps: 1_100_000,
    vmSlots: 12,
    boot: { avgMs: 1300, maxMs: 2100, samples: 4 },
    runs: runs.map((r) => ({ startedAt: Date.now() - 60_000, ...r })),
    logins: [{ runtime: "codex", state: "logged_out" }],
    ...over,
  };
}

async function monitor(token: string, machineId: string, query = "range=15m") {
  return api(`/v1/machines/${machineId}/monitor?${query}`, { token });
}

describe("machine monitor", () => {
  it("is for owners only, and says not_found for an unknown machine", async () => {
    const w = await world();
    const r = await monitor(w.human.token, w.machineId);
    expect(r.status).toBe(403);
    expect((await monitor(w.owner.token, "01JBX0NOPE0000000000000000")).status).toBe(404);
    expect((await monitor(w.owner.token, w.machineId, "range=7d")).status).toBe(400);
  });

  it("answers before the daemon ever connected, with empty numbers", async () => {
    const w = await world();
    const r = await monitor(w.owner.token, w.machineId);
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const m = MachineMonitor.parse(r.body);
    expect(m.machine.connected).toBe(false);
    expect(m.sampledAt).toBeUndefined();
    expect(m.runs).toEqual([]);
    expect(m.series.cpuPct.every((v) => v === null)).toBe(true);
  });

  it("keeps heartbeat metrics, joins VMs to their member and counts today's runs", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    // world() gives the bot a label the machine must have.
    d.close();
    const runner = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: [], machineId: w.machineId } });
    expect(runner.status, JSON.stringify(runner.body)).toBe(200);
    const { d: d2 } = await Daemon.connect(w.machineToken, { labels: [] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const assign = await d2.next((f) => f.t === "run.assign");
    d2.send({ t: "run.accept", runId: assign.runId });
    await d2.acked(assign.runId, d2.event(assign.runId, { type: "status", state: "running" }));

    // An old-style heartbeat (no metrics) and one with metrics a newer daemon might send.
    d2.send({ t: "heartbeat", at: Date.now(), health: { ...HEALTH, activeRuns: 1 }, runs: [{ runId: assign.runId, lastSeq: 1, done: false }] });
    await d2.next((f) => f.t === "heartbeat.ack");
    d2.frames.length = 0;
    d2.send({
      t: "heartbeat",
      at: Date.now(),
      health: { ...HEALTH, activeRuns: 1 },
      runs: [{ runId: assign.runId, lastSeq: 1, done: false }],
      metrics: metrics([
        { runId: assign.runId, phase: "running", memBytes: 2 * GIB, cpuCores: 1.5 },
        { runId: "not-a-server-run", phase: "booting" },
      ]),
    });
    await d2.next((f) => f.t === "heartbeat.ack");

    const r = await monitor(w.owner.token, w.machineId, `range=15m&dayStart=${Date.now() - 3_600_000}`);
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const m = MachineMonitor.parse(r.body);
    expect(m.machine.connected).toBe(true);
    expect(m.machine.connectedAt).toBeGreaterThan(0);
    expect(m.sampledAt).toBeGreaterThan(0);
    expect(m.host).toMatchObject({ cpuPct: 62.4, memTotalBytes: 64 * GIB, memVmBytes: 2 * GIB, netRxBps: 3_500_000 });
    expect(m.boot).toEqual({ avgMs: 1300, maxMs: 2100, samples: 4 });
    const run = m.runs.find((x) => x.runId === assign.runId);
    expect(run).toMatchObject({ memberId: w.bot.id, state: "running", memBytes: 2 * GIB, cpuCores: 1.5 });
    expect(m.runs.find((x) => x.runId === "not-a-server-run")).toMatchObject({ state: "booting" });
    expect(m.capacity).toMatchObject({ slots: 12, running: 2 });
    expect(m.clis).toEqual([{ runtime: "codex", kind: "codex", available: true, version: "0.159.0", login: "logged_out", failedToday: 0 }]);
    expect(m.members).toEqual([
      { memberId: w.bot.id, running: 1, waiting: 0, queued: 0, cpuCores: 1.5, memBytes: 2 * GIB, tokensToday: 0, finishedToday: 0 },
    ]);
    expect(m.series.cpuPct.filter((v) => v !== null)).toEqual([62.4]);

    // The run ends: it counts for today with its tokens.
    const done = d2.done(assign.runId, { status: "succeeded", usage: { inputTokens: 1200, outputTokens: 300 }, resultText: "ok" });
    await d2.acked(assign.runId, done);
    let after = MachineMonitor.parse((await monitor(w.owner.token, w.machineId, `range=1h&dayStart=${Date.now() - 3_600_000}`)).body);
    for (let i = 0; i < 50 && after.today.finished !== 1; i++) {
      await new Promise((r) => setTimeout(r, 100));
      after = MachineMonitor.parse((await monitor(w.owner.token, w.machineId, `range=1h&dayStart=${Date.now() - 3_600_000}`)).body);
    }
    expect(after.today).toMatchObject({ finished: 1, succeeded: 1, failed: 0, tokens: 1500 });
    expect(after.members[0]).toMatchObject({ memberId: w.bot.id, tokensToday: 1500, finishedToday: 1 });
    expect(after.series.range).toBe("1h");
    expect(after.series.cpuPct).toHaveLength(60);
    c.close();
    d2.close();
  });

  it("drops metrics it cannot read but still takes the heartbeat", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    d.send({ t: "heartbeat", at: Date.now(), health: HEALTH, runs: [], metrics: { at: "soon", runs: "many" } });
    await d.next((f) => f.t === "heartbeat.ack");
    const m = MachineMonitor.parse((await monitor(w.owner.token, w.machineId)).body);
    expect(m.machine.connected).toBe(true);
    expect(m.sampledAt).toBeUndefined();
    expect(m.host).toMatchObject({ load1: HEALTH.load1, diskTotalBytes: HEALTH.diskTotalBytes });
    d.close();
  });
});

describe("monitor math", () => {
  it("keeps a fifth of memory free and estimates how many more VMs fit", () => {
    expect(reserveBytes(64 * GIB)).toBe(13 * GIB);
    expect(reserveBytes(4 * GIB)).toBe(2 * GIB);
    // 12 slots, 3 running: 9 free slots; 30 GiB free - 13 reserve = 17 GiB at 2 GiB = 8.
    expect(canTake(12, 3, 30 * GIB, 64 * GIB, 2 * GIB)).toBe(8);
    expect(canTake(12, 3)).toBe(9);
    expect(canTake(2, 5)).toBe(0);
    expect(canTake(12, 0, 5 * GIB, 64 * GIB, 2 * GIB)).toBe(0);
  });

  it("shows waiting and paused runs, falls back to the server's runs without metrics, and finds the last failure", () => {
    const rollup = (over: Record<string, unknown>) => ({
      active: [],
      queued: [],
      finished: 0,
      succeeded: 0,
      failed: 0,
      finishedYesterday: 0,
      tokens: 0,
      failedByRuntime: {},
      lastFailure: null,
      ...over,
    });
    const m = buildMonitor({
      machine: { id: "01JBX0MACHINE00000000000AV", name: "runner-1", revoked: false },
      link: null,
      desks: [
        {
          botId: "01JBX0B0T0000000000000000A",
          rollup: rollup({
            active: [
              { runId: "r1", status: "waiting", startedAt: 1, runtime: "codex", paused: false },
              { runId: "r2", status: "running", startedAt: 2, runtime: "codex", paused: true },
            ],
            queued: [{ runId: "q1", queuedAt: 50 }],
            finished: 3,
            succeeded: 2,
            failed: 1,
            failedByRuntime: { codex: 1 },
            lastFailure: { runId: "r0", at: 100, groupId: "g", topicId: "t" },
            lastFailureTitle: "对一下十月的账单",
          }),
        },
        { botId: "01JBX0B0T0000000000000000B", rollup: rollup({ queued: [{ runId: "q2", queuedAt: 40 }] }) },
      ],
      now: 1000,
    });
    expect(m.runs.map((r) => [r.runId, r.state])).toEqual([
      ["r1", "waiting"],
      ["r2", "paused"],
      ["q1", "queued"],
      ["q2", "queued"],
    ]);
    expect(m.queue).toEqual({ count: 2, oldestAt: 40 });
    expect(m.today).toMatchObject({ finished: 3, succeeded: 2, failed: 1 });
    expect(m.lastFailure).toEqual({ runId: "r0", memberId: "01JBX0B0T0000000000000000A", at: 100, title: "对一下十月的账单" });
    expect(m.members.map((x) => [x.memberId, x.running, x.waiting, x.queued])).toEqual([
      ["01JBX0B0T0000000000000000A", 1, 1, 1],
      ["01JBX0B0T0000000000000000B", 0, 0, 1],
    ]);
    expect(MachineMonitor.safeParse(m).success).toBe(true);
  });
});

describe("sign in and API keys from the app (feature accounts)", () => {
  const KEY = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

  it("relays a sign-in: start, the CLI's link, the pasted code, done; owners only", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["accounts"], publicKey: KEY });
    expect((await api(`/v1/machines/${w.machineId}/logins`, { token: w.human.token, body: { runtime: "codex" } })).status).toBe(403);
    const start = await api(`/v1/machines/${w.machineId}/logins`, { token: w.owner.token, body: { runtime: "codex" } });
    expect(start.status, JSON.stringify(start.body)).toBe(200);
    const loginId = start.body.loginId as string;
    const f = await d.next((x) => x.t === "account.login");
    expect(f).toEqual({ t: "account.login", loginId, runtime: "codex", op: "start" });
    expect((await api(`/v1/machines/${w.machineId}/logins/${loginId}`, { token: w.owner.token })).body).toMatchObject({ state: "starting" });
    d.send({ t: "account.login.status", loginId, runtime: "codex", state: "device_code", url: "https://auth.openai.com/codex/device", userCode: "ABCD-12345" });
    let got: any;
    for (let i = 0; i < 30; i++) {
      got = (await api(`/v1/machines/${w.machineId}/logins/${loginId}`, { token: w.owner.token })).body;
      if (got.state === "device_code") break;
      await new Promise((r) => setTimeout(r, 50));
    }
    expect(got).toMatchObject({ loginId, runtime: "codex", state: "device_code", userCode: "ABCD-12345" });
    expect(got.updatedAt).toBeGreaterThan(0);
    const code = await api(`/v1/machines/${w.machineId}/logins/${loginId}/code`, { token: w.owner.token, body: { code: "abc#def" } });
    expect(code.status).toBe(200);
    expect(await d.next((x) => x.t === "account.login" && x.op === "code")).toMatchObject({ code: "abc#def", runtime: "codex" });
    expect((await api(`/v1/machines/${w.machineId}/logins/${loginId}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    expect(await d.next((x) => x.t === "account.login" && x.op === "cancel")).toMatchObject({ loginId });
    expect((await api(`/v1/machines/${w.machineId}/logins/nope`, { token: w.owner.token })).status).toBe(404);
    d.close();
  });

  it("relays a sealed key and its removal, and lists only names and last 4 from the heartbeat", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["accounts"], publicKey: KEY });
    const put = await api(`/v1/machines/${w.machineId}/secrets/OPENAI_API_KEY`, {
      method: "PUT",
      token: w.owner.token,
      body: { sealed: "c2VhbGVk", members: [w.bot.id] },
    });
    expect(put.status, JSON.stringify(put.body)).toBe(200);
    expect(await d.next((x) => x.t === "secret.set")).toEqual({ t: "secret.set", name: "OPENAI_API_KEY", sealed: "c2VhbGVk", members: [w.bot.id] });
    expect((await api(`/v1/machines/${w.machineId}/secrets/lower`, { method: "PUT", token: w.owner.token, body: { sealed: "x" } })).status).toBe(400);
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: metrics([], { secrets: [{ name: "OPENAI_API_KEY", last4: "x9Qz", members: [w.bot.id], updatedAt: Date.now() }] }),
    });
    await d.next((x) => x.t === "heartbeat.ack");
    const list = await api(`/v1/machines/${w.machineId}/secrets`, { token: w.owner.token });
    expect(list.body.publicKey).toBe(KEY);
    expect(list.body.secrets).toEqual([expect.objectContaining({ name: "OPENAI_API_KEY", last4: "x9Qz" })]);
    expect(JSON.stringify(list.body)).not.toContain("c2VhbGVk");
    expect((await api(`/v1/machines/${w.machineId}/secrets/OPENAI_API_KEY`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    expect(await d.next((x) => x.t === "secret.delete")).toEqual({ t: "secret.delete", name: "OPENAI_API_KEY" });
    d.close();
  });

  it("says when the machine is offline or its daemon too old, and never sends those frames to it", async () => {
    const w = await world();
    const off = await api(`/v1/machines/${w.machineId}/logins`, { token: w.owner.token, body: { runtime: "claude" } });
    expect(off.status).toBe(409);
    const { d } = await Daemon.connect(w.machineToken);
    const old = await api(`/v1/machines/${w.machineId}/logins`, { token: w.owner.token, body: { runtime: "claude" } });
    expect(old.status).toBe(409);
    expect(JSON.stringify(old.body)).toContain("too old");
    expect((await api(`/v1/machines/${w.machineId}/secrets`, { token: w.owner.token })).body).toEqual({ secrets: [] });
    expect(d.frames.some((x) => x.t === "account.login" || x.t === "secret.set")).toBe(false);
    d.close();
  });
});
