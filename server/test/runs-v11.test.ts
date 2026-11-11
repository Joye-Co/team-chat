// 1.1 runs (docs/protocol.md, "Run fields, pause and resume" and "Side chats"): the output
// contract's done card and its proof, acceptance criteria, take-over (pause / resume) and side
// chat runs, against a fake daemon that reports the 1.1 features.
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { MachineDownFrame } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { STALL_MS, TEXT } from "../src/bots/dispatch";
import type { BotsEnv } from "../src/bots/env";
import { ACK_EMOJI } from "@chat/protocol";
import { PROOF_TEXT, doneCard, prRefs, testCounts } from "../src/bots/proof";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;
const downViolations: unknown[] = [];

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const HEALTH = { diskFreeBytes: 100e9, diskTotalBytes: 500e9, load1: 0.5, load5: 0.5, load15: 0.5, cpuCount: 8, activeRuns: 0 };

class Daemon {
  frames: any[] = [];
  seq = new Map<string, number>();
  constructor(readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      const f = JSON.parse(e.data as string);
      const check = MachineDownFrame.safeParse(f);
      if (!check.success) downViolations.push({ frame: f, issues: check.error.issues });
      this.frames.push(f);
    });
  }

  static async connect(token: string, features: string[], takeoverUrl?: string): Promise<Daemon> {
    const res = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${token}` } });
    if (!res.webSocket) throw new Error(`upgrade failed: ${res.status}`);
    res.webSocket.accept();
    const d = new Daemon(res.webSocket);
    d.send({
      t: "hello",
      protocol: 1,
      machine: { name: "mac-mini", labels: [], daemonVersion: "1.1.0", os: "macos", arch: "aarch64", maxParallelRuns: 4, features, ...(takeoverUrl ? { takeoverUrl } : {}) },
      clis: [{ runtime: "codex", kind: "codex", command: "codex", version: "0.159.0", available: true }],
      health: HEALTH,
      runs: [],
    });
    await d.next((f) => f.t === "welcome");
    return d;
  }

  send(f: unknown): void {
    this.ws.send(JSON.stringify(f));
  }

  async next(pred: (f: any) => boolean, ms = 5000): Promise<any> {
    return waitUntil(() => this.frames.find(pred), ms, "daemon frame");
  }

  private nextSeq(runId: string): number {
    const seq = (this.seq.get(runId) ?? 0) + 1;
    this.seq.set(runId, seq);
    return seq;
  }

  event(runId: string, event: unknown): number {
    const seq = this.nextSeq(runId);
    this.send({ t: "run.event", runId, seq, at: Date.now(), event });
    return seq;
  }

  done(runId: string, extra: Record<string, unknown>): number {
    const seq = this.nextSeq(runId);
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

interface World {
  owner: TestMember;
  human: TestMember;
  other: TestMember;
  bot: TestMember;
  gid: string;
  machineToken: string;
}

async function world(): Promise<World> {
  const label = `t-${crypto.randomUUID().slice(0, 8)}`;
  const owner = await createMember({ level: "owner", name: "Joye" });
  const human = await createMember({ name: "Alice" });
  const other = await createMember({ name: "Bob" });
  const bot = await createMember({ kind: "bot", name: "dev" });
  const gid = await createGroup(owner, [human.id, other.id, bot.id]);
  const r = await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: owner.token, body: { runtime: "codex", labels: [label] } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const m = await api("/v1/machines", { token: owner.token, body: { name: "mac-mini", labels: [label] } });
  expect(m.status).toBe(200);
  return { owner, human, other, bot, gid, machineToken: m.body.token };
}

/** The human mentions the bot in a new topic; the daemon accepts the run. */
async function startRun(w: World, c: Client, d: Daemon): Promise<{ topicId: string; runId: string }> {
  const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev ship it", mentions: [w.bot.id] });
  const topicId = sent.message.topicId as string;
  const { runId } = await d.next((f) => f.t === "run.assign" && f.topicId === topicId);
  d.send({ t: "run.accept", runId });
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
  return { topicId, runId };
}

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const lastRun = (c: Client, runId: string) => c.events().filter((e) => e.type === "run.updated" && e.data.run.id === runId).at(-1)?.data.run;

async function audited(action: string, target: string) {
  return env.DB.prepare("SELECT actor_id, details FROM audit WHERE action = ? AND target = ? ORDER BY id")
    .bind(action, target)
    .all<{ actor_id: string; details: string }>();
}

describe("done card rules", () => {
  it("read test counts and pull requests from a result", () => {
    expect(testCounts("test result: ok. 12 passed; 0 failed\ntest result: ok. 3 passed; 1 failed")).toEqual({ passed: 15, failed: 1 });
    expect(testCounts(" 485 pass\n 0 fail")).toEqual({ passed: 485, failed: 0 });
    expect(testCounts("Tests  33 passed (33)")).toEqual({ passed: 33, failed: 0 });
    expect(testCounts("all good")).toBeNull();
    expect(testCounts("Tests  12 passed | 1 failed | 2 skipped (15)")).toEqual({ passed: 12, failed: 1, skipped: 2 });
    const card = doneCard({ runId: "r", result: "做完了\n12 passed, 1 failed", prs: [], artifacts: [], acceptance: [], tests: { passed: 12, failed: 1 } });
    expect(card?.blocks).toContainEqual(expect.objectContaining({ type: "evidence", id: "tests", state: "fail", tests: { passed: 12, failed: 1 } }));
    expect(prRefs("see https://github.com/a/b/pull/7 and https://github.com/a/b/pull/7, https://github.com/c/d/pull/9")).toEqual([
      { url: "https://github.com/a/b/pull/7", repo: "a/b", number: 7 },
      { url: "https://github.com/c/d/pull/9", repo: "c/d", number: 9 },
    ]);
  });

  it("keep a long result within the card limits", () => {
    const card = doneCard({ runId: "r1", result: "😀".repeat(9000), prs: [], artifacts: [], acceptance: [] });
    expect(card).not.toBeNull();
    expect(card?.fallback.length).toBeLessThanOrEqual(4000);
    expect(card?.title.length).toBeLessThanOrEqual(200);
  });
});

describe("1.1 runs", () => {
  it("tell a CLI on a machine that reads acceptance blocks how to write them", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["acceptance"]);
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev ship it", mentions: [w.bot.id] });
    const assign = await d.next((f) => f.t === "run.assign" && f.topicId === sent.message.topicId);
    expect(assign.prompt).toContain("```acceptance");
    expect(assign.prompt).toContain("At most 20 items");
    d.send({ t: "run.accept", runId: assign.runId });
    // What the daemon parses from the CLI's block becomes the run's 验收标准.
    const items = [{ text: "Tests pass", state: "pass", evidence: { text: "cargo test: 42 passed" } }];
    d.event(assign.runId, { type: "acceptance", items });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === assign.runId && e.data.run.acceptance);
    expect((await api(`/v1/runs/${assign.runId}`, { token: w.human.token })).body.acceptance).toEqual(items);
    // A daemon without the feature does not get the rule.
    const old = await Daemon.connect(w.machineToken, []);
    d.close();
    const sent2 = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev again", mentions: [w.bot.id] });
    const assign2 = await old.next((f) => f.t === "run.assign" && f.topicId === sent2.message.topicId);
    expect(assign2.prompt).not.toContain("```acceptance");
    c.close();
    old.close();
  });

  it("take acceptance criteria from the run event and from the run's bot, and show them with 产出 in the done card", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["acceptance"]);
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);

    const items = [
      { text: "staging 上能登录", state: "pending" },
      { text: "E2E 全过", state: "pass", evidence: { text: "48 个通过", url: "https://ci.example.com/1" } },
    ];
    // Unknown states from a newer daemon are dropped, the rest is kept.
    d.event(runId, { type: "acceptance", items: [...items, { text: "x", state: "met" }] });
    const got = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.acceptance);
    expect(got.data.run.acceptance).toEqual(items);

    // The run's bot replaces them over HTTP; nobody else can.
    const path = `/v1/runs/${runId}/acceptance`;
    const next = [{ text: "staging 上能登录", state: "pass" }, items[1]];
    expect((await api(path, { method: "PUT", token: w.human.token, body: { items: next } })).status).toBe(403);
    expect((await api(path, { method: "PUT", token: w.bot.token, body: { items: [{ text: "", state: "pass" }] } })).status).toBe(400);
    const put = await api(path, { method: "PUT", token: w.bot.token, body: { items: next } });
    expect(put.status, JSON.stringify(put.body)).toBe(200);
    expect(put.body.acceptance).toEqual(next);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.acceptance?.[0]?.state === "pass");
    expect((await audited("run.acceptance", runId)).results).toHaveLength(1);

    // A 产出 version the run made (its runId), stored the way the files feature stores it.
    const room = benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(w.gid));
    await runInDurableObject(room, async (_o, state) => {
      const sql = state.storage.sql;
      const now = Date.now();
      sql.exec(
        "INSERT INTO artifacts (id, topic_id, title, kind, current_version, created_by, created_at, updated_at) VALUES ('01JBX0ART000000000000000AM', ?, '发布说明', 'html', 2, ?, ?, ?)",
        topicId,
        w.bot.id,
        now,
        now,
      );
      for (const v of [1, 2]) {
        sql.exec(
          "INSERT INTO artifact_versions (artifact_id, version, entry, files, created_by, created_at, run_id) VALUES ('01JBX0ART000000000000000AM', ?, 'index.html', '[]', ?, ?, ?)",
          v,
          w.bot.id,
          now + v,
          runId,
        );
      }
    });

    const seq = d.done(runId, { status: "succeeded", resultText: "发好了。" });
    await d.acked(runId, seq);
    const card = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.cardV2), 5000, "done card");
    // A one-line result is the title only: no text block repeats it.
    expect(card.cardV2.title).toBe("发好了。");
    expect(card.cardV2.blocks).toEqual([
      // 接 5.1: met without a file, link or 产出 is shown as a claim.
      { type: "evidence", id: "acceptance-1", label: "staging 上能登录", state: "info", detail: PROOF_TEXT.claimed },
      { type: "evidence", id: "acceptance-2", label: "E2E 全过", state: "pass", detail: "48 个通过", url: "https://ci.example.com/1" },
      { type: "ref", id: "artifact-1", label: "发布说明（第 2 版）", target: { kind: "artifact", artifactId: "01JBX0ART000000000000000AM", version: 2 } },
    ]);
    // The criteria survive the end of the run, and an ended run takes no more.
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "done");
    expect(lastRun(c, runId)).toMatchObject({ acceptance: next, resultMessageId: card.id });
    expect((await api(path, { method: "PUT", token: w.bot.token, body: { items: [] } })).status).toBe(409);
    c.close();
    d.close();
  });

  it("pause and resume a run on a machine that can, for the requester or an owner, audited, with no stall while paused", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["pause"], "vnc://mac-mini.tailnet");
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    // The screen sharing address: owners and the run's requester only; features: owners only.
    const seen = async (m: TestMember) => (await api("/v1/machines", { token: m.token })).body.find((x: any) => x.runningRuns.includes(runId) || x.takeoverUrl);
    expect(await seen(w.owner)).toMatchObject({ features: ["pause"], takeoverUrl: "vnc://mac-mini.tailnet" });
    const forRequester = (await api("/v1/machines", { token: w.human.token })).body.filter((x: any) => x.takeoverUrl);
    expect(forRequester).toHaveLength(1);
    expect(forRequester[0].features).toBeUndefined();
    expect((await api("/v1/machines", { token: w.other.token })).body.some((x: any) => x.takeoverUrl)).toBe(false);
    const pause = (m: TestMember) => api(`/v1/runs/${runId}/pause`, { token: m.token, body: {} });
    const resume = (m: TestMember, body: unknown = {}) => api(`/v1/runs/${runId}/resume`, { token: m.token, body });

    // Another member and the bot cannot take over; the requester can, once.
    expect((await pause(w.other)).status).toBe(403);
    expect((await pause(w.bot)).status).toBe(403);
    expect((await resume(w.human)).status).toBe(409);
    const paused = await pause(w.human);
    expect(paused.status, JSON.stringify(paused.body)).toBe(200);
    expect(paused.body).toMatchObject({ status: "running", pausedBy: w.human.id });
    expect(await d.next((f) => f.t === "run.pause")).toEqual({ t: "run.pause", runId, by: w.human.id });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.pausedBy === w.human.id);
    expect((await pause(w.owner)).status).toBe(409);

    // No stall while paused, even long after the last event.
    await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - STALL_MS - 1000, runId);
    });
    // Run the alarm handler itself: a paused run schedules no alarm of its own.
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as unknown as { alarm(): Promise<void> }).alarm());
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    await new Promise((r) => setTimeout(r, 200));
    expect(botMessages(c, topicId, w.bot.id).some((m) => String(m.body).startsWith("卡住了"))).toBe(false);
    expect(lastRun(c, runId).status).toBe("running");

    // An owner hands it back with a note for the CLI; the stall clock starts again.
    expect((await resume(w.owner, { note: "x".repeat(2001) })).status).toBe(400);
    const back = await resume(w.owner, { note: "配置改好了，从第 3 步接着做" });
    expect(back.status, JSON.stringify(back.body)).toBe(200);
    expect(back.body.pausedBy).toBeUndefined();
    expect(await d.next((f) => f.t === "run.resume")).toEqual({ t: "run.resume", runId, note: "配置改好了，从第 3 步接着做" });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && !e.data.run.pausedBy && e.data.run.updatedAt > paused.body.updatedAt);
    const lastEvent = await runInDurableObject(deskOf(w.bot.id), async (_o, state) =>
      state.storage.sql.exec<{ at: number }>("SELECT last_event_at AS at FROM runs WHERE id = ?", runId).one().at,
    );
    expect(Date.now() - lastEvent).toBeLessThan(STALL_MS);

    const pauses = await audited("run.pause", runId);
    const resumes = await audited("run.resume", runId);
    expect(pauses.results.map((r) => r.actor_id)).toEqual([w.human.id]);
    expect(resumes.results.map((r) => r.actor_id)).toEqual([w.owner.id]);
    // The note goes to the CLI, not into the audit log.
    expect(resumes.results[0].details).not.toContain("配置");

    // A cancel while paused ends the run without pausedBy.
    expect((await pause(w.human)).status).toBe(200);
    const stopped = await api(`/v1/runs/${runId}/cancel`, { token: w.human.token, body: {} });
    expect(stopped.status).toBe(200);
    expect(stopped.body.status).toBe("cancelled");
    expect(stopped.body.pausedBy).toBeUndefined();
    expect((await pause(w.human)).status).toBe(409);
    c.close();
    d.close();
  });

  it("clear pausedBy when the daemon reports the CLI going on by itself", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["pause"]);
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    const paused = await api(`/v1/runs/${runId}/pause`, { token: w.human.token, body: {} });
    expect(paused.body).toMatchObject({ pausedBy: w.human.id });
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    // The daemon restarted the CLI: it goes on without a resume from the server.
    const seq = d.event(runId, { type: "paused", paused: false });
    await d.acked(runId, seq);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && !e.data.run.pausedBy && e.data.run.updatedAt > paused.body.updatedAt);
    const got = await api(`/v1/runs/${runId}`, { token: w.human.token });
    expect(got.status).toBe(200);
    expect(got.body.pausedBy).toBeUndefined();
    // It can be taken over again.
    expect((await api(`/v1/runs/${runId}/pause`, { token: w.human.token, body: {} })).status).toBe(200);
    c.close();
    d.close();
  });

  it("keep a newer pause when the paused: false of an earlier resume arrives late (SB-16)", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["pause"]);
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    const pause = () => api(`/v1/runs/${runId}/pause`, { token: w.human.token, body: {} });
    const resume = () => api(`/v1/runs/${runId}/resume`, { token: w.human.token, body: {} });
    expect((await pause()).status).toBe(200);
    await d.next((f) => f.t === "run.pause");
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    expect((await resume()).status).toBe(200);
    await d.next((f) => f.t === "run.resume");
    // A second pause before the daemon reports the first resume.
    const again = await pause();
    expect(again.status).toBe(200);
    await d.next((f) => f.t === "run.pause");
    // The daemon's answer to the resume arrives now, then its answer to the new pause.
    const late = d.event(runId, { type: "paused", paused: false });
    await d.acked(runId, late);
    const got = await api(`/v1/runs/${runId}`, { token: w.human.token });
    expect(got.body.pausedBy).toBe(w.human.id);
    const seq = d.event(runId, { type: "paused", paused: true, by: w.human.id });
    await d.acked(runId, seq);
    // Still paused: no stall, and the requester can hand it back.
    await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - STALL_MS - 1000, runId);
    });
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as unknown as { alarm(): Promise<void> }).alarm());
    expect(botMessages(c, topicId, w.bot.id).some((m) => String(m.body).startsWith("卡住了"))).toBe(false);
    const back = await resume();
    expect(back.status, JSON.stringify(back.body)).toBe(200);
    expect(back.body.pausedBy).toBeUndefined();
    // After the daemon took every pause, a paused: false clears pausedBy again.
    expect((await pause()).status).toBe(200);
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    const on = d.event(runId, { type: "paused", paused: false });
    await d.acked(runId, on);
    expect((await api(`/v1/runs/${runId}`, { token: w.human.token })).body.pausedBy).toBeUndefined();
    c.close();
    d.close();
  });

  it("clear pausedBy on paused: false after the daemon skipped a pause because the run was still stopped (SB-16)", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["pause"]);
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    const pause = () => api(`/v1/runs/${runId}/pause`, { token: w.human.token, body: {} });
    expect((await pause()).status).toBe(200);
    await d.next((f) => f.t === "run.pause");
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    // The resume reaches the server but the daemon never acts on it: the CLI stays stopped.
    expect((await api(`/v1/runs/${runId}/resume`, { token: w.human.token, body: {} })).status).toBe(200);
    await d.next((f) => f.t === "run.resume");
    // The next pause finds the CLI already stopped; the daemon reports the pause again.
    expect((await pause()).status).toBe(200);
    await d.next((f) => f.t === "run.pause");
    d.event(runId, { type: "paused", paused: true, by: w.human.id });
    // The daemon then goes on by itself (for example after a restart of the CLI).
    const on = d.event(runId, { type: "paused", paused: false });
    await d.acked(runId, on);
    const got = await api(`/v1/runs/${runId}`, { token: w.human.token });
    expect(got.status).toBe(200);
    expect(got.body.pausedBy).toBeUndefined();
    expect((await pause()).status).toBe(200);
    c.close();
    d.close();
  });

  it("retry an ended run with the same request, for the requester or an owner, audited", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, []);
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    const retry = (m: TestMember, id = runId) => api(`/v1/runs/${id}/retry`, { token: m.token, body: {} });
    // Not while it runs.
    expect((await retry(w.human)).status).toBe(409);
    const seq = d.done(runId, { status: "failed", error: "tests failed" });
    await d.acked(runId, seq);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "failed");
    expect((await retry(w.other)).status).toBe(403);
    expect((await retry(w.bot)).status).toBe(403);
    const again = await retry(w.human);
    expect(again.status, JSON.stringify(again.body)).toBe(200);
    expect(again.body).toMatchObject({ retryOf: runId, topicId, requestedBy: w.human.id });
    expect(again.body.id).not.toBe(runId);
    const assign = await d.next((f) => f.t === "run.assign" && f.runId === again.body.id);
    expect(assign.prompt).toContain("@dev ship it");
    const got = await api(`/v1/runs/${again.body.id}`, { token: w.human.token });
    expect(got.body.retryOf).toBe(runId);
    // One active run per topic: no second retry while the first one runs.
    expect((await retry(w.owner)).status).toBe(409);
    const audits = await audited("run.retry", runId);
    expect(audits.results.map((r) => r.actor_id)).toEqual([w.human.id]);
    // An owner retries too, once the retry ended.
    d.send({ t: "run.accept", runId: again.body.id });
    const seq2 = d.done(again.body.id, { status: "failed", error: "again" });
    await d.acked(again.body.id, seq2);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === again.body.id && e.data.run.status === "failed");
    // A run made before requests were kept: the request is read from its message.
    await runInDurableObject(deskOf(w.bot.id), async (_o, state) => {
      state.storage.sql.exec("DELETE FROM meta WHERE key = ?", `request:${runId}`);
    });
    const third = await retry(w.owner, runId);
    expect(third.status, JSON.stringify(third.body)).toBe(200);
    expect(third.body).toMatchObject({ retryOf: runId, requestedBy: w.human.id });
    const assign3 = await d.next((f) => f.t === "run.assign" && f.runId === third.body.id);
    expect(assign3.prompt).toContain("@dev ship it");
    c.close();
    d.close();
  });

  it("let a run's daemon upload files and make 产出 as the runner member, shown as proof in the done card", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["output"]);
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    const page = new TextEncoder().encode("<!doctype html><p>看板</p>");
    const sha = async (b: Uint8Array) => [...new Uint8Array(await crypto.subtle.digest("SHA-256", b))].map((x) => x.toString(16).padStart(2, "0")).join("");
    const put = (token: string, bytes: Uint8Array, hash: string, id = runId) =>
      SELF.fetch(`${BASE}/v1/machine/runs/${id}/files`, {
        method: "PUT",
        headers: { authorization: `Bearer ${token}`, "content-type": "text/html", "content-length": String(bytes.length), "x-file-name": encodeURIComponent("首页.html"), "x-file-sha256": hash },
        body: bytes,
      });
    const post = (path: string, body: unknown, token = w.machineToken) =>
      SELF.fetch(`${BASE}/v1/machine/runs/${runId}${path}`, { method: "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body: JSON.stringify(body) });

    // A wrong hash stores nothing; another machine cannot write for this run.
    expect((await put(w.machineToken, page, "0".repeat(64))).status).toBe(400);
    const other = await api("/v1/machines", { token: w.owner.token, body: { name: "other", labels: ["x"] } });
    expect((await put(other.body.token, page, await sha(page))).status).toBe(404);
    const up = await put(w.machineToken, page, await sha(page));
    expect(up.status).toBe(200);
    const file = (await up.json()) as any;
    expect(file).toMatchObject({ name: "首页.html", status: "ready", createdBy: w.bot.id, topicId, groupId: w.gid, mime: "text/html" });

    const made = await post("/artifacts", { title: "看板", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: file.id }] });
    expect(made.status, await made.clone().text()).toBe(200);
    const art = (await made.json()) as any;
    expect(art.artifact).toMatchObject({ topicId, createdBy: w.bot.id });
    expect(art.version).toMatchObject({ version: 1, runId });
    const v2 = await post(`/artifacts/${art.artifact.id}/versions`, { entry: "index.html", files: [{ path: "index.html", fileId: file.id }], note: "第二版" });
    expect(v2.status, await v2.clone().text()).toBe(200);
    expect(((await v2.json()) as any).version).toMatchObject({ version: 2, runId });
    // The writes are the member's, through the machine.
    const audit = await env.DB.prepare("SELECT actor_id, credential_id FROM audit WHERE action = 'artifact.create' AND target = ?").bind(art.artifact.id).first<any>();
    expect(audit).toMatchObject({ actor_id: w.bot.id });
    expect(audit.credential_id).toMatch(/^machine:/);

    const seq = d.done(runId, { status: "succeeded", resultText: "看板做好了。" });
    await d.acked(runId, seq);
    const card = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.cardV2), 5000, "done card");
    expect(card.cardV2.blocks).toContainEqual({ type: "ref", id: "artifact-1", label: "看板（第 2 版）", target: { kind: "artifact", artifactId: art.artifact.id, version: 2 } });
    // An ended run writes nothing more.
    const late = await put(w.machineToken, page, await sha(page));
    expect(late.status).toBe(409);
    expect(((await late.json()) as any).error.reason).toBe("run.ended");
    c.close();
    d.close();
  });

  it("refuse to pause a run whose machine cannot pause", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, []);
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    const res = await api(`/v1/runs/${runId}/pause`, { token: w.owner.token, body: {} });
    expect(res.status).toBe(409);
    await new Promise((r) => setTimeout(r, 100));
    expect(d.frames.some((f) => f.t === "run.pause")).toBe(false);
    c.close();
    d.close();
  });

  it("answer a side chat with a read-only run in the asker's DM, from the topic up to the asker's point", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["aside"]);
    const { c: cs } = await Client.connect(w.other.token);
    // The source topic, with a message after the asker's point.
    const src = await cs.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "发布卡在签名上" });
    await cs.ok({ t: "send", groupId: w.gid, topicId: src.topic.id, clientMsgId: clientMsgId(), body: "签名问题已修好" });
    const later = await cs.ok({ t: "send", groupId: w.gid, topicId: src.topic.id, clientMsgId: clientMsgId(), body: "这是之后的消息" });
    const context = { groupId: w.gid, topicId: src.topic.id, uptoSeq: later.message.createdSeq - 1 };

    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    expect(dm.status, JSON.stringify(dm.body)).toBe(200);
    const dmId = dm.body.group.id as string;
    const { c } = await Client.connect(w.human.token);
    const side = await c.ok({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "现在卡在哪？", context });
    const sideTopic = side.topic.id as string;

    const assign = await d.next((f) => f.t === "run.assign" && f.topicId === sideTopic);
    expect(assign).toMatchObject({ groupId: dmId, kind: "aside" });
    expect(assign.prompt).toContain("发布卡在签名上");
    expect(assign.prompt).toContain("签名问题已修好");
    expect(assign.prompt).not.toContain("这是之后的消息");
    expect(assign.prompt).toContain("现在卡在哪？");
    const runId = assign.runId as string;
    d.send({ t: "run.accept", runId });
    const ack = await c.waitEvent((e) => e.type === "reaction.changed" && e.data.messageId === side.message.id);
    expect(ack.data.emoji).toBe(ACK_EMOJI);
    const seq = d.done(runId, { status: "succeeded", resultText: "已经修好了。可以在话题里说：\n> 签名修好了，可以继续发布。" });
    await d.acked(runId, seq);

    // The answer is a plain message in the DM, for the asker only.
    const answer = await waitUntil(() => botMessages(c, sideTopic, w.bot.id)[0], 5000, "side chat answer");
    expect(answer.body).toContain("签名修好了");
    expect(answer.cardV2).toBeUndefined();
    const run = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "done");
    expect(run.data.run).toMatchObject({ kind: "aside", groupId: dmId, resultMessageId: answer.id });
    expect((await api(`/v1/runs/${runId}`, { token: w.human.token })).status).toBe(200);
    expect((await api(`/v1/runs/${runId}`, { token: w.owner.token })).status).toBe(404);
    expect((await api(`/v1/runs/${runId}/events`, { token: w.other.token })).status).toBe(404);
    // Nothing reached the source topic, and the side chat keeps no work status.
    await new Promise((r) => setTimeout(r, 200));
    expect(botMessages(cs, src.topic.id, w.bot.id)).toEqual([]);
    expect(c.events().some((e) => e.type === "topic.updated" && e.data.topicId === sideTopic && e.data.status)).toBe(false);
    c.close();
    cs.close();
    d.close();
  });

  it("never give a side chat run to a daemon that cannot run it read-only", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["pause"]);
    const { c: cs } = await Client.connect(w.other.token);
    const src = await cs.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "话题" });
    const dm = await api("/v1/dms", { token: w.human.token, body: { memberId: w.bot.id } });
    const dmId = dm.body.group.id as string;
    const { c } = await Client.connect(w.human.token);
    const context = { groupId: w.gid, topicId: src.topic.id, uptoSeq: src.message.createdSeq };
    const side = await c.ok({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "问一下", context });
    await waitUntil(() => botMessages(c, side.topic.id, w.bot.id).some((m) => m.body === TEXT.noAsideMachine), 5000, "no machine note");
    expect(d.frames.some((f) => f.t === "run.assign")).toBe(false);
    // A normal DM topic still gets a normal run on the same machine.
    const plain = await c.ok({ t: "send", groupId: dmId, clientMsgId: clientMsgId(), body: "普通的问题" });
    const assign = await d.next((f) => f.t === "run.assign" && f.topicId === plain.topic.id);
    expect(assign.kind).toBeUndefined();
    c.close();
    cs.close();
    d.close();
  });
});

