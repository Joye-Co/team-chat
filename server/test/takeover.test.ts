// Usable take-over as a state machine on the server (design-takeover T2, T3): the run phase,
// pause versus take-over and who may do which, the hold on the machine link, reminders, the
// release when the holder leaves, the two-minute rule, a daemon restart under a hold, and a
// person's commands (routes, hand.* events, who sees the output, audit), against a fake daemon.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { HOLD_REMIND_MS, HOLD_TEXT, mayPause, mayResume, mayTakeOver, takeoverOf } from "../src/bots/desk/hold";
import { type PhaseRow, phaseOf } from "../src/bots/desk/phase";
import { DAEMON_RESTARTED } from "../src/agent/recovery";
import { TEXT } from "../src/bots/dispatch";
import { Daemon, downViolations } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const linkOf = (machineId: string) => benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId));

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const TAKEOVER = ["pause", "takeover"];
const ISOLATED = ["pause", "takeover", "takeover-isolated"];

interface World {
  owner: TestMember;
  human: TestMember;
  other: TestMember;
  bot: TestMember;
  gid: string;
  machineToken: string;
  machineId: string;
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
  return { owner, human, other, bot, gid, machineToken: m.body.token, machineId: m.body.machine.id };
}

/** The human mentions the bot in a new topic; the daemon accepts the run unless `accept` is false. */
async function startRun(w: World, c: Client, d: Daemon, accept = true): Promise<{ topicId: string; runId: string }> {
  const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev ship it", mentions: [w.bot.id] });
  const topicId = sent.message.topicId as string;
  const { runId } = await d.next((f) => f.t === "run.assign" && f.topicId === topicId);
  if (accept) {
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
  }
  return { topicId, runId };
}

const hold = (m: TestMember, runId: string, what: "pause" | "takeover" | "resume", body: unknown = {}) => api(`/v1/runs/${runId}/${what}`, { token: m.token, body });
const command = (m: TestMember, runId: string, text: string) => api(`/v1/runs/${runId}/commands`, { token: m.token, body: { command: text } });
const getRun = async (m: TestMember, runId: string) => (await api(`/v1/runs/${runId}`, { token: m.token })).body;

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

async function audited(action: string, target: string) {
  return (
    await env.DB.prepare("SELECT actor_id, details FROM audit WHERE action = ? AND target = ? ORDER BY id")
      .bind(action, target)
      .all<{ actor_id: string; details: string }>()
  ).results;
}

const deskRow = (botId: string, runId: string) =>
  runInDurableObject(deskOf(botId), (_o, state) => state.storage.sql.exec<Record<string, SqlStorageValue>>("SELECT * FROM runs WHERE id = ?", runId).one());
const linkRow = (machineId: string, runId: string) =>
  runInDurableObject(linkOf(machineId), (_o, state) => state.storage.sql.exec<Record<string, SqlStorageValue>>("SELECT * FROM runs WHERE run_id = ?", runId).one());

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

const ROW: PhaseRow = { status: "running", ended_at: null, paused_by: null, hold_kind: null, away_since: null, queued_at: null, machine_id: "m1", approval_id: null };

