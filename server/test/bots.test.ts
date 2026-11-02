import { SELF, env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { MachineDownFrame } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import {
  type MachineView,
  MIN_FREE_DISK_BYTES,
  Progress,
  STALL_MS,
  TEXT,
  approvalDecision,
  candidates,
  isHealthy,
  labelsMatch,
} from "../src/bots/dispatch";
import type { BotsEnv } from "../src/bots/env";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;
const downViolations: unknown[] = [];

afterAll(() => {
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

// ---------------------------------------------------------------------------
// A fake machine daemon speaking docs/machine-protocol.md
// ---------------------------------------------------------------------------

const HEALTH = { diskFreeBytes: 100e9, diskTotalBytes: 500e9, load1: 0.5, load5: 0.5, load15: 0.5, cpuCount: 8, activeRuns: 0 };

class Daemon {
  frames: any[] = [];
  closed: { code: number } | null = null;
  seq = new Map<string, number>();
  constructor(readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      const f = JSON.parse(e.data as string);
      const check = MachineDownFrame.safeParse(f);
      if (!check.success) downViolations.push({ frame: f, issues: check.error.issues });
      this.frames.push(f);
    });
    ws.addEventListener("close", (e) => {
      this.closed = { code: e.code };
    });
  }

  static async connect(
    token: string,
    opts: { labels?: string[]; health?: Partial<typeof HEALTH>; runs?: { runId: string; lastSeq: number; done: boolean }[]; runtime?: string } = {},
  ): Promise<{ d: Daemon; welcome: any }> {
    const res = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${token}` } });
    if (!res.webSocket) throw new Error(`upgrade failed: ${res.status}`);
    res.webSocket.accept();
    const d = new Daemon(res.webSocket);
    d.send({
      t: "hello",
      protocol: 1,
      machine: { name: "mac-mini", labels: opts.labels ?? ["mac"], daemonVersion: "0.1.0", os: "macos", arch: "aarch64", maxParallelRuns: 2 },
      clis: [{ runtime: opts.runtime ?? "codex", kind: "codex", command: "codex", version: "0.159.0", available: true }],
      health: { ...HEALTH, ...opts.health },
      runs: opts.runs ?? [],
    });
    const welcome = await d.next((f) => f.t === "welcome" || f.t === "error");
    return { d, welcome };
  }

  send(f: unknown): void {
    this.ws.send(JSON.stringify(f));
  }

  async next(pred: (f: any) => boolean, ms = 5000): Promise<any> {
    return waitUntil(() => this.frames.find(pred), ms, "daemon frame");
  }

  event(runId: string, event: unknown): number {
    const seq = (this.seq.get(runId) ?? 0) + 1;
    this.seq.set(runId, seq);
    this.send({ t: "run.event", runId, seq, at: Date.now(), event });
    return seq;
  }

  done(runId: string, extra: Record<string, unknown>): number {
    const seq = (this.seq.get(runId) ?? 0) + 1;
    this.seq.set(runId, seq);
    this.send({ t: "run.done", runId, seq, at: Date.now(), durationMs: 1000, ...extra });
    return seq;
  }

  async acked(runId: string, seq: number): Promise<void> {
    await this.next((f) => f.t === "ack" && f.runId === runId && f.seq >= seq);
  }

  close(): void {
    try {
      this.ws.close(1000, "done");
    } catch {
      // ignore
    }
  }
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

interface World {
  owner: TestMember;
  human: TestMember;
  bot: TestMember;
  gid: string;
  machineToken: string;
  machineId: string;
}

/** Every world gets its own machine label, so runs never go to another test's machine. */
async function world(opts: { runtime?: string; labels?: string[] } = {}): Promise<World> {
  const label = `t-${crypto.randomUUID().slice(0, 8)}`;
  const owner = await createMember({ level: "owner", name: "Joye" });
  const human = await createMember({ name: "Alice" });
  const bot = await createMember({ kind: "bot", name: "dev" });
  const gid = await createGroup(owner, [human.id, bot.id]);
  const r = await api(`/v1/bots/${bot.id}/runner`, {
    method: "PUT",
    token: owner.token,
    body: { runtime: opts.runtime ?? "codex", labels: opts.labels ?? [label], repo: { path: "/tmp/repo" }, baseBranch: "main" },
  });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const m = await api("/v1/machines", { token: owner.token, body: { name: "mac-mini", labels: [label] } });
  expect(m.status).toBe(200);
  return { owner, human, bot, gid, machineToken: m.body.token, machineId: m.body.machine.id };
}

/** Human starts a topic that mentions the bot; returns the topic id. */
async function mention(c: Client, w: World, body = "@dev please fix tests"): Promise<string> {
  const res = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body, mentions: [w.bot.id] });
  return res.message.topicId as string;
}

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

function topicStatuses(c: Client, topicId: string): string[] {
  return c
    .events()
    .filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status)
    .map((e) => e.data.status);
}

function deskOf(botId: string) {
  return benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
}
function linkOf(machineId: string) {
  return benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId));
}

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

describe("dispatch rules", () => {
  const now = 1_000_000_000;
  const base: MachineView = {
    id: "m1",
    name: "a",
    labels: ["mac", "main"],
    revoked: false,
    connected: true,
    lastHeartbeatAt: now - 10_000,
    info: { name: "a", labels: [], daemonVersion: "1", os: "macos", arch: "arm64", maxParallelRuns: 2 },
    health: HEALTH,
    clis: [{ runtime: "codex", kind: "codex", command: "codex", available: true }],
    runningRuns: [],
  };
  const runner = { runtime: "codex", labels: ["mac"] };

  it("match labels as a subset", () => {
    expect(labelsMatch([], ["x"])).toBe(true);
    expect(labelsMatch(["mac"], ["mac", "main"])).toBe(true);
    expect(labelsMatch(["mac", "gpu"], ["mac", "main"])).toBe(false);
  });

  it("gate on health: connection, heartbeat age and free disk", () => {
    expect(isHealthy(base, now)).toBe(true);
    expect(isHealthy({ ...base, connected: false }, now)).toBe(false);
    expect(isHealthy({ ...base, lastHeartbeatAt: now - 91_000 }, now)).toBe(false);
    expect(isHealthy({ ...base, health: { ...HEALTH, diskFreeBytes: MIN_FREE_DISK_BYTES - 1 } }, now)).toBe(false);
    expect(isHealthy({ ...base, revoked: true }, now)).toBe(false);
  });

  it("pick a healthy machine with the labels and runtime, least busy first", () => {
    const busy = { ...base, id: "m2", runningRuns: ["r1"] };
    const idle = { ...base, id: "m3" };
    const noLabel = { ...base, id: "m4", labels: ["linux"] };
    const noRuntime = { ...base, id: "m5", clis: [{ runtime: "claude", kind: "claude" as const, command: "claude", available: true }] };
    const unavailable = { ...base, id: "m6", clis: [{ runtime: "codex", kind: "codex" as const, command: "codex", available: false }] };
    const full = { ...base, id: "m7", runningRuns: ["a", "b"] };
    const lowDisk = { ...base, id: "m8", health: { ...HEALTH, diskFreeBytes: 1e9 } };
    const list = candidates([busy, noLabel, noRuntime, unavailable, full, lowDisk, idle], runner, now);
    expect(list.map((m) => m.id)).toEqual(["m3", "m2"]);
    expect(candidates([idle], runner, now, ["m3"])).toEqual([]);
  });

  it("read approval answers", () => {
    expect(approvalDecision("同意")).toBe("approve");
    expect(approvalDecision("@dev 同意。")).toBe("approve");
    expect(approvalDecision("拒绝")).toBe("deny");
    expect(approvalDecision("同意，但是先改 README")).toBe(null);
  });

  it("stream progress without repeating the final text of an item", () => {
    const p = new Progress("", {}, false);
    p.delta("i1", "Hel");
    p.delta("i1", "lo");
    expect(p.take()).toBe("Hello");
    p.message("i1", "Hello world");
    p.tool("cargo test");
    expect(p.take()).toBe(" world\n\n· cargo test\n");
    p.message("i2", "gap filled");
    expect(p.text).toBe("Hello world\n\n· cargo test\ngap filled\n\n");
  });
});

// ---------------------------------------------------------------------------
// A bot member on the normal WebSocket protocol
// ---------------------------------------------------------------------------

describe("bot member over the WebSocket protocol", () => {
  it("reads, sends and streams with its bot token", async () => {
    const owner = await createMember({ level: "owner" });
    const created = await api("/v1/bots", { token: owner.token, body: { name: "helper" } });
    expect(created.status).toBe(200);
    const botToken = created.body.token as string;
    const botId = created.body.member.id as string;
    const gid = await createGroup(owner, [botId]);
    const { c: human } = await Client.connect(owner.token);
    const { c: bot, welcome } = await Client.connect(botToken);
    expect(welcome.me).toMatchObject({ id: botId, kind: "bot" });
    expect(welcome.groups.map((g: any) => g.group.id)).toContain(gid);

    const ask = await human.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@helper hi", mentions: [botId] });
    const topicId = ask.message.topicId;
    await bot.waitEvent((e) => e.type === "message.created" && e.data.message.id === ask.message.id);
    const page = await bot.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.some((e: any) => e.type === "message.created")).toBe(true);
    const hist = await bot.request({ t: "history", groupId: gid, topicId });
    expect(hist.messages[0].id).toBe(ask.message.id);

    const reply = await bot.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "", streaming: true });
    bot.send({ t: "delta", groupId: gid, messageId: reply.message.id, append: "Hel" });
    bot.send({ t: "delta", groupId: gid, messageId: reply.message.id, append: "lo" });
    await human.next((f) => f.t === "transient" && f.event.type === "message.delta" && f.event.messageId === reply.message.id);
    await bot.ok({ t: "edit", groupId: gid, messageId: reply.message.id, body: "Hello", streaming: false });
    const upd = await human.waitEvent((e) => e.type === "message.updated" && e.data.messageId === reply.message.id);
    expect(upd.data).toMatchObject({ body: "Hello", streaming: false });
    await bot.ok({ t: "topic.update", groupId: gid, topicId, status: "done", ownerId: botId });
    human.close();
    bot.close();
  });
});

// ---------------------------------------------------------------------------
// Machines
// ---------------------------------------------------------------------------

describe("machines", () => {
  it("make a token, upgrade the daemon, report health, revoke", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    expect((await api("/v1/machines", { token: member.token, body: { name: "x" } })).status).toBe(403);
    const m = await api("/v1/machines", { token: owner.token, body: { name: "mac-mini", labels: ["main"] } });
    expect(m.body.token).toMatch(/^tm_[A-Za-z0-9_-]{43}$/);
    expect(m.body.machine).toMatchObject({ connected: false, healthy: false, labels: ["main"], runningRuns: [] });

    const bad = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: "Bearer tm_nope" } });
    expect(bad.status).toBe(401);
    expect(bad.webSocket).toBeFalsy();
    // A member token is not a machine token.
    const memberToken = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${owner.token}` } });
    expect(memberToken.status).toBe(401);

    const { d, welcome } = await Daemon.connect(m.body.token, { labels: ["mac"] });
    expect(welcome).toMatchObject({ t: "welcome", protocol: 1, machineId: m.body.machine.id, runs: [] });
    d.send({ t: "heartbeat", at: Date.now(), health: { ...HEALTH, diskFreeBytes: 42e9 }, runs: [] });
    await d.next((f) => f.t === "heartbeat.ack");
    const list = await api("/v1/machines", { token: member.token });
    const mine = list.body.find((x: any) => x.id === m.body.machine.id);
    expect(mine).toMatchObject({ connected: true, healthy: true, labels: ["mac", "main"], daemonVersion: "0.1.0", runningRuns: [] });
    expect(mine.health.diskFreeBytes).toBe(42e9);
    expect(typeof mine.lastHeartbeatAt).toBe("number");

    // A second link replaces the first.
    const { d: d2 } = await Daemon.connect(m.body.token);
    await waitUntil(() => d.closed, 5000, "old link closed");

    expect((await api(`/v1/machines/${m.body.machine.id}`, { method: "DELETE", token: owner.token })).status).toBe(200);
    await waitUntil(() => d2.closed, 5000, "revoked link closed");
    const again = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${m.body.token}` } });
    expect(again.status).toBe(401);
  });
});

// ---------------------------------------------------------------------------
// BotDesk: dispatch, streaming, results, follow-ups, loops, leases, stalls
// ---------------------------------------------------------------------------

describe("runner-backed bots", () => {
  it("configure a runner only for bots, as owner", async () => {
    const owner = await createMember({ level: "owner" });
    const human = await createMember();
    const bot = await createMember({ kind: "bot" });
    expect((await api(`/v1/bots/${human.id}/runner`, { method: "PUT", token: owner.token, body: { runtime: "codex" } })).status).toBe(404);
    expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: human.token, body: { runtime: "codex" } })).status).toBe(403);
    expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: owner.token, body: { runtime: "codex", repo: { url: "u", path: "p" } } })).status).toBe(400);
    const r = await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: owner.token, body: { runtime: "claude" } });
    expect(r.body).toMatchObject({ botId: bot.id, runtime: "claude", labels: [], envAllowlist: [], enabled: true });
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: owner.token })).body.runtime).toBe("claude");
    expect((await api(`/v1/bots/${bot.id}/runner`, { method: "DELETE", token: owner.token })).status).toBe(200);
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: owner.token })).status).toBe(404);
  });

  it("dispatch a mention to a machine, stream progress, post the result and set the topic status", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);

    const assign = await d.next((f) => f.t === "run.assign");
    expect(assign).toMatchObject({ botMemberId: w.bot.id, groupId: w.gid, topicId, runtime: "codex", repo: { path: "/tmp/repo", baseBranch: "main" } });
    expect(assign.prompt).toContain("@dev please fix tests");
    expect(assign.sessionUpload.url).toBe(`${BASE}/v1/machine/runs/${assign.runId}/session`);
    const runId = assign.runId as string;

    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.ack), 5000, "ack");
    await waitUntil(() => topicStatuses(c, topicId).includes("in_progress"), 5000, "in_progress");
    const owned = c.events().find((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.ownerId);
    expect(owned.data.ownerId).toBe(w.bot.id);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "created");

    d.send({ t: "run.accept", runId });
    const running = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    expect(running.data.run).toMatchObject({ botId: w.bot.id, machineId: w.machineId, topicId, requestedBy: w.human.id });

    d.event(runId, { type: "status", state: "running" });
    d.event(runId, { type: "message.delta", itemId: "m1", text: "正在看" });
    d.event(runId, { type: "tool.call", itemId: "c1", tool: "bash", title: "cargo test" });
    const s = d.event(runId, { type: "message", itemId: "m1", role: "assistant", text: "正在看测试" });
    await d.acked(runId, s);
    const progress = await waitUntil(
      () => botMessages(c, topicId, w.bot.id).find((m) => m.streaming),
      5000,
      "streaming progress message",
    );
    const seqDone = d.done(runId, { status: "succeeded", exitCode: 0, resultText: "修好了：sync 测试不再随机失败。", summary: "修好了" });
    await d.acked(runId, seqDone);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body.startsWith("修好了")), 5000, "result");
    const final = await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === progress.id && !e.data.streaming);
    expect(final.data.body).toContain("cargo test");
    expect(final.data.body).toContain("测试");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "done", 5000, "done");
    const doneRun = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "done");
    expect(doneRun.data.run.summary).toBe("修好了：sync 测试不再随机失败。");
    // Exactly one acknowledgement.
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.body === TEXT.ack)).toHaveLength(1);

    // The run log for clients, paged.
    const page1 = await api(`/v1/runs/${runId}/events?limit=2`, { token: w.human.token });
    expect(page1.status).toBe(200);
    expect(page1.body.entries.map((e: any) => e.seq)).toEqual([1, 2]);
    expect(page1.body.hasMore).toBe(true);
    const page2 = await api(`/v1/runs/${runId}/events?afterSeq=2`, { token: w.human.token });
    expect(page2.body.entries.at(-1)).toMatchObject({ t: "run.done", status: "succeeded" });
    expect(page2.body.lastSeq).toBe(seqDone);
    expect((await api(`/v1/runs/${runId}`, { token: w.human.token })).body.status).toBe("done");
    const outsider = await createMember();
    expect((await api(`/v1/runs/${runId}/events`, { token: outsider.token })).status).toBe(404);

    // Replayed frames (after a reconnect) are stored once and acked again.
    d.send({ t: "run.event", runId, seq: 2, at: Date.now(), event: { type: "message.delta", itemId: "m1", text: "正在看" } });
    await d.next((f) => f.t === "ack" && f.runId === runId && f.seq === seqDone && d.frames.filter((x) => x.t === "ack").length > 2);
    expect((await api(`/v1/runs/${runId}/events`, { token: w.human.token })).body.entries).toHaveLength(seqDone);

    // Raw session: uploaded with the machine token, downloaded by owners only.
    const gz = new Uint8Array([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    const put = await SELF.fetch(`${BASE}/v1/machine/runs/${runId}/session`, {
      method: "PUT",
      headers: { authorization: `Bearer ${w.machineToken}`, "content-type": "application/gzip", "content-length": String(gz.byteLength), "x-session-sha256": "a".repeat(64), "x-session-bytes": "0" },
      body: gz,
    });
    expect(put.status).toBe(200);
    expect((await api(`/v1/runs/${runId}/session`, { token: w.human.token })).status).toBe(403);
    const got = await SELF.fetch(`${BASE}/v1/runs/${runId}/session`, { headers: { authorization: `Bearer ${w.owner.token}` } });
    expect(got.status).toBe(200);
    expect(new Uint8Array(await got.arrayBuffer())).toEqual(gz);
    const other = await SELF.fetch(`${BASE}/v1/machine/runs/${runId}/session`, { method: "PUT", headers: { authorization: "Bearer tm_x", "content-length": "1" }, body: "x" });
    expect(other.status).toBe(401);
    d.send({ t: "run.session", runId, seq: seqDone + 1, at: Date.now(), ok: true, source: "cli", bytes: 0, gzipBytes: gz.byteLength });
    await d.acked(runId, seqDone + 1);
    c.close();
    d.close();
  });

  it("send later topic messages to the running run and route approvals", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");

    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "Also update the docs" });
    const input = await d.next((f) => f.t === "run.input");
    expect(input).toMatchObject({ runId, text: "Also update the docs", authorId: w.human.id });
    d.event(runId, { type: "input.accepted", inputId: input.inputId });

    d.event(runId, { type: "approval.request", approvalId: "a1", title: "Run git push", detail: "git push origin bot/x" });
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "waiting_answer", 5000, "waiting_answer");
    const waiting = c.events().filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "waiting_answer").at(-1);
    expect(waiting.data.waitingOn).toEqual([w.human.id]);
    const ask = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body.includes("需要你确认")), 5000, "approval message");
    expect(ask.mentions).toEqual([w.human.id]);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "waiting");

    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ runId, approvalId: "a1", decision: "approve" });
    d.event(runId, { type: "approval.resolved", approvalId: "a1", decision: "approve" });
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "in_progress", 5000, "back to in_progress");

    // Input the CLI never took starts a new run when this one ends.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "And add a test" });
    await d.next((f) => f.t === "run.input" && f.text === "And add a test");
    d.done(runId, { status: "succeeded", resultText: "done" });
    const second = await d.next((f) => f.t === "run.assign" && f.runId !== runId);
    expect(second.prompt).toContain("And add a test");
    c.close();
    d.close();
  });

  it("never let bot messages trigger bots, and keep one responsible bot per topic", async () => {
    const w = await world();
    const other = await createMember({ kind: "bot", name: "ops" });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: other.id } });
    await api(`/v1/bots/${other.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex" } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c: botClient } = await Client.connect(w.bot.token);
    const { c } = await Client.connect(w.human.token);

    // A bot mentioning a runner-backed bot starts nothing.
    const fromBot = await botClient.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@ops look", mentions: [other.id] });
    // A human mention of dev claims the topic; a later mention of ops in it goes to dev's run.
    const topicId = await mention(c, w, "@dev start");
    const first = await d.next((f) => f.t === "run.assign");
    expect(first.botMemberId).toBe(w.bot.id);
    d.send({ t: "run.accept", runId: first.runId });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@ops help", mentions: [other.id] });
    const input = await d.next((f) => f.t === "run.input");
    expect(input).toMatchObject({ runId: first.runId, text: "@ops help" });
    // The bot's own messages in its topic do not reach its run.
    await botClient.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "note from dev" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "sync" });
    await d.next((f) => f.t === "run.input" && f.text === "sync");
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    expect(d.frames.filter((f) => f.t === "run.input").map((f) => f.text)).toEqual(["@ops help", "sync"]);
    expect(botMessages(c, fromBot.message.topicId, other.id)).toEqual([]);
    c.close();
    botClient.close();
    d.close();
  });

  it("give no new run to a machine with low disk or the wrong labels", async () => {
    const w = await world({ labels: ["gpu-only"] });
    const { d } = await Daemon.connect(w.machineToken, { labels: ["mac"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.noMachine), 5000, "no machine note");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "blocked", 5000, "blocked");
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);

    const w2 = await world();
    const { d: low } = await Daemon.connect(w2.machineToken, { health: { diskFreeBytes: 1e9 } });
    const { c: c2 } = await Client.connect(w2.human.token);
    const t2 = await mention(c2, w2);
    await waitUntil(() => botMessages(c2, t2, w2.bot.id).some((m) => m.body === TEXT.noMachine), 5000, "no machine note");
    expect(low.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    c.close();
    c2.close();
    d.close();
    low.close();
  });

  it("keep a run through a reconnect, and block it when the machine stays away past the lease", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    const s1 = d.event(runId, { type: "status", state: "running" });
    await d.acked(runId, s1);
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");

    // Reconnect within the lease: the run is still known, with the stored seq.
    const { d: d2, welcome } = await Daemon.connect(w.machineToken, { runs: [{ runId, lastSeq: 3, done: false }] });
    expect(welcome.runs).toEqual([{ runId, ackedSeq: s1 }]);
    d2.seq.set(runId, s1);
    d2.event(runId, { type: "message.delta", itemId: "m", text: "still here" });
    await d2.acked(runId, s1 + 1);
    d2.close();
    await waitUntil(() => d2.closed, 5000, "closed");

    // Away past the lease: the run is blocked with one note, never run again.
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET lease_until = ? WHERE run_id = ?", Date.now() - 1, runId);
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.lost), 5000, "lost note");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "blocked", 5000, "blocked");
    const run = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "blocked");
    expect(run.data.run.endedAt).toBeTypeOf("number");
    const { d: d3, welcome: w3 } = await Daemon.connect(w.machineToken, { runs: [{ runId, lastSeq: 9, done: false }] });
    expect(w3.runs).toEqual([]);
    expect(d3.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    // Late frames of the lost run are acked and dropped.
    d3.send({ t: "run.event", runId, seq: 9, at: Date.now(), event: { type: "status", state: "running" } });
    await d3.acked(runId, 9);
    expect(topicStatuses(c, topicId).at(-1)).toBe("blocked");
    c.close();
    d3.close();
  });

  it("resend an assign that got no answer on the next link", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    const { d: d2 } = await Daemon.connect(w.machineToken);
    expect((await d2.next((f) => f.t === "run.assign")).runId).toBe(runId);
    c.close();
    d2.close();
  });

  it("detect a stall after 10 minutes without events and recover on the next event", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    const alarm = await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - STALL_MS - 1000, runId);
      return state.storage.getAlarm();
    });
    expect(alarm).toBeTypeOf("number");
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.stalled), 5000, "stall note");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "blocked", 5000, "blocked");
    // A second alarm does not post a second note.
    await runDurableObjectAlarm(deskOf(w.bot.id));
    d.event(runId, { type: "status", state: "running" });
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "in_progress", 5000, "recovered");
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.body === TEXT.stalled)).toHaveLength(1);
    c.close();
    d.close();
  });
});