describe("desk note from a run (chat-run desk note)", () => {
  it("sets, keeps and clears the bot's desk note in the run's group; a bad body is refused", async () => {
    const w = await world();
    const d = await Daemon.connect(w.machineToken, ["output"]);
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    const out = async (outputId: string, body: unknown) => {
      d.event(runId, { type: "output", outputId, op: "desk.note", body });
      return d.next((f) => f.t === "run.output.result" && f.runId === runId && f.outputId === outputId);
    };
    const set = await out("o1", { text: "  今天先把 1.1 发出去 " });
    expect(set, JSON.stringify(set)).toMatchObject({ ok: true, result: { seq: expect.any(Number) } });
    const noted = await c.waitEvent((e) => e.type === "desk.noted" && e.seq === set.result.seq);
    expect(noted).toMatchObject({ groupId: w.gid, actorId: w.bot.id, data: { text: "今天先把 1.1 发出去" } });
    expect(await out("o2", { text: "今天先把 1.1 发出去" })).toMatchObject({ ok: true, result: { seq: null } });
    expect(await out("o3", { text: 5 })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await out("o4", { text: "长".repeat(141) })).toMatchObject({ ok: false, error: { code: "invalid" } });
    const list = await api(`/v1/groups/${w.gid}/desk-notes`, { token: w.human.token });
    expect(list.body.notes).toEqual([{ memberId: w.bot.id, text: "今天先把 1.1 发出去", at: expect.any(Number), seq: set.result.seq }]);
    expect(await out("o5", { text: "" })).toMatchObject({ ok: true, result: { seq: expect.any(Number) } });
    expect((await api(`/v1/groups/${w.gid}/desk-notes`, { token: w.human.token })).body.notes).toEqual([]);
    c.close();
    d.close();
  });
});