describe("phaseOf (design-takeover 3.1)", () => {
  it("give an ended run stopped, needs_reply, done or failed", () => {
    const ended = { ...ROW, ended_at: 1 };
    expect(phaseOf({ ...ended, status: "cancelled" })).toEqual({ phase: "stopped" });
    const waiting = { state: "waiting" as const, text: "need the key" };
    expect(phaseOf({ ...ended, status: "done" }, { report: waiting })).toEqual({ phase: "needs_reply", waitingFor: "report" });
    expect(phaseOf({ ...ended, status: "done" }, { report: { state: "blocked", text: "no access" } })).toEqual({ phase: "needs_reply", waitingFor: "stuck" });
    expect(phaseOf({ ...ended, status: "done" }, { report: waiting, superseded: true })).toEqual({ phase: "done" });
    expect(phaseOf({ ...ended, status: "done" })).toEqual({ phase: "done" });
    expect(phaseOf({ ...ended, status: "failed" }, { report: waiting })).toEqual({ phase: "failed" });
    expect(phaseOf({ ...ended, status: "blocked" })).toEqual({ phase: "failed" });
    // An ended run is never held, whatever the row says.
    expect(phaseOf({ ...ended, status: "done", paused_by: "p" })).toEqual({ phase: "done" });
  });

  it("give a held run paused or taken_over, with offline when its machine is away", () => {
    expect(phaseOf({ ...ROW, paused_by: "p", hold_kind: "pause" })).toEqual({ phase: "paused" });
    // Rows from before take-over have no hold kind: a pause.
    expect(phaseOf({ ...ROW, paused_by: "p", hold_kind: null })).toEqual({ phase: "paused" });
    expect(phaseOf({ ...ROW, paused_by: "p", hold_kind: "takeover" })).toEqual({ phase: "taken_over" });
    expect(phaseOf({ ...ROW, paused_by: "p", hold_kind: "takeover", away_since: 5 })).toEqual({ phase: "taken_over", waitingFor: "offline" });
    // A hold wins over an approval and a question.
    expect(phaseOf({ ...ROW, status: "waiting", approval_id: "a", paused_by: "p" })).toEqual({ phase: "paused" });
  });

  it("give waiting_computer for a queue, an away machine and a run being prepared", () => {
    expect(phaseOf({ ...ROW, status: "created", machine_id: null, queued_at: 1 })).toEqual({ phase: "waiting_computer", waitingFor: "queue" });
    expect(phaseOf({ ...ROW, away_since: 1 })).toEqual({ phase: "waiting_computer", waitingFor: "offline" });
    expect(phaseOf({ ...ROW, status: "created" })).toEqual({ phase: "waiting_computer", waitingFor: "booting" });
  });

  it("give waiting_approval, waiting_person and working", () => {
    expect(phaseOf({ ...ROW, status: "waiting", approval_id: "a" })).toEqual({ phase: "waiting_approval" });
    expect(phaseOf({ ...ROW, status: "waiting" })).toEqual({ phase: "waiting_person", waitingFor: "question" });
    expect(phaseOf({ ...ROW, status: "blocked" })).toEqual({ phase: "waiting_person", waitingFor: "stuck" });
    expect(phaseOf(ROW)).toEqual({ phase: "working" });
    // A run with no machine yet that is not queued (an external run) works.
    expect(phaseOf({ ...ROW, status: "created", machine_id: null })).toEqual({ phase: "working" });
  });
});

