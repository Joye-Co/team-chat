import { SELF, env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import {
  type MachineView,
  MIN_FREE_DISK_BYTES,
  Progress,
  STALL_MS,
  TEXT,
  approvalDecision,
  readableError,
  candidates,
  isHealthy,
  isOnline,
  labelsMatch,
  whyNoMachine,
} from "../src/bots/dispatch";
import type { BotsEnv } from "../src/bots/env";
import { Daemon, HEALTH, type World, downViolations, mention, world } from "./daemon";
import { ACK_EMOJI } from "@chat/protocol";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

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

  it("run a runner pinned to one machine only there, whatever the labels", () => {
    const other = { ...base, id: "m2" };
    const pinned = { ...base, id: "m3", labels: [] };
    const r = { ...runner, machineId: "m3" };
    expect(candidates([other, pinned], r, now).map((m) => m.id)).toEqual(["m3"]);
    // The pinned machine still has to be healthy and have the runtime.
    expect(candidates([other, { ...pinned, connected: false }], r, now)).toEqual([]);
    expect(candidates([other], r, now)).toEqual([]);
  });

  it("tell why no machine takes a run: none, runtime, busy, offline (by name) or disk", () => {
    const pinned = { ...runner, machineId: "m3" };
    expect(whyNoMachine([], runner, now)).toEqual({ why: "none" });
    expect(whyNoMachine([{ ...base, labels: ["linux"] }], runner, now)).toEqual({ why: "none" });
    // A machine that never said hello is not set up yet.
    expect(whyNoMachine([{ ...base, clis: undefined, connected: false, lastHeartbeatAt: undefined }], runner, now)).toEqual({ why: "none" });
    expect(whyNoMachine([{ ...base, clis: [{ runtime: "claude", kind: "claude", command: "claude", available: true }] }], runner, now)).toEqual({ why: "runtime" });
    expect(whyNoMachine([{ ...base, runningRuns: ["a", "b"] }], runner, now)).toEqual({ why: "busy" });
    expect(whyNoMachine([{ ...base, id: "m3", name: "Joye 的 Mac", connected: false, lastHeartbeatAt: now - 10 * 60_000 }], pinned, now)).toEqual({ why: "offline", names: ["Joye 的 Mac"] });
    // A busy machine wins over an offline one: the run waits for a slot.
    expect(whyNoMachine([{ ...base, runningRuns: ["a", "b"] }, { ...base, id: "m2", name: "b", connected: false }], runner, now)).toEqual({ why: "busy" });
    expect(whyNoMachine([{ ...base, health: { ...HEALTH, diskFreeBytes: 1e9 } }], runner, now)).toEqual({ why: "disk" });
    expect(isOnline({ ...base, health: { ...HEALTH, diskFreeBytes: 1e9 } }, now)).toBe(true);
    expect(isOnline({ ...base, lastHeartbeatAt: now - 91_000 }, now)).toBe(false);
  });

  it("read approval answers", () => {
    expect(readableError('{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The \'gpt-6.1-sol\' model is not supported when using Codex with a ChatGPT account.","code":"model_not_found"}}')).toBe(
      "这台电脑上的账号用不了模型 gpt-6.1-sol，换个模型再试",
    );
    expect(readableError('{"error":{"message":"rate limited"}}')).toBe("rate limited");
    expect(readableError("exit 1")).toBe("exit 1");
    expect(approvalDecision("同意")).toBe("approve");
    expect(approvalDecision("@dev 同意。")).toBe("approve");
    expect(approvalDecision("拒绝")).toBe("deny");
    expect(approvalDecision("同意，但是先改 README")).toBe(null);
    // A plain 好 / 行 / 可以 / ok may be said to someone else: only in a reply to the card.
    for (const short of ["好", "好的", "行", "可以", "ok", "OK", "yes"]) {
      expect(approvalDecision(short)).toBe(null);
      expect(approvalDecision(short, true)).toBe("approve");
    }
    for (const no of ["不行", "no"]) {
      expect(approvalDecision(no)).toBe(null);
      expect(approvalDecision(no, true)).toBe("deny");
    }
    expect(approvalDecision("同意", true)).toBe("approve");
    expect(approvalDecision("拒绝", true)).toBe("deny");
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
    // Usable: a member adds only his own machine (machine-owner.test.ts).
    expect((await api("/v1/machines", { token: member.token, body: { name: "x", personal: false } })).status).toBe(403);
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
    const list = await api("/v1/machines", { token: owner.token });
    const mine = list.body.find((x: any) => x.id === m.body.machine.id);
    expect(mine).toMatchObject({ connected: true, healthy: true, labels: ["mac", "main"], daemonVersion: "0.1.0", runningRuns: [] });
    expect(mine.health.diskFreeBytes).toBe(42e9);
    expect(typeof mine.lastHeartbeatAt).toBe("number");
    // A member sees only the name and state, nothing about the machine itself.
    const seen = (await api("/v1/machines", { token: member.token })).body.find((x: any) => x.id === m.body.machine.id);
    expect(seen).toEqual({
      id: m.body.machine.id,
      name: "mac-mini",
      labels: [],
      createdBy: owner.id,
      createdAt: mine.createdAt,
      connected: true,
      healthy: true,
      runningRuns: [],
    });

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
    // 1.2.2: pin to one machine; absent keeps it, null clears it, an unknown machine is 404.
    const m = await api("/v1/machines", { token: owner.token, body: { name: "mbp" } });
    const mid = m.body.machine.id as string;
    const put = (body: Record<string, unknown>) => api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: owner.token, body });
    expect((await put({ runtime: "claude", machineId: mid })).body.machineId).toBe(mid);
    expect((await put({ runtime: "claude", labels: ["x"] })).body.machineId).toBe(mid);
    expect((await put({ runtime: "claude", machineId: null })).body.machineId).toBeUndefined();
    expect((await put({ runtime: "claude", machineId: "01JBX0MACH00000000000000ZZ" })).status).toBe(404);
    await put({ runtime: "claude", machineId: mid });
    expect((await api(`/v1/machines/${mid}`, { method: "DELETE", token: owner.token })).status).toBe(200);
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: owner.token })).body.machineId).toBeUndefined();
    expect((await api(`/v1/bots/${bot.id}/runner`, { method: "DELETE", token: owner.token })).status).toBe(200);
    expect((await api(`/v1/bots/${bot.id}/runner`, { token: owner.token })).status).toBe(404);
  });

  it("dispatch a mention to a machine, acknowledge with a reaction, post a done card and set the topic status", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev please fix tests", mentions: [w.bot.id] });
    const topicId = sent.message.topicId as string;
    const trigger = sent.message;

    const assign = await d.next((f) => f.t === "run.assign");
    expect(assign).toMatchObject({ botMemberId: w.bot.id, groupId: w.gid, topicId, runtime: "codex", repo: { path: "/tmp/repo", baseBranch: "main" } });
    expect(assign.kind).toBeUndefined();
    expect(assign.prompt).toContain("@dev please fix tests");
    expect(assign.sessionUpload.url).toBe(`${BASE}/v1/machine/runs/${assign.runId}/session`);
    const runId = assign.runId as string;

    // The acknowledgement is a reaction on the request, not a text line.
    const ack = await c.waitEvent((e) => e.type === "reaction.changed" && e.data.messageId === trigger.id);
    expect(ack).toMatchObject({ actorId: w.bot.id, data: { emoji: ACK_EMOJI, on: true } });
    expect(ACK_EMOJI).toBe("👀");
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
    const result = "修好了：sync 测试不再随机失败。\nhttps://github.com/acme/app/pull/42\ntest result: ok. 12 passed; 0 failed";
    const seqDone = d.done(runId, {
      status: "succeeded",
      exitCode: 0,
      resultText: result,
      summary: "修好了",
      usage: { inputTokens: 1200, outputTokens: 340, costUsd: 0.05 },
      durationMs: 61_000,
    });
    await d.acked(runId, seqDone);
    const card = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.cardV2), 5000, "done card");
    expect(card.body).toBe(result);
    expect(card.kind).toBe("card");
    expect(card.cardV2).toMatchObject({ v: 2, intent: "done", title: "修好了：sync 测试不再随机失败。", data: { runId } });
    // The first line is the title; the text block holds only the lines after it.
    expect(card.cardV2.blocks).toContainEqual({ type: "text", id: "result", text: result.split("\n").slice(1).join("\n") });
    expect(card.cardV2.blocks).toContainEqual({ type: "evidence", id: "tests", label: "测试：12 个通过", state: "pass", tests: { passed: 12, failed: 0 } });
    // The pull request the result names becomes a topic link.
    const linked = await c.waitEvent((e) => e.type === "topic.linked" && e.data.topicId === topicId);
    expect(linked.data.link).toMatchObject({ kind: "pr", repo: "acme/app", number: 42, url: "https://github.com/acme/app/pull/42", state: "open" });
    expect(card.cardV2.blocks).toContainEqual({
      type: "ref",
      id: "pr-1",
      label: "acme/app#42",
      target: { kind: "pr", url: "https://github.com/acme/app/pull/42", repo: "acme/app", number: 42 },
    });
    // Old clients get a derived v1 card. A 1.0.x client draws the body unless it is empty or
    // the card title, then the v1 text blocks: the result shows once.
    expect(card.card).toMatchObject({ v: 1, tone: "success" });
    const drawn10 = [
      ...(card.body === "" || card.body === card.card.title.trim() ? [] : [card.body]),
      ...card.card.blocks.filter((b: any) => b.type === "text").map((b: any) => b.text),
    ];
    expect(drawn10).toEqual([result]);
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "done", 5000, "done");
    const doneRun = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "done");
    expect(doneRun.data.run).toMatchObject({
      summary: "修好了：sync 测试不再随机失败。",
      resultMessageId: card.id,
      usage: { inputTokens: 1200, outputTokens: 340, costUsd: 0.05 },
      durationMs: 61_000,
    });
    // No progress message and no text acknowledgement: the done card is the bot's only message.
    expect(botMessages(c, topicId, w.bot.id)).toHaveLength(1);
    expect(botMessages(c, topicId, w.bot.id).some((m) => m.streaming)).toBe(false);

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

  it("never let bot messages trigger bots, and give a mention in another bot's topic to the bot it names", async () => {
    const w = await world();
    const other = await createMember({ kind: "bot", name: "ops" });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: other.id } });
    await api(`/v1/bots/${other.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex" } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c: botClient } = await Client.connect(w.bot.token);
    const { c } = await Client.connect(w.human.token);

    // A bot mentioning a runner-backed bot starts nothing.
    const fromBot = await botClient.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@ops look", mentions: [other.id] });
    // A human mention of dev starts dev's run; a later mention of ops in its topic starts ops's own run (接 1.1).
    const topicId = await mention(c, w, "@dev start");
    const first = await d.next((f) => f.t === "run.assign");
    expect(first.botMemberId).toBe(w.bot.id);
    d.send({ t: "run.accept", runId: first.runId });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@ops help", mentions: [other.id] });
    const second = await d.next((f) => f.t === "run.assign" && f.runId !== first.runId);
    expect(second).toMatchObject({ botMemberId: other.id, topicId });
    d.send({ t: "run.accept", runId: second.runId });
    // The bot's own messages in its topic do not reach any run.
    await botClient.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "note from dev" });
    // Without an @ the requester's message goes to the member that most recently started a run.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "sync" });
    await d.next((f) => f.t === "run.input" && f.text === "sync");
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
    expect(d.frames.filter((f) => f.t === "run.input").map((f) => [f.runId, f.text])).toEqual([[second.runId, "sync"]]);
    expect(botMessages(c, fromBot.message.topicId, other.id)).toEqual([]);
    c.close();
    botClient.close();
    d.close();
  });

  it("retry a mention whose hand-off to the bot failed, and hand it off once", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const room = benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(w.gid));
    const outbox = () =>
      runInDurableObject(room, (_i, state: DurableObjectState) =>
        state.storage.sql.exec<{ attempts: number }>("SELECT attempts FROM bot_outbox").toArray(),
      );
    // D1 fails while the room hands the mention off (the bot lookup reads bot_runners).
    await env.DB.exec("ALTER TABLE bot_runners RENAME TO bot_runners_down");
    let sent: any;
    try {
      sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev fix it", mentions: [w.bot.id] });
      for (let i = 0; !(await outbox()).some((r) => r.attempts >= 1); i++) {
        if (i > 200) throw new Error("timed out waiting for the failed hand-off to be kept");
        await new Promise((r) => setTimeout(r, 20));
      }
    } finally {
      await env.DB.exec("ALTER TABLE bot_runners_down RENAME TO bot_runners");
    }
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    // The room alarm retries (made due now) and the desk accepts: the run starts.
    await runInDurableObject(room, (_i, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE bot_outbox SET next_at = 0");
    });
    expect(await runDurableObjectAlarm(room)).toBe(true);
    const assign = await d.next((f) => f.t === "run.assign");
    expect(assign).toMatchObject({ botMemberId: w.bot.id, topicId: sent.message.topicId });
    expect(await outbox()).toEqual([]);
    // A repeated hand-off of the same message (a retry after a lost answer) starts nothing.
    await deskOf(w.bot.id).onMessage(w.bot.id, {
      groupId: w.gid,
      topicId: sent.message.topicId,
      messageId: sent.message.id,
      authorId: w.human.id,
      body: "@dev fix it",
      mentioned: true,
    });
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    c.close();
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
    await waitUntil(() => botMessages(c2, t2, w2.bot.id).some((m) => m.body === TEXT.noDisk), 5000, "low disk note");
    expect(low.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    c.close();
    c2.close();
    d.close();
    low.close();
  });

  it("send a runner pinned to a machine there even when the labels do not match", async () => {
    const w = await world({ labels: ["gpu-only"] });
    const r = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", machineId: w.machineId } });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const { d } = await Daemon.connect(w.machineToken, { labels: ["mac"] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    await waitUntil(() => d.frames.some((f) => f.t === "run.assign"), 5000, "assigned to the pinned machine");
    c.close();
    d.close();
  });

  it("keep a run through a reconnect and past the lease while the machine is away, and block it when the daemon comes back without it", async () => {
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

    // Away past the lease (盯 1.1): the only machine is waited for, the run stays open.
    await runInDurableObject(deskOf(w.bot.id), (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET interventions = 3 WHERE id = ?", runId);
    });
    const expire = () =>
      runInDurableObject(linkOf(w.machineId), (_o, state) => {
        state.storage.sql.exec("UPDATE runs SET lease_until = ? WHERE run_id = ?", Date.now() - 1, runId);
      });
    await expire();
    await runDurableObjectAlarm(linkOf(w.machineId));
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.machineAway("mac-mini")), 5000, "away note");
    const lease = await runInDurableObject(linkOf(w.machineId), (_o, state) =>
      state.storage.sql.exec<{ state: string; lease_until: number }>("SELECT state, lease_until FROM runs WHERE run_id = ?", runId).one(),
    );
    expect(lease.state).toBe("accepted");
    expect(lease.lease_until).toBeGreaterThan(Date.now());
    expect(botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.lost)).toBe(false);

    // The daemon comes back without the run (it lost it): at the next lease end it is blocked
    // with one note, never run again (its tries are spent).
    const { d: d4, welcome: back } = await Daemon.connect(w.machineToken, { runs: [] });
    expect(back.runs).toEqual([{ runId, ackedSeq: s1 + 1 }]);
    await expire();
    await runDurableObjectAlarm(linkOf(w.machineId));
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.lost), 5000, "lost note");
    d4.close();
    await waitUntil(() => d4.closed, 5000, "closed");
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

  it("detect a stall after 10 minutes without events once the desk's own tries are spent, and recover on the next event", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    const alarm = await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ?, interventions = 3, nudge_sent = 1 WHERE id = ?", Date.now() - STALL_MS - 1000, runId);
      return state.storage.getAlarm();
    });
    expect(alarm).toBeTypeOf("number");
    await runDurableObjectAlarm(deskOf(w.bot.id));
    // 盯 1.12: the note names the true silence and the tries that happened.
    const gaveUp = TEXT.gaveUp(10, ["提醒"]);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === gaveUp), 5000, "stall note");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "blocked", 5000, "blocked");
    // A second alarm does not post a second note.
    await runDurableObjectAlarm(deskOf(w.bot.id));
    d.event(runId, { type: "status", state: "running" });
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "in_progress", 5000, "recovered");
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.body === gaveUp)).toHaveLength(1);
    c.close();
    d.close();
  });
});

// ---------------------------------------------------------------------------
// Run plans (docs/protocol.md, "Run plans")
// ---------------------------------------------------------------------------

describe("run plans", () => {
  const plan = [
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "current" },
    { text: "等 Joye 确认", state: "pending" },
  ];
  const plan2 = [
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "done" },
    { text: "等 Joye 确认", state: "waiting" },
    { text: "周四 20:00 发出", state: "pending" },
  ];
  const runUpdates = (c: Client, runId: string) => c.events().filter((e) => e.type === "run.updated" && e.data.run.id === runId);

  async function externalWorld() {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const human = await createMember({ name: "Alice" });
    const bot = await createMember({ kind: "bot", name: "taro" });
    const other = await createMember({ kind: "bot", name: "other" });
    const gid = await createGroup(owner, [human.id, bot.id, other.id]);
    const { c } = await Client.connect(human.token);
    const sent = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@taro 写一条微博", mentions: [bot.id] });
    return { owner, human, bot, other, gid, c, topicId: sent.message.topicId as string };
  }

  it("let an external bot start a run with a plan, replace it, clear it and end the run", async () => {
    const { human, bot, other, gid, c, topicId } = await externalWorld();
    const start = await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId, plan } });
    expect(start.status, JSON.stringify(start.body)).toBe(200);
    expect(start.body).toMatchObject({ botId: bot.id, groupId: gid, topicId, status: "running", plan });
    expect(start.body.machineId).toBeUndefined();
    expect(start.body.requestedBy).toBeUndefined();
    const runId = start.body.id as string;
    const first = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId);
    expect(first.actorId).toBe(bot.id);
    expect(first.data.run.plan).toEqual(plan);

    // Starting again returns the active run: no second run in the topic.
    const again = await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId } });
    expect(again.body.id).toBe(runId);

    // An update replaces the whole plan and fans out with the run.
    const put = await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: plan2 } });
    expect(put.status, JSON.stringify(put.body)).toBe(200);
    expect(put.body.plan).toEqual(plan2);
    const second = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.plan?.length === 4);
    expect(second.data.run.plan).toEqual(plan2);
    expect(second.data.run.updatedAt).toBeGreaterThanOrEqual(first.data.run.updatedAt);
    expect((await api(`/v1/runs/${runId}`, { token: human.token })).body.plan).toEqual(plan2);

    // The same plan again stores nothing.
    const before = runUpdates(c, runId).length;
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: plan2 } })).status).toBe(200);
    // A later change proves the room is past the no-op.
    await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: [] } });
    const cleared = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && !e.data.run.plan);
    expect(cleared.data.run.plan).toBeUndefined();
    expect(runUpdates(c, runId).length).toBe(before + 1);

    // Only the run's bot sets the plan; invalid plans are rejected.
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: human.token, body: { plan } })).status).toBe(403);
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: other.token, body: { plan } })).status).toBe(403);
    const outsider = await createMember({ kind: "bot", name: "outsider" });
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: outsider.token, body: { plan } })).status).toBe(404);
    const long = await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: [{ text: "字".repeat(201), state: "current" }] } });
    expect(long.status).toBe(400);
    const badState = await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: [{ text: "x", state: "in_progress" }] } });
    expect(badState.status).toBe(400);
    const tooMany = Array.from({ length: 31 }, (_, i) => ({ text: `step ${i}`, state: "pending" }));
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: tooMany } })).status).toBe(400);

    // Only the run's bot ends it; an ended run takes no plan.
    expect((await api(`/v1/runs/${runId}/end`, { token: other.token, body: { status: "done" } })).status).toBe(403);
    await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan: plan2 } });
    const end = await api(`/v1/runs/${runId}/end`, { token: bot.token, body: { status: "done", summary: "已排期" } });
    expect(end.status, JSON.stringify(end.body)).toBe(200);
    expect(end.body).toMatchObject({ status: "done", summary: "已排期", plan: plan2 });
    expect(end.body.endedAt).toBeTypeOf("number");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "done");
    expect((await api(`/v1/runs/${runId}/plan`, { method: "PUT", token: bot.token, body: { plan } })).status).toBe(409);
    expect((await api(`/v1/runs/${runId}/end`, { token: bot.token, body: { status: "done" } })).status).toBe(409);

    // A new start after the end makes a new run.
    const next = await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId } });
    expect(next.body.id).not.toBe(runId);
    expect(next.body.plan).toBeUndefined();
    c.close();
  });

  it("start runs only for external bots in their groups and topics", async () => {
    const { human, bot, gid, c, topicId } = await externalWorld();
    expect((await api("/v1/runs", { token: human.token, body: { groupId: gid, topicId } })).status).toBe(403);
    expect((await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId: "01NOPE00000000000000000000" } })).status).toBe(404);
    const outsider = await createMember({ kind: "bot", name: "outsider" });
    expect((await api("/v1/runs", { token: outsider.token, body: { groupId: gid, topicId } })).status).toBe(404);
    expect((await api("/v1/runs", { token: bot.token, body: { groupId: gid } })).status).toBe(400);
    // A runner-backed bot gets runs from mentions only.
    const w = await world();
    const { c: c2 } = await Client.connect(w.human.token);
    const t2 = await c2.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "hi" });
    expect((await api("/v1/runs", { token: w.bot.token, body: { groupId: w.gid, topicId: t2.message.topicId } })).status).toBe(409);
    c.close();
    c2.close();
  });

  it("never mark an external run stalled", async () => {
    const { bot, gid, c, topicId } = await externalWorld();
    const runId = (await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId, plan } })).body.id as string;
    await runInDurableObject(deskOf(bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - STALL_MS - 1000, runId);
      await state.storage.setAlarm(Date.now());
    });
    await runDurableObjectAlarm(deskOf(bot.id));
    expect((await api(`/v1/runs/${runId}`, { token: bot.token })).body.status).toBe("running");
    expect(botMessages(c, topicId, bot.id)).toHaveLength(0);
    c.close();
  });

  it("take the runner's plan events from the machine that holds the run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
    d.event(runId, { type: "plan", items: plan });
    d.event(runId, { type: "plan", items: plan2 });
    const s = d.event(runId, { type: "plan", items: plan2 });
    await d.acked(runId, s);
    const got = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.plan?.length === 4);
    expect(got.data.run.plan).toEqual(plan2);
    expect(got.data.run.status).toBe("running");
    // One batch or several, the last plan wins and equal plans store nothing new.
    const withPlan = () => runUpdates(c, runId).filter((e) => e.data.run.plan);
    expect(withPlan().at(-1).data.run.plan).toEqual(plan2);
    const n = withPlan().filter((e) => JSON.stringify(e.data.run.plan) === JSON.stringify(plan2)).length;
    expect(n).toBe(1);
    // 1.1: step times and estimates are kept.
    const timed = [
      { text: "读代码", state: "done", startedAt: 1_000, endedAt: 61_000 },
      { text: "改", state: "current", startedAt: 61_000, estimateMs: 600_000 },
    ];
    const st = d.event(runId, { type: "plan", items: timed });
    await d.acked(runId, st);
    const gotTimed = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.plan?.[1]?.startedAt === 61_000);
    expect(gotTimed.data.run.plan).toEqual(timed);
    // Items past the limits are cut to them (a newer or broken daemon), bad items are dropped.
    const s2 = d.event(runId, {
      type: "plan",
      items: [{ text: ` ${"字".repeat(250)} `, state: "current" }, { text: "", state: "pending" }, { text: "x", state: "in_progress" }, ...Array.from({ length: 40 }, (_, i) => ({ text: `step ${i}`, state: "pending" }))],
    });
    await d.acked(runId, s2);
    const cut = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.plan?.length === 30);
    expect(cut.data.run.plan[0]).toEqual({ text: "字".repeat(200), state: "current" });
    expect(cut.data.run.plan[1]).toEqual({ text: "step 0", state: "pending" });
    const s3 = d.event(runId, { type: "plan", items: plan2 });
    await d.acked(runId, s3);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.plan?.length === 4 && e.seq > cut.seq);
    // The bot's own token may set the plan of its runner's run too; the machine's run ends with the plan kept.
    const done = d.done(runId, { status: "succeeded", exitCode: 0, resultText: "好了", summary: "好了" });
    await d.acked(runId, done);
    const end = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "done");
    expect(end.data.run.plan).toEqual(plan2);
    // External-run endpoints do not end a runner's run.
    expect((await api(`/v1/runs/${runId}/end`, { token: w.bot.token, body: { status: "done" } })).status).toBe(403);
    c.close();
    d.close();
  });
});

// ---------------------------------------------------------------------------
// Review fixes: who may approve, deactivation, DMs, stuck runs, stopping runs
// ---------------------------------------------------------------------------

/** A running run of `w.bot` in a new topic, with an approval waiting. */
async function waitingApproval(w: World, d: Daemon, c: Client): Promise<{ topicId: string; runId: string; card: any }> {
  const topicId = await mention(c, w);
  const { runId } = await d.next((f) => f.t === "run.assign");
  d.send({ t: "run.accept", runId });
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
  d.event(runId, { type: "approval.request", approvalId: "a1", title: "rm -rf build", detail: "rm -rf build" });
  const card = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.kind === "card"), 5000, "approval card");
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "waiting");
  return { topicId, runId, card };
}

describe("runner-backed bots: review fixes", () => {
  it("take an approval only from the requester or an owner, never from another member or a bot", async () => {
    const w = await world();
    const intern = await createMember({ name: "intern" });
    const sdkBot = await createMember({ kind: "bot", name: "sdk" });
    for (const id of [intern.id, sdkBot.id]) {
      expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: id } })).status).toBe(200);
    }
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const { c: ci } = await Client.connect(intern.token);
    const { c: cb } = await Client.connect(sdkBot.token);
    const { topicId, card } = await waitingApproval(w, d, c);

    // Another bot presses approve, and an intern presses it or types 同意: nothing reaches the machine.
    await cb.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:a1" });
    await ci.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:a1" });
    await ci.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    // The intern's 同意 is plain input for the run.
    await d.next((f) => f.t === "run.input" && f.text === "同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);

    // The requester's press counts, and is audited.
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "deny:a1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "a1", decision: "deny" });
    const audit = await env.DB.prepare("SELECT actor_id, details FROM audit WHERE action = 'run.approval' AND actor_id = ?")
      .bind(w.human.id)
      .first<{ actor_id: string; details: string }>();
    expect(JSON.parse(audit?.details ?? "{}")).toMatchObject({ decision: "deny", via: "card" });
    c.close();
    ci.close();
    cb.close();
    d.close();
  });

  it("let an owner approve by text", async () => {
    const w = await world();
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: w.owner.id } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const { topicId } = await waitingApproval(w, d, c);
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "a1", decision: "approve" });
    c.close();
    co.close();
    d.close();
  });

  it("stop a deactivated bot's runs, release its topics and give it no more work", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");

    const off = await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { deactivated: true } });
    expect(off.status, JSON.stringify(off.body)).toBe(200);
    expect(await d.next((f) => f.t === "run.cancel")).toMatchObject({ runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "cancelled");
    const claims = await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(w.gid)), (_o, state) =>
      state.storage.sql.exec("SELECT count(*) AS n FROM bot_topics WHERE bot_id = ?", w.bot.id).one().n,
    );
    expect(claims).toBe(0);

    // Messages in its old topic and new mentions start nothing.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "再试一次" });
    await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev again", mentions: [w.bot.id] });
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    expect(d.frames.filter((f) => f.t === "run.input")).toEqual([]);
    c.close();
    d.close();
  });

  it("answer a message in a DM with a runner-backed bot without an @", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    expect(dm.status).toBe(200);
    const { c } = await Client.connect(w.human.token);
    const res = await c.ok({ t: "send", groupId: dm.body.group.id, clientMsgId: clientMsgId(), body: "帮我整理今天的待办" });
    const assign = await d.next((f) => f.t === "run.assign");
    expect(assign).toMatchObject({ botMemberId: w.bot.id, topicId: res.message.topicId });
    expect(assign.prompt).toContain("帮我整理今天的待办");
    c.close();
    d.close();
  });

  it("end a run whose card cannot be posted instead of leaving it running", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    // The owner removes the bot from the group mid-run: its posts now fail.
    expect((await api(`/v1/groups/${w.gid}/members/${w.bot.id}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    d.event(runId, { type: "message.delta", itemId: "m", text: "working" });
    d.event(runId, { type: "approval.request", approvalId: "a9", title: "push" });
    const s = d.done(runId, { status: "succeeded", resultText: "ok" });
    await d.acked(runId, s);
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "a9", decision: "deny" });
    const row = await runInDurableObject(deskOf(w.bot.id), (_o, state) =>
      state.storage.sql.exec("SELECT status, ended_at, approval_id, applied_seq FROM runs WHERE id = ?", runId).one(),
    );
    expect(row).toMatchObject({ status: "done", approval_id: null, applied_seq: s });
    expect(row.ended_at).toBeTypeOf("number");
    c.close();
    d.close();
  });

  it("apply stored frames again when an earlier apply did not finish", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    // As after a reset between store and apply: run.done is stored but not applied.
    await runInDurableObject(deskOf(w.bot.id), (_o, state) => {
      state.storage.sql.exec(
        "INSERT INTO run_log (run_id, seq, at, frame) VALUES (?, 1, ?, ?)",
        runId,
        Date.now(),
        JSON.stringify({ t: "run.done", runId, seq: 1, at: Date.now(), durationMs: 1, status: "succeeded", resultText: "all good" }),
      );
      state.storage.sql.exec("UPDATE runs SET last_seq = 1 WHERE id = ?", runId);
    });
    // The daemon resends it (it got no ack): the run now ends.
    d.seq.set(runId, 0);
    const s = d.done(runId, { status: "succeeded", resultText: "all good" });
    await d.acked(runId, s);
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "done", 5000, "done");
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.body === "all good")).toHaveLength(1);
    c.close();
    d.close();
  });

  it("end a run that never reached a machine", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const topic = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "a topic" });
    const topicId = topic.message.topicId as string;
    const runId = "01J0000000000000000STUCK0";
    await (deskOf(w.bot.id) as any).getRun(w.bot.id, "none");
    await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      const old = Date.now() - 10 * 60_000;
      state.storage.sql.exec(
        `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at)
         VALUES (?, ?, ?, 'created', ?, 'm0', ?, ?, ?)`,
        runId,
        w.gid,
        topicId,
        w.human.id,
        old,
        old,
        old,
      );
      await state.storage.setAlarm(Date.now() + 60_000);
    });
    expect(await runDurableObjectAlarm(deskOf(w.bot.id))).toBe(true);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "failed");
    // The topic takes a new run again.
    const { d } = await Daemon.connect(w.machineToken);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev go", mentions: [w.bot.id] });
    await d.next((f) => f.t === "run.assign");
    c.close();
    d.close();
  });

  it("let a person in the group stop a run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");

    const outsider = await createMember();
    expect((await api(`/v1/runs/${runId}/cancel`, { method: "POST", token: outsider.token })).status).toBe(404);
    expect((await api(`/v1/runs/${runId}/cancel`, { method: "POST", token: w.bot.token })).status).toBe(403);
    const r = await api(`/v1/runs/${runId}/cancel`, { method: "POST", token: w.human.token });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body).toMatchObject({ id: runId, status: "cancelled" });
    expect(await d.next((f) => f.t === "run.cancel")).toMatchObject({ runId });
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.cancelled), 5000, "cancel note");
    await waitUntil(() => topicStatuses(c, topicId).at(-1) === "open", 5000, "open");
    expect((await api(`/v1/runs/${runId}/cancel`, { method: "POST", token: w.human.token })).status).toBe(409);
    // A late run.done of the machine changes nothing.
    const s = d.done(runId, { status: "cancelled" });
    await d.acked(runId, s);
    expect((await api(`/v1/runs/${runId}`, { token: w.human.token })).body.status).toBe("cancelled");
    c.close();
    d.close();
  });

  it("start a run for input left after a follow-up run too", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId: a } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "M1 late" });
    await d.next((f) => f.t === "run.input" && f.text === "M1 late");
    d.done(a, { status: "succeeded", resultText: "A done" });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a);
    expect(b.prompt).toContain("M1 late");
    d.send({ t: "run.accept", runId: b.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.status === "running");
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "M2 late" });
    await d.next((f) => f.t === "run.input" && f.text === "M2 late");
    d.done(b.runId, { status: "succeeded", resultText: "B done" });
    const third = await d.next((f) => f.t === "run.assign" && f.runId !== a && f.runId !== b.runId);
    expect(third.prompt).toContain("M2 late");
    c.close();
    d.close();
  });

  it("not count the daemon's own stall report as progress, nor act on it (盯 1.3)", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    const before = Date.now() - 6 * 60_000;
    await runInDurableObject(deskOf(w.bot.id), (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ?, interventions = 3 WHERE id = ?", before, runId);
    });
    const s = d.event(runId, { type: "status", state: "stalled" });
    await d.acked(runId, s);
    await new Promise((r) => setTimeout(r, 200));
    // A signal only: no restart, no note; the desk's own clock decides.
    expect(topicStatuses(c, topicId).at(-1)).toBe("in_progress");
    expect(d.frames.filter((f) => f.t === "run.cancel")).toHaveLength(0);
    const row = await runInDurableObject(deskOf(w.bot.id), (_o, state) =>
      state.storage.sql.exec("SELECT last_event_at, stalled, ended_at FROM runs WHERE id = ?", runId).one(),
    );
    expect(row).toEqual({ last_event_at: before, stalled: 0, ended_at: null });
    c.close();
    d.close();
  });

  it("route runs only on labels an owner gave the machine", async () => {
    const label = `own-${crypto.randomUUID().slice(0, 6)}`;
    const w = await world({ labels: [label] });
    // The daemon claims the bot's label itself; the owner gave the machine another one.
    const { d } = await Daemon.connect(w.machineToken, { labels: [label] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.noMachine), 5000, "no machine note");
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    c.close();
    d.close();
  });

  it("stream no progress into the topic and post a failed run as one note", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    d.event(runId, { type: "tool.call", itemId: "c1", tool: "bash", title: "cargo test" });
    d.event(runId, { type: "message", itemId: "m1", role: "assistant", text: "还在看。" });
    const seq = d.done(runId, { status: "failed", error: "cargo test failed", usage: { inputTokens: 5, outputTokens: 6 } });
    await d.acked(runId, seq);
    const ended = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "failed");
    const msgs = botMessages(c, topicId, w.bot.id);
    expect(msgs.map((m) => m.body)).toEqual([TEXT.failed("cargo test failed")]);
    expect(msgs[0].cardV2).toBeUndefined();
    expect(ended.data.run).toMatchObject({ resultMessageId: msgs[0].id, usage: { inputTokens: 5, outputTokens: 6 }, durationMs: 1000 });
    c.close();
    d.close();
  });

  it("handle a message once when the room hook calls the desk again", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    const sent = await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "once" });
    await d.next((f) => f.t === "run.input" && f.text === "once");
    const m = { groupId: w.gid, topicId, messageId: sent.message.id, authorId: w.human.id, body: "once", mentioned: false };
    await (deskOf(w.bot.id) as any).onMessage(w.bot.id, m);
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.input" && f.text === "once")).toHaveLength(1);
    c.close();
    d.close();
  });
});
