// An AI member uses the GitHub sign-in of the computer it runs on (design-github-identity 3.2,
// 3.4, unit G2): who a run works for, `run.assign.github` (always "machine", commits by the AI
// member, who asked), the switched-off person sign-in, the cleanup of old data and waits, and what
// the AI member says when the computer has no sign-in. Ids and names are made up.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { githubOff1 } from "../src/bots/desk/core";
import { runMigrations } from "../src/core/migrations";
import { githubFor, identityPerson } from "../src/github/assign";
import { GITHUB_TEXT } from "../src/github/text";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, type TestMember, api, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const benv = env as unknown as BotsEnv;
const deskOf = (botId: string): any => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId)) as never;
const linkOf = (machineId: string): any => benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId)) as never;

const githubDaemon = (w: World, runsIn = "vm", token = w.machineToken) => Daemon.connect(token, { features: ["github"], runsIn });

/** A personal machine of `person` with the world's runner label. */
async function personalMachine(w: World, person: TestMember): Promise<{ id: string; token: string }> {
  const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
  const r = await api("/v1/machines", { token: person.token, body: { name: "own-mac", labels: runner.body.labels, personal: true } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  // The world's team machine is never connected in these tests, so runs go to this one.
  return { id: r.body.machine.id, token: r.body.token };
}

const ai = (w: World) => ({ name: "dev", email: `ai-${w.bot.id}@users.noreply.invalid` });
const settle = () => new Promise((r) => setTimeout(r, 300));

describe("the identity person", () => {
  it("takes the requester, the hook's maker, the member's adder, then the earliest admin", async () => {
    const db = env.DB;
    const admin = await createMember({ level: "owner" });
    const alice = await createMember();
    const bot = await createMember({ kind: "bot" });
    await db.prepare("UPDATE members SET created_by = ? WHERE id = ?").bind(alice.id, bot.id).run();
    const earliest = (await db
      .prepare("SELECT id FROM members WHERE level = 'owner' AND kind = 'human' AND deactivated = 0 ORDER BY created_at, id LIMIT 1")
      .first<{ id: string }>())!.id;

    // 1. The person the run works for.
    expect(await identityPerson(db, { requestedFor: admin.id, requestedBy: "anonymous", botId: bot.id })).toBe(admin.id);
    // A run from before the field: its requester when that is a person.
    expect(await identityPerson(db, { requestedFor: null, requestedBy: admin.id, botId: bot.id })).toBe(admin.id);
    // 2. A hook's run: the person who made the hook.
    const hookBot = await createMember({ kind: "bot" });
    await db
      .prepare("INSERT INTO hooks (id, name, kind, group_id, member_id, secret_enc, created_by, created_at) VALUES (?, 'h', 'generic', ?, ?, 'x', ?, ?)")
      .bind(`hook-${hookBot.id}`, await createGroup(admin, []), hookBot.id, admin.id, Date.now())
      .run();
    expect(await identityPerson(db, { requestedFor: null, requestedBy: hookBot.id, botId: bot.id })).toBe(admin.id);
    // 3. Nobody asked (a private answer from before the field): who added the AI member.
    expect(await identityPerson(db, { requestedFor: null, requestedBy: "anonymous", botId: bot.id })).toBe(alice.id);
    // Someone who left: the earliest active human admin.
    await db.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(alice.id).run();
    expect(await identityPerson(db, { requestedFor: alice.id, requestedBy: alice.id, botId: bot.id })).toBe(earliest);
    expect(await identityPerson(db, { requestedFor: null, requestedBy: "anonymous", botId: bot.id })).toBe(earliest);
  });
});

describe("run.assign.github", () => {
  it("a team machine in a VM: the machine's sign-in, the AI member commits, the PR names who asked", async () => {
    const w = await world();
    const { d } = await githubDaemon(w, "vm");
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toEqual({ source: "machine", memberId: w.human.id, committer: ai(w), author: ai(w), askedBy: "Alice" });
    // Nobody's GitHub is recorded for the run any more.
    const row = await env.DB.prepare("SELECT github_member_id FROM bot_runs WHERE id = ?").bind(a.runId).first<{ github_member_id: string | null }>();
    expect(row?.github_member_id).toBeNull();
    d.close();
    c.close();
  });

  it("the requester's own machine on its host: the same", async () => {
    const w = await world();
    const own = await personalMachine(w, w.human);
    const { d } = await githubDaemon(w, "host", own.token);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toEqual({ source: "machine", memberId: w.human.id, committer: ai(w), author: ai(w), askedBy: "Alice" });
    d.close();
    c.close();
  });

  it("names nobody for a private card's answer, and cuts a long name", async () => {
    const w = await world();
    const machine = { info: { features: ["github"], runsIn: "vm" } } as never;
    const anon = await githubFor(env as never, { requestedFor: w.human.id, requestedBy: "anonymous", botId: w.bot.id }, machine);
    expect(anon).toEqual({ source: "machine", memberId: w.human.id, committer: ai(w), author: ai(w) });
    const long = await createMember({ name: "名".repeat(120) });
    const named = await githubFor(env as never, { requestedFor: long.id, requestedBy: long.id, botId: w.bot.id }, machine);
    expect(named?.askedBy).toBe("名".repeat(100));
    // Longer than a member name may be on the wire: shortened again before any client sees it.
    await env.DB.prepare("UPDATE members SET name = 'Long' WHERE id = ?").bind(long.id).run();
    // No `runsIn` or no feature: no field.
    expect(await githubFor(env as never, { requestedFor: w.human.id, requestedBy: w.human.id, botId: w.bot.id }, { info: { features: ["github"] } } as never)).toBeNull();
    expect(await githubFor(env as never, { requestedFor: w.human.id, requestedBy: w.human.id, botId: w.bot.id }, { info: { runsIn: "vm" } } as never)).toBeNull();
  });

  it("an older daemon gets no field", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toBeUndefined();
    d.close();
    c.close();
  });
});

describe("who a run works for", () => {
  it("a retry keeps it and a hand-over passes it on", async () => {
    const w = await world();
    const qa = await createMember({ kind: "bot", name: "qa" });
    const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
    expect((await api(`/v1/bots/${qa.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: runner.body.labels } })).status).toBe(200);
    expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: qa.id } })).status).toBe(200);
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "output", outputId: "o1", op: "member.handoff", body: { member: "qa", goal: "补测试" } });
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === qa.id);
    expect(b.github.memberId).toBe(w.human.id);
    expect((await api(`/v1/runs/${b.runId}`, { token: w.human.token })).body.requestedFor).toBe(w.human.id);

    await d.acked(a.runId, d.done(a.runId, { status: "failed", error: "boom" }));
    const retry = await api(`/v1/runs/${a.runId}/retry`, { method: "POST", token: w.owner.token });
    expect(retry.status, JSON.stringify(retry.body)).toBe(200);
    expect(retry.body.requestedFor).toBe(w.human.id);
    const r = await d.next((f) => f.t === "run.assign" && f.runId === retry.body.id);
    expect(r.github.memberId).toBe(w.human.id);
    d.close();
    c.close();
  });
});

describe("a person's GitHub sign-in is off", () => {
  it("status says off, a sign-in is refused, disconnecting is fine", async () => {
    const m = await createMember();
    const status = await api("/v1/me/github", { token: m.token });
    expect(status.status).toBe(200);
    expect(status.body).toEqual({ available: false, unavailableReason: "off", waits: [] });
    const device = await api("/v1/me/github/device", { method: "POST", token: m.token });
    expect(device.status).toBe(409);
    expect(device.body.error).toMatchObject({ code: "conflict", reason: "github.off" });
    const poll = await api("/v1/me/github/device/x1/poll", { method: "POST", token: m.token });
    expect(poll.status).toBe(409);
    expect((await api("/v1/me/github", { method: "DELETE", token: m.token })).body).toEqual({ ok: true });
  });

  it("the machine's GitHub routes are gone", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    expect((await api(`/v1/machine/runs/${a.runId}/github`, { token: w.machineToken })).status).toBe(404);
    expect((await api(`/v1/machine/runs/${a.runId}/github/wait`, { token: w.machineToken, body: { reason: "not_connected" } })).status).toBe(404);
    expect((await api(`/v1/machine/runs/${a.runId}/github/rejected`, { token: w.machineToken, body: {} })).status).toBe(404);
    d.close();
    c.close();
  });

  it("migration 0019 empties the three tables and keeps them", async () => {
    const m = await createMember();
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare(
        "INSERT INTO github_accounts (member_id, github_user_id, login, name, token_enc, scopes, status, connected_at, checked_at) VALUES (?, ?, 'octo', NULL, 'v1.x.y', 'repo', 'ok', ?, ?)",
      ).bind(m.id, 900_000 + Math.floor(Math.random() * 1e9), now, now),
      env.DB.prepare("INSERT INTO github_device (id, member_id, device_enc, interval_s, expires_at, created_at) VALUES (?, ?, 'x', 5, ?, ?)").bind(`dev-${m.id}`, m.id, now + 60_000, now),
      env.DB.prepare("INSERT INTO github_waits (run_id, member_id, group_id, topic_id, bot_id, reason, created_at) VALUES (?, ?, 'g', 't', 'b', 'invalid', ?)").bind(`run-${m.id}`, m.id, now),
    ]);
    const step = env.TEST_MIGRATIONS.find((s) => s.name.startsWith("0019_github_off"));
    expect(step).toBeDefined();
    for (const q of step!.queries) await env.DB.prepare(q).run();
    for (const table of ["github_accounts", "github_device", "github_waits"]) {
      expect((await env.DB.prepare(`SELECT count(*) AS n FROM ${table}`).first<{ n: number }>())?.n, table).toBe(0);
    }
  });
});

describe("runs that waited for a person's GitHub (github-off-1)", () => {
  it("their topics wait on nobody and end as their runs ended", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    const t1 = await mention(c, w, "@dev one");
    const a = await d.next((f) => f.t === "run.assign" && f.topicId === t1);
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    const t2 = await mention(c, w, "@dev two");
    const b = await d.next((f) => f.t === "run.assign" && f.topicId === t2);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "failed", error: "boom" }));
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === t1 && e.data.status === "done");

    // As an older server left them: blocked on the person, with the wait keys in meta.
    const runIds: string[] = [a.runId, b.runId];
    await runInDurableObject(deskOf(w.bot.id), async (inst, state) => {
      const o = inst as any;
      for (const id of runIds) await o.topic(o.run(id), "blocked", [w.human.id], { outcome: o.run(id).status, result: "" });
      state.storage.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `github_wait:${a.runId}`, w.human.id);
      state.storage.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `github_resume:${b.runId}`, w.human.id);
      state.storage.sql.exec("DELETE FROM migrations WHERE name = 'github-off-1'");
      expect(runMigrations(state.storage, [{ name: "github-off-1", up: githubOff1 }])).toEqual(["github-off-1"]);
      const keys = state.storage.sql.exec<{ key: string }>("SELECT key FROM meta WHERE key GLOB 'github_*'").toArray().map((x) => x.key);
      expect(keys).toEqual(["github_off_pending"]);
      expect(JSON.parse(o.meta("github_off_pending")).sort()).toEqual([a.runId, b.runId].sort());
      o.githubOffStarted = false;
    });
    for (const t of [t1, t2]) {
      await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === t && e.data.status === "blocked" && e.data.waitingOn?.[0] === w.human.id);
    }
    const from = c.frames.length;
    const after = (t: string, status: string) =>
      waitUntil(
        () => c.frames.slice(from).find((f) => f.t === "event" && f.event.type === "topic.updated" && f.event.data.topicId === t && f.event.data.status === status),
        5000,
        `topic ${status}`,
      );

    // The next bind sets them right.
    await runInDurableObject(deskOf(w.bot.id), (o) => (o as any).bind(w.bot.id));
    const done = (await after(t1, "done")).event;
    expect(done.data.waitingOn ?? []).toEqual([]);
    const failed = (await after(t2, "blocked")).event;
    expect(failed.data.waitingOn ?? []).toEqual([]);
    await waitUntil(
      () => runInDurableObject(deskOf(w.bot.id), (o) => (o as any).meta("github_off_pending") === null || undefined),
      5000,
      "pending cleared",
    );
    d.close();
    c.close();
  });
});

describe("a computer without a GitHub sign-in", () => {
  const githubNotes = (c: Client, w: World) =>
    c.events().filter((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));

  it("a team computer: said once in the topic, the admins alerted, repeats are silent", async () => {
    const w = await world();
    const { d } = await githubDaemon(w, "vm");
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "missing", reason: "signed_out" }));
    const note = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(note.data.message.body).toBe(GITHUB_TEXT.teamMissing);
    expect(note.data.message.topicId).toBe(topicId);
    expect(note.data.message.mentions ?? []).toEqual([]);
    // The admins hear it from the computer's alert (monitor/alerts.ts), started once.
    const alert = await waitUntil(() => runInDurableObject(linkOf(w.machineId), (o) => (o as any).upAlerts().github), 5000, "github alert");
    expect(alert).toMatchObject({ kind: "github", reason: "signed_out" });
    expect(await (linkOf(w.machineId) as any).githubMissing("signed_out")).toBe(false);

    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "invalid" }));
    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "missing", reason: "no_gh" }));
    await settle();
    expect(githubNotes(c, w)).toHaveLength(1);
    // The topic is not blocked by it.
    expect(c.events().some((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "blocked")).toBe(false);
    d.close();
    c.close();
  });

  it("a team computer whose sign-in GitHub refused", async () => {
    const w = await world();
    const { d } = await githubDaemon(w, "vm");
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "invalid" }));
    const note = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(note.data.message.body).toBe(GITHUB_TEXT.teamInvalid);
    d.close();
    c.close();
  });

  it("a personal computer: names its owner, alerts nobody", async () => {
    const w = await world();
    const own = await personalMachine(w, w.human);
    const { d } = await githubDaemon(w, "vm", own.token);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "missing", reason: "signed_out" }));
    const note = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(note.data.message.body).toBe("这台电脑还没有登录 GitHub，推不了代码。请电脑主人 Alice 在电脑上登录后再 @我。");
    expect(note.data.message.mentions ?? []).toEqual([]);
    await d.acked(a.runId, d.event(a.runId, { type: "github", state: "invalid" }));
    await settle();
    expect(githubNotes(c, w)).toHaveLength(1);
    // No alert: the run's own message is all.
    expect(await runInDurableObject(linkOf(own.id), (o) => (o as any).upAlerts().github)).toBeUndefined();
    d.close();
    c.close();
  });
});