describe("hold rights (design-takeover 3.2)", () => {
  const admin = { id: "adm", kind: "human" as const, level: "owner" as const };
  const req = { id: "req", kind: "human" as const, level: "member" as const };
  const mine = { id: "own", kind: "human" as const, level: "member" as const };
  const bot = { id: "bot", kind: "bot" as const, level: "member" as const };
  const team = (features: string[]) => ({ info: { features } as never });
  const personal = (features: string[]) => ({ ownerId: "own", info: { features } as never });

  it("say how a machine can be taken over", () => {
    expect(takeoverOf(team(["pause"]))).toBe("none");
    expect(takeoverOf(team(TAKEOVER))).toBe("commands");
    expect(takeoverOf(team(ISOLATED))).toBe("isolated");
    expect(takeoverOf(undefined)).toBe("none");
  });

  it("let the requester pause, and the machine's owner or (team) an admin", () => {
    expect(mayPause(req, "req", team([]))).toBe(true);
    expect(mayPause(admin, "req", team([]))).toBe(true);
    expect(mayPause(mine, "req", team([]))).toBe(false);
    expect(mayPause(admin, "req", personal([]))).toBe(false);
    expect(mayPause(mine, "req", personal([]))).toBe(true);
    expect(mayPause(bot, "bot", team([]))).toBe(false);
  });

  it("let only the owner take over on a personal machine, only the requester on an isolated team machine", () => {
    expect(mayTakeOver(mine, "req", personal(TAKEOVER))).toBe(true);
    expect(mayTakeOver(req, "req", personal(TAKEOVER))).toBe(false);
    expect(mayTakeOver(admin, "req", personal(TAKEOVER))).toBe(false);
    expect(mayTakeOver(mine, "req", personal(["pause"]))).toBe(false);
    expect(mayTakeOver(req, "req", team(TAKEOVER))).toBe(false);
    expect(mayTakeOver(req, "req", team(ISOLATED))).toBe(true);
    expect(mayTakeOver(admin, "req", team(ISOLATED))).toBe(false);
    expect(mayTakeOver(admin, "adm", team(ISOLATED))).toBe(true);
    // A person alone in the team: also on a team machine that is not isolated, never without the feature.
    expect(mayTakeOver(req, "req", team(TAKEOVER), true)).toBe(true);
    expect(mayTakeOver(req, "req", team(["pause"]), true)).toBe(false);
    expect(mayTakeOver(bot, "bot", team(TAKEOVER), true)).toBe(false);
  });

  it("let the holder hand back, and an admin for him on a team machine only", () => {
    const takeover = { by: "req", kind: "takeover" as const };
    expect(mayResume(req, takeover, "req", team(ISOLATED))).toBe(true);
    expect(mayResume(admin, takeover, "req", team(ISOLATED))).toBe(true);
    expect(mayResume(admin, { by: "own", kind: "takeover" }, "req", personal(TAKEOVER))).toBe(false);
    expect(mayResume(req, { by: "own", kind: "takeover" }, "req", personal(TAKEOVER))).toBe(false);
    const pause = { by: "own", kind: "pause" as const };
    expect(mayResume(req, pause, "req", personal([]))).toBe(true);
    expect(mayResume(admin, pause, "req", personal([]))).toBe(false);
    expect(mayResume(admin, { by: "req", kind: "pause" }, "req", team([]))).toBe(true);
    expect(mayResume(mine, { by: "req", kind: "pause" }, "req", team([]))).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// The hold on a real desk and link
// ---------------------------------------------------------------------------

describe("take-over (design-takeover T2)", () => {
  it("take over on an isolated team machine: requester only, phase, hold frame, audit; then hand back", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    expect(await getRun(w.human, runId)).toMatchObject({ phase: "working", takeover: "isolated" });

    expect((await hold(w.other, runId, "takeover")).status).toBe(403);
    expect((await hold(w.owner, runId, "takeover")).status).toBe(403);
    expect((await hold(w.bot, runId, "takeover")).status).toBe(403);
    const took = await hold(w.human, runId, "takeover");
    expect(took.status, JSON.stringify(took.body)).toBe(200);
    expect(took.body).toMatchObject({ status: "running", pausedBy: w.human.id, phase: "taken_over", holdKind: "takeover", takeover: "isolated" });
    expect(took.body.heldAt).toBeTypeOf("number");
    expect(await d.next((f) => f.t === "run.pause")).toEqual({ t: "run.pause", runId, by: w.human.id, hold: "takeover" });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.phase === "taken_over");
    expect((await linkRow(w.machineId, runId)).held).toBe(1);
    // Again by the holder: the run as it is; a pause on top of it: conflict.
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    expect((await hold(w.human, runId, "pause")).status).toBe(409);
    expect((await audited("run.takeover", runId)).map((x) => x.actor_id)).toEqual([w.human.id, w.human.id]);

    // Another member cannot hand it back; an admin can, for the holder (audited).
    expect((await hold(w.other, runId, "resume")).status).toBe(403);
    const back = await hold(w.owner, runId, "resume", { note: "done here" });
    expect(back.status, JSON.stringify(back.body)).toBe(200);
    expect(back.body).toMatchObject({ phase: "working" });
    expect(back.body.pausedBy).toBeUndefined();
    expect(back.body.holdKind).toBeUndefined();
    expect(await d.next((f) => f.t === "run.resume")).toEqual({ t: "run.resume", runId, note: "done here" });
    expect((await linkRow(w.machineId, runId)).held).toBe(0);
    const resumes = await audited("run.resume", runId);
    expect(resumes.at(-1)).toMatchObject({ actor_id: w.owner.id });
    expect(JSON.parse(resumes.at(-1)?.details ?? "{}")).toMatchObject({ for: w.human.id, holdKind: "takeover" });
    expect((await deskRow(w.bot.id, runId)).handed_back_at).toBeTypeOf("number");
    c.close();
    d.close();
  });

  it("refuse take-over without the feature or on a team machine that is not isolated, and upgrade a pause", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["pause"], version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    expect(await getRun(w.human, runId)).toMatchObject({ takeover: "none" });
    const old = await hold(w.human, runId, "takeover");
    expect(old.status).toBe(409);
    expect(old.body.error.reason).toBe("run.no_takeover");
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");

    // The daemon comes back with `takeover`, not isolated: a team Mac gives no shell to anyone.
    const { d: d2 } = await Daemon.connect(w.machineToken, { features: TAKEOVER, version: "1.2.0", runs: [{ runId, lastSeq: 0, done: false }] });
    expect((await hold(w.human, runId, "takeover")).status).toBe(403);
    expect(await getRun(w.human, runId)).toMatchObject({ takeover: "commands" });
    d2.close();
    await waitUntil(() => d2.closed, 5000, "closed");

    // Isolated: a pause, then a take-over upgrades it and keeps when the hold started.
    const { d: d3 } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0", runs: [{ runId, lastSeq: 0, done: false }] });
    const paused = await hold(w.human, runId, "pause");
    expect(paused.body).toMatchObject({ phase: "paused", holdKind: "pause" });
    expect(await d3.next((f) => f.t === "run.pause")).toEqual({ t: "run.pause", runId, by: w.human.id });
    const up = await hold(w.human, runId, "takeover");
    expect(up.status, JSON.stringify(up.body)).toBe(200);
    expect(up.body).toMatchObject({ phase: "taken_over", holdKind: "takeover", heldAt: paused.body.heldAt });
    await d3.next((f) => f.t === "run.pause" && f.hold === "takeover");
    expect((await deskRow(w.bot.id, runId)).pause_gen).toBe(2);
    c.close();
    d3.close();
  });

  it("let a person alone in the team take over on a team machine that is not isolated", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: TAKEOVER, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(403);
    // Everyone else leaves: no one else's credentials are on the machine any more.
    const others = await env.DB.prepare("SELECT id FROM members WHERE kind = 'human' AND deactivated = 0 AND id != ?").bind(w.human.id).all<{ id: string }>();
    const ids = others.results.map((x) => x.id);
    const set = (v: number) => Promise.all(ids.map((id) => env.DB.prepare("UPDATE members SET deactivated = ? WHERE id = ?").bind(v, id).run()));
    await set(1);
    try {
      const took = await hold(w.human, runId, "takeover");
      expect(took.status, JSON.stringify(took.body)).toBe(200);
      expect(took.body).toMatchObject({ phase: "taken_over", holdKind: "takeover", takeover: "commands", pausedBy: w.human.id });
      await d.next((f) => f.t === "run.pause" && f.hold === "takeover");
    } finally {
      await set(0);
    }
    c.close();
    d.close();
  });

  it("follow the machine owner on a personal machine running someone else's run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: TAKEOVER, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    // Bob's personal machine runs Alice's run.
    await env.DB.prepare("UPDATE machines SET owner_id = ? WHERE id = ?").bind(w.other.id, w.machineId).run();
    expect((await hold(w.human, runId, "takeover")).status).toBe(403);
    expect((await hold(w.owner, runId, "pause")).status).toBe(403);
    // Alice pauses; Bob takes over from her pause; the admin cannot hand back on a personal machine.
    expect((await hold(w.human, runId, "pause")).status).toBe(200);
    const took = await hold(w.other, runId, "takeover");
    expect(took.status, JSON.stringify(took.body)).toBe(200);
    expect(took.body).toMatchObject({ pausedBy: w.other.id, phase: "taken_over", takeover: "commands" });
    expect((await hold(w.owner, runId, "resume")).status).toBe(403);
    expect((await hold(w.human, runId, "resume")).status).toBe(403);
    expect((await hold(w.other, runId, "resume")).status).toBe(200);
    c.close();
    d.close();
  });

  it("refuse to hold a run whose computer is not ready, or a run that ended", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d, false);
    expect(await getRun(w.human, runId)).toMatchObject({ phase: "waiting_computer", waitingFor: "booting" });
    for (const what of ["pause", "takeover"] as const) {
      const res = await hold(w.human, runId, what);
      expect(res.status).toBe(409);
      expect(res.body.error.reason).toBe("run.not_ready");
    }
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
    const seq = d.done(runId, { status: "succeeded", resultText: "ok" });
    await d.acked(runId, seq);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.phase === "done");
    expect((await hold(w.human, runId, "takeover")).status).toBe(409);
    expect((await hold(w.human, runId, "resume")).status).toBe(409);
    c.close();
    d.close();
  });

  it("keep a held run when its machine goes away and its lease runs out", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    // Long past the lease: the link keeps it and asks nobody.
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET lease_until = ? WHERE run_id = ?", Date.now() - 1, runId);
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    expect((await linkRow(w.machineId, runId)).state).toBe("accepted");
    // Even when the lease ends some other way, the desk keeps a held run.
    expect(await deskOf(w.bot.id).runLost(runId, w.machineId)).toBe(true);
    // The machine stayed away past the grace: the phase stays, with offline.
    await deskOf(w.bot.id).heldMachine(runId, w.machineId, false);
    expect(await getRun(w.human, runId)).toMatchObject({ phase: "taken_over", waitingFor: "offline", pausedBy: w.human.id });
    // A command is never queued for a machine that is away.
    const cmd = await command(w.human, runId, "git status");
    expect(cmd.status).toBe(409);
    expect(cmd.body.error.reason).toBe("hand.offline");
    const outbox = await runInDurableObject(linkOf(w.machineId), (_o, state) => state.storage.sql.exec("SELECT frame FROM outbox").toArray());
    expect(outbox.some((x) => String(x.frame).includes("run.command"))).toBe(false);
    // Back: the hold stands, no longer offline; the link sees the hold too.
    const { d: d2 } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0", runs: [{ runId, lastSeq: 0, done: false }] });
    for (let i = 0; i < 100 && (await getRun(w.human, runId)).waitingFor !== undefined; i++) await new Promise((r) => setTimeout(r, 20));
    expect(await getRun(w.human, runId)).toMatchObject({ phase: "taken_over" });
    // Replayed frames of a held run say nothing in the topic.
    const replayed = d2.event(runId, { type: "paused", paused: true, by: w.human.id });
    await d2.acked(runId, replayed);
    await deskOf(w.bot.id).heldMachine(runId, w.machineId, false);
    const again = d2.event(runId, { type: "paused", paused: true, by: w.human.id });
    await d2.acked(runId, again);
    expect(botMessages(c, topicId, w.bot.id).some((m) => m.body === TEXT.machineBack("mac-mini"))).toBe(false);
    await deskOf(w.bot.id).heldMachine(runId, w.machineId, true);
    // Handed back: the link gives it a lease again.
    expect((await hold(w.human, runId, "resume")).status).toBe(200);
    const row = await linkRow(w.machineId, runId);
    expect(row.held).toBe(0);
    expect(row.lease_until as number).toBeGreaterThan(Date.now());
    c.close();
    d2.close();
  });

  it("remind the holder after 2 hours, then everyone after 24, without stalling the run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c: mine } = await Client.connect(w.human.token);
    const { c } = await Client.connect(w.owner.token);
    const { topicId, runId } = await startRun(w, mine, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    const ago = (ms: number) =>
      runInDurableObject(deskOf(w.bot.id), (_o, state) => {
        state.storage.sql.exec("UPDATE runs SET paused_at = ?, last_event_at = ? WHERE id = ?", Date.now() - ms, Date.now() - ms, runId);
      });
    await ago(HOLD_REMIND_MS[0] + 1000);
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as unknown as { alarm(): Promise<void> }).alarm());
    const first = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body === HOLD_TEXT.remind("dev")), 5000, "2 h reminder");
    expect(first.mentions).toEqual([w.human.id]);
    // Not again at the next alarm.
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as unknown as { alarm(): Promise<void> }).alarm());
    await ago(HOLD_REMIND_MS[1] + 1000);
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as unknown as { alarm(): Promise<void> }).alarm());
    const all = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body === HOLD_TEXT.remindAll("Alice", "dev", 24, "takeover")), 5000, "24 h reminder");
    expect([...all.mentions].sort()).toEqual([w.human.id, w.owner.id].sort());
    await new Promise((r) => setTimeout(r, 100));
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.body === HOLD_TEXT.remind("dev"))).toHaveLength(1);
    // Still held, never stalled.
    expect(await getRun(w.human, runId)).toMatchObject({ status: "running", phase: "taken_over" });
    expect((await deskRow(w.bot.id, runId)).hold_reminded).toBe(2);
    mine.close();
    c.close();
    d.close();
  });

  it("hand back automatically when the holder leaves the group", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c: mine } = await Client.connect(w.human.token);
    const { c } = await Client.connect(w.owner.token);
    const { topicId, runId } = await startRun(w, mine, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    const left = await api(`/v1/groups/${w.gid}/members/${w.human.id}`, { method: "DELETE", token: w.owner.token });
    expect(left.status, JSON.stringify(left.body)).toBe(200);
    const resume = await d.next((f) => f.t === "run.resume" && f.runId === runId);
    expect(resume.note).toContain("left the team");
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === HOLD_TEXT.left("Alice")), 5000, "left note");
    const run = await deskOf(w.bot.id).getRun(w.bot.id, runId);
    expect(run?.pausedBy).toBeUndefined();
    expect(run?.phase).toBe("working");
    mine.close();
    c.close();
    d.close();
  });

  it("go on in the session when the CLI fails right after a hand-back", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    expect((await hold(w.human, runId, "resume")).status).toBe(200);
    const seq = d.done(runId, { status: "failed", error: "Connection error" });
    await d.acked(runId, seq);
    const next = await d.next((f) => f.t === "run.assign" && f.topicId === topicId && f.runId !== runId);
    expect(next.runId).not.toBe(runId);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === HOLD_TEXT.retried("dev")), 5000, "retry note");
    const old = await getRun(w.human, runId);
    expect(old.phase).not.toBe("failed");
    c.close();
    d.close();
  });

  it("end the hold when the daemon restarts under it, and go on", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await startRun(w, c, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    const seq = d.done(runId, { status: "failed", error: DAEMON_RESTARTED, sessionId: "s-1" });
    await d.acked(runId, seq);
    await waitUntil(() => botMessages(c, topicId, w.bot.id).some((m) => m.body === HOLD_TEXT.restarted("mac-mini")), 5000, "restart note");
    const next = await d.next((f) => f.t === "run.assign" && f.topicId === topicId && f.runId !== runId);
    expect(next.prompt).toContain("the computer restarted while a person held it");
    const old = await getRun(w.human, runId);
    expect(old.pausedBy).toBeUndefined();
    expect((await linkRow(w.machineId, runId)).held).toBe(0);
    c.close();
    d.close();
  });
});

describe("a person's commands (design-takeover T3)", () => {
  it("run one command at a time for the holder, keep output to the holder and admins, audit it", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    // Not taken over yet: conflict.
    expect((await command(w.human, runId, "ls")).status).toBe(409);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    expect((await command(w.other, runId, "ls")).status).toBe(403);
    expect((await command(w.owner, runId, "ls")).status).toBe(403);
    expect((await command(w.human, runId, "")).status).toBe(400);
    expect((await command(w.human, runId, "x".repeat(4001))).status).toBe(400);
    const before = (await deskRow(w.bot.id, runId)).last_event_at;

    const secret = "ghp_" + "a".repeat(36);
    const ran = await command(w.human, runId, `GH_TOKEN=${secret} git push`);
    expect(ran.status, JSON.stringify(ran.body)).toBe(200);
    const { commandId } = ran.body;
    expect(await d.next((f) => f.t === "run.command")).toEqual({ t: "run.command", runId, commandId, command: `GH_TOKEN=${secret} git push`, by: w.human.id });
    // One at a time.
    const busy = await command(w.human, runId, "ls");
    expect(busy.status).toBe(409);
    expect(busy.body.error.reason).toBe("hand.busy");
    const audit = await audited("run.command", runId);
    expect(audit.map((x) => x.actor_id)).toEqual([w.human.id]);
    expect(audit[0].details).not.toContain(secret);
    expect(audit[0].details).toContain("git push");

    // The daemon reports the command; the output reaches only the holder and admins.
    d.event(runId, { type: "hand.command", commandId, command: "git push", by: w.human.id });
    d.event(runId, { type: "hand.output", commandId, text: "Everything up-to-date\n" });
    // Cancel of the running command is sent now.
    const cancel = await api(`/v1/runs/${runId}/commands/${commandId}/cancel`, { token: w.human.token, body: {} });
    expect(cancel).toMatchObject({ status: 200, body: { ok: true } });
    expect(await d.next((f) => f.t === "run.command.cancel")).toEqual({ t: "run.command.cancel", runId, commandId });
    expect((await api(`/v1/runs/${runId}/commands/${commandId}/cancel`, { token: w.other.token, body: {} })).status).toBe(403);
    const seq = d.event(runId, { type: "hand.result", commandId, ok: true, exitCode: 0, ms: 120 });
    await d.acked(runId, seq);
    // hand.* are not progress: the stall clock did not move, no tool call counted.
    const row = await deskRow(w.bot.id, runId);
    expect(row.last_event_at).toBe(before);
    expect(row.tool_calls).toBe(0);
    expect(row.hand_command).toBeNull();

    const types = async (m: TestMember) => ((await api(`/v1/runs/${runId}/events`, { token: m.token })).body.entries as any[]).map((e) => e.event?.type).filter(Boolean);
    expect(await types(w.human)).toEqual(expect.arrayContaining(["hand.command", "hand.output", "hand.result"]));
    expect(await types(w.owner)).toContain("hand.output");
    const others = await types(w.other);
    expect(others).toEqual(expect.arrayContaining(["hand.command", "hand.result"]));
    expect(others).not.toContain("hand.output");

    // The next command runs; then the hand-back ends a running command first and keeps the summary.
    const second = await command(w.human, runId, "npm test");
    expect(second.status).toBe(200);
    expect((await hold(w.human, runId, "resume")).status).toBe(200);
    expect(await d.next((f) => f.t === "run.command.cancel" && f.commandId === second.body.commandId)).toBeTruthy();
    const summary = { type: "hand.summary", by: w.human.id, heldMs: 60_000, commands: 2, files: [".env.example"], headFrom: "a1b2c3", headTo: "d4e5f6" };
    const s2 = d.event(runId, summary);
    await d.acked(runId, s2);
    const { type: _t, ...handback } = summary;
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.lastHandback);
    expect((await getRun(w.human, runId)).lastHandback).toEqual(handback);
    // Not held any more: no commands.
    expect((await command(w.human, runId, "ls")).status).toBe(409);
    c.close();
    d.close();
  });

  it("never leave a page of run events empty while more follow", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ISOLATED, version: "1.2.0" });
    const { c } = await Client.connect(w.human.token);
    const { runId } = await startRun(w, c, d);
    expect((await hold(w.human, runId, "takeover")).status).toBe(200);
    const { commandId } = (await command(w.human, runId, "yes | head")).body;
    d.event(runId, { type: "hand.command", commandId, command: "yes | head", by: w.human.id });
    // More hidden frames than a page-by-page scan of 50 pages would cover at limit 1.
    for (let i = 0; i < 60; i++) d.event(runId, { type: "hand.output", commandId, text: `y${i}\n` });
    const last = d.event(runId, { type: "hand.result", commandId, ok: true, exitCode: 0, ms: 5 });
    await d.acked(runId, last);
    // Bob pages one at a time from after the command: the outputs are skipped, the result comes.
    const page = await api(`/v1/runs/${runId}/events?afterSeq=1&limit=1`, { token: w.other.token });
    expect(page.status).toBe(200);
    expect(page.body.entries.map((e: any) => e.event.type)).toEqual(["hand.result"]);
    expect(page.body.hasMore).toBe(false);
    c.close();
    d.close();
  });
});
