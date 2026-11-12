// A run uses the GitHub of the person it works for (design-github-identity G3): who that person
// is (requested_for and the fallbacks), `run.assign.github` per machine kind, the machine token
// routes (token, wait, rejected), and the follow-up that goes on once the person connects.
// GitHub is a fake fetch; tokens and ids are made up.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { authorOf, identityPerson, machineKind, mayUseToken, sourceFor } from "../src/github/assign";
import { encryptToken } from "../src/github/store";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, type TestMember, api, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

afterEach(() => {
  vi.restoreAllMocks();
});

const realFetch = globalThis.fetch;
let n = 0;
const fakeToken = () => `gho_${"S".repeat(30)}${String(++n).padStart(6, "0")}`;
const githubUserId = () => 900_000 + Math.floor(Math.random() * 1_000_000_000);

/** Stores a connection as the device sign-in would (encrypted); returns the made-up token. */
async function connect(memberId: string, opts: { status?: "ok" | "invalid"; login?: string; name?: string | null; userId?: number } = {}) {
  const token = fakeToken();
  const now = Date.now();
  await env.DB.prepare(
    `INSERT INTO github_accounts (member_id, github_user_id, login, name, token_enc, scopes, status, connected_at, checked_at)
     VALUES (?, ?, ?, ?, ?, 'repo read:org workflow', ?, ?, ?)`,
  )
    .bind(memberId, opts.userId ?? githubUserId(), opts.login ?? "octo", opts.name === undefined ? "Octo Cat" : opts.name, await encryptToken(env as never, memberId, token), opts.status ?? "ok", now, now)
    .run();
  return token;
}

/** GitHub as a fake: the device sign-in for `user`, and `GET /user` answering `userStatus`. */
function fakeGithub(user: { id: number; login: string; name?: string }, userStatus = 200) {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    const u = new URL(req.url);
    if (u.host !== "github.com" && u.host !== "api.github.com") return realFetch(input as RequestInfo, init);
    if (u.pathname === "/login/device/code") {
      return Response.json({ device_code: "dev-code", user_code: "ABCD-1234", verification_uri: "https://github.com/login/device", expires_in: 899, interval: 5 });
    }
    if (u.pathname === "/login/oauth/access_token") return Response.json({ access_token: fakeToken(), token_type: "bearer", scope: "repo,read:org,workflow" });
    if (u.pathname === "/user") return userStatus === 200 ? Response.json(user) : Response.json({ message: "Bad credentials" }, { status: userStatus });
    return new Response("not found", { status: 404 });
  });
}

const tokenOf = (runId: string, machineToken: string) => api(`/v1/machine/runs/${runId}/github`, { token: machineToken });
const waitFor = (runId: string, machineToken: string, reason = "not_connected") =>
  api(`/v1/machine/runs/${runId}/github/wait`, { token: machineToken, body: { reason } });

const githubDaemon = (w: World, runsIn = "vm", token = w.machineToken) => Daemon.connect(token, { features: ["github"], runsIn });

/** A personal machine of `person` with the world's runner label. */
async function personalMachine(w: World, person: TestMember): Promise<{ id: string; token: string }> {
  const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
  const r = await api("/v1/machines", { token: person.token, body: { name: "own-mac", labels: runner.body.labels, personal: true } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  // The world's team machine is never connected in these tests, so runs go to this one.
  return { id: r.body.machine.id, token: r.body.token };
}

describe("which machine uses whose GitHub (3.2)", () => {
  it("follows the table", () => {
    // The person's own personal machine, on its host: its own sign-in until they connect.
    expect(sourceFor("own", "host", false)).toBe("machine");
    expect(sourceFor("own", "host", true)).toBe("app");
    // Another person's personal machine on its host: always its own sign-in.
    expect(sourceFor("other", "host", false)).toBe("machine");
    expect(sourceFor("other", "host", true)).toBe("machine");
    // Team machines and any VM: the person's connected GitHub.
    for (const kind of ["team", "own", "other"] as const) {
      expect(sourceFor(kind, "vm", false)).toBe("app");
      expect(sourceFor(kind, "vm", true)).toBe("app");
    }
    expect(sourceFor("team", "host", false)).toBe("app");
    // An unknown place counts as a VM.
    expect(sourceFor("own", "pod", false)).toBe("app");
    expect(machineKind(null, "a")).toBe("team");
    expect(machineKind("a", "a")).toBe("own");
    expect(machineKind("b", "a")).toBe("other");
    expect(mayUseToken("other", "host")).toBe(false);
    expect(mayUseToken("other", "vm")).toBe(true);
    expect(mayUseToken("own", "host")).toBe(true);
    expect(mayUseToken("team", undefined)).toBe(true);
  });

  it("names the author by GitHub when connected, else by the team", () => {
    expect(authorOf("m1", "Alice", null)).toEqual({ name: "Alice", email: "m1@users.noreply.invalid" });
    const row = { github_user_id: 42, login: "octo", name: null, status: "ok" } as never;
    expect(authorOf("m1", "Alice", row)).toEqual({ name: "octo", email: "42+octo@users.noreply.github.com" });
  });
});

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
  it("a team machine in a VM: the requester's GitHub, author by name until connected", async () => {
    const w = await world();
    const { d } = await githubDaemon(w, "vm");
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toEqual({
      source: "app",
      memberId: w.human.id,
      author: { name: "Alice", email: `${w.human.id}@users.noreply.invalid` },
      committer: { name: "dev", email: `ai-${w.bot.id}@users.noreply.invalid` },
      rewriteSsh: true,
    });
    const row = await env.DB.prepare("SELECT github_member_id FROM bot_runs WHERE id = ?").bind(a.runId).first<{ github_member_id: string }>();
    expect(row?.github_member_id).toBe(w.human.id);
    // The run says whom it works for; the frame never holds a token.
    const run = await api(`/v1/runs/${a.runId}`, { token: w.human.token });
    expect(run.body.requestedFor).toBe(w.human.id);
    expect(JSON.stringify(a)).not.toContain("gho_");
    d.close();
    c.close();
  });

  it("a connected person: login and the GitHub author", async () => {
    const w = await world();
    await connect(w.human.id, { login: "alice-gh", name: "Alice G", userId: 123456 });
    const { d } = await githubDaemon(w, "host");
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toMatchObject({ source: "app", memberId: w.human.id, login: "alice-gh", author: { name: "Alice G", email: "123456+alice-gh@users.noreply.github.com" }, rewriteSsh: true });
    d.close();
    c.close();
  });

  it("the requester's own machine on its host: its own sign-in, then the app once connected", async () => {
    const w = await world();
    const own = await personalMachine(w, w.human);
    const { d } = await githubDaemon(w, "host", own.token);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toEqual({ source: "machine", memberId: w.human.id, committer: { name: "dev", email: `ai-${w.bot.id}@users.noreply.invalid` } });
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));

    await connect(w.human.id, { login: "alice-gh" });
    await mention(c, w, "@dev one more");
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    // Connected: the app's token, and the machine's own git config for the author.
    expect(b.github).toEqual({ source: "app", memberId: w.human.id, login: "alice-gh", committer: { name: "dev", email: `ai-${w.bot.id}@users.noreply.invalid` } });
    expect((await tokenOf(b.runId, own.token)).status).toBe(200);
    d.close();
    c.close();
  });

  it("another person's machine on its host: never takes the person's run", async () => {
    // design-roles R4: a personal machine takes only its owner's work, so the person's run is left
    // to the team machine (never set up here: no run starts) and the owner's machine gets nothing.
    // "other" on a host stays in the table above, for a hook's run.
    const w = await world();
    const bobs = await personalMachine(w, w.owner);
    await connect(w.human.id);
    const { d } = await githubDaemon(w, "host", bobs.token);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.endedAt !== undefined);
    expect(d.frames.filter((f) => f.t === "run.assign")).toEqual([]);
    d.close();
    c.close();
  });

  it("an older daemon gets no field and its runs no person", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.github).toBeUndefined();
    const row = await env.DB.prepare("SELECT github_member_id FROM bot_runs WHERE id = ?").bind(a.runId).first<{ github_member_id: string | null }>();
    expect(row?.github_member_id).toBeNull();
    expect((await tokenOf(a.runId, w.machineToken)).status).toBe(403);
    d.close();
    c.close();
  });
});

describe("the machine's token route", () => {
  it("gives the token only to the run's machine while the run lives, and audits without it", async () => {
    const w = await world();
    const token = await connect(w.human.id, { login: "alice-gh" });
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });

    const ok = await tokenOf(a.runId, w.machineToken);
    expect(ok.status).toBe(200);
    expect(ok.body).toEqual({ token, login: "alice-gh" });
    const audit = await env.DB.prepare("SELECT details, target FROM audit WHERE action = 'github.token.issue' AND details LIKE ?")
      .bind(`%${a.runId}%`)
      .first<{ details: string; target: string }>();
    expect(JSON.parse(audit!.details)).toEqual({ runId: a.runId, machineId: w.machineId, memberId: w.human.id });
    expect(audit!.target).toBe(w.human.id);
    const leaked = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE details LIKE ?").bind(`%${token}%`).first<{ n: number }>();
    expect(leaked?.n).toBe(0);

    // Another machine, an unknown run: not allowed.
    const other = await api("/v1/machines", { token: w.owner.token, body: { name: "other", labels: ["x"] } });
    const wrong = await tokenOf(a.runId, other.body.token);
    expect(wrong.status).toBe(403);
    expect(wrong.body.error.reason).toBe("github.not_allowed");
    expect((await tokenOf("01UNKNOWNRUN00000000000000", w.machineToken)).status).toBe(403);
    expect((await api(`/v1/machine/runs/${a.runId}/github`, {})).status).toBe(401);

    // Invalid, then not connected.
    await env.DB.prepare("UPDATE github_accounts SET status = 'invalid' WHERE member_id = ?").bind(w.human.id).run();
    const invalid = await tokenOf(a.runId, w.machineToken);
    expect(invalid.status).toBe(409);
    expect(invalid.body.error).toMatchObject({ code: "conflict", reason: "github.invalid" });
    await env.DB.prepare("DELETE FROM github_accounts WHERE member_id = ?").bind(w.human.id).run();
    const missing = await tokenOf(a.runId, w.machineToken);
    expect(missing.status).toBe(404);
    expect(missing.body.error).toMatchObject({ code: "not_found", reason: "github.not_connected" });

    // Ended: never again.
    await connect(w.human.id);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    const ended = await tokenOf(a.runId, w.machineToken);
    expect(ended.status).toBe(403);
    expect(ended.body.error.reason).toBe("github.not_allowed");
    d.close();
    c.close();
  });

  it("refuses a cancelled run's machine", async () => {
    const w = await world();
    await connect(w.human.id);
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    expect((await tokenOf(a.runId, w.machineToken)).status).toBe(200);
    expect((await api(`/v1/runs/${a.runId}/cancel`, { method: "POST", token: w.human.token })).status).toBe(200);
    expect((await tokenOf(a.runId, w.machineToken)).status).toBe(403);
    d.close();
    c.close();
  });

  it("rejected: checks the token once and marks it invalid on 401", async () => {
    const w = await world();
    await connect(w.human.id, { login: "alice-gh" });
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    fakeGithub({ id: 1, login: "alice-gh" }, 200);
    expect((await api(`/v1/machine/runs/${a.runId}/github/rejected`, { method: "POST", token: w.machineToken, body: {} })).body).toEqual({ ok: true });
    expect((await env.DB.prepare("SELECT status FROM github_accounts WHERE member_id = ?").bind(w.human.id).first<{ status: string }>())?.status).toBe("ok");
    vi.restoreAllMocks();
    fakeGithub({ id: 1, login: "alice-gh" }, 401);
    expect((await api(`/v1/machine/runs/${a.runId}/github/rejected`, { method: "POST", token: w.machineToken, body: {} })).status).toBe(200);
    expect((await env.DB.prepare("SELECT status FROM github_accounts WHERE member_id = ?").bind(w.human.id).first<{ status: string }>())?.status).toBe("invalid");
    expect((await tokenOf(a.runId, w.machineToken)).status).toBe(409);
    d.close();
    c.close();
  });
});

describe("waiting for a person to connect GitHub", () => {
  it("asks once, waits, and goes on by itself once they connect", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    expect((await tokenOf(a.runId, w.machineToken)).status).toBe(404);

    // Two reports at once: one row, one @.
    const [r1, r2] = await Promise.all([waitFor(a.runId, w.machineToken), waitFor(a.runId, w.machineToken)]);
    expect(r1.body).toEqual({ ok: true });
    expect(r2.body).toEqual({ ok: true });
    const ask = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(ask.data.message.body).toBe("@Alice 这一步要用你的 GitHub。连接 GitHub 后我会自己接着做。");
    expect(ask.data.message.mentions).toEqual([w.human.id]);
    await new Promise((r) => setTimeout(r, 300));
    const asks = c.events().filter((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(asks).toHaveLength(1);

    // The person sees what waits for them.
    const status = await api("/v1/me/github", { token: w.human.token });
    expect(status.body.waits).toEqual([{ runId: a.runId, groupId: w.gid, topicId, botId: w.bot.id, reason: "not_connected" }]);
    expect((await api("/v1/me/github", { token: w.owner.token })).body.waits).toEqual([]);

    // The CLI stops and says it waits: the topic is not done, it waits on the person.
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "Waiting for Alice to connect GitHub." }));
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "blocked");
    expect(c.events().some((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "done")).toBe(false);

    // They connect: one follow-up, for them, in the same work.
    fakeGithub({ id: githubUserId(), login: "alice-gh", name: "Alice G" });
    const dev = await api("/v1/me/github/device", { method: "POST", token: w.human.token });
    const poll = await api(`/v1/me/github/device/${dev.body.deviceId}/poll`, { method: "POST", token: w.human.token });
    expect(poll.body.state).toBe("done");
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.topicId).toBe(topicId);
    expect(b.prompt).toContain("Alice has connected GitHub");
    expect(b.github).toMatchObject({ source: "app", memberId: w.human.id, login: "alice-gh" });
    const run = await api(`/v1/runs/${b.runId}`, { token: w.human.token });
    expect(run.body).toMatchObject({ requestedFor: w.human.id, requestedBy: w.human.id });
    expect((await api("/v1/me/github", { token: w.human.token })).body.waits).toEqual([]);
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
    d.close();
    c.close();
  });

  it("a connection while the run still reports: it goes on after the run ends", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await waitFor(a.runId, w.machineToken, "invalid");
    const ask = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(ask.data.message.body).toBe("@Alice 你的 GitHub 连接失效了，重新连接后我会自己接着做。");
    expect((await api("/v1/me/github", { token: w.human.token })).body.waits[0].reason).toBe("invalid");

    fakeGithub({ id: githubUserId(), login: "alice-gh" });
    const dev = await api("/v1/me/github/device", { method: "POST", token: w.human.token });
    expect((await api(`/v1/me/github/device/${dev.body.deviceId}/poll`, { method: "POST", token: w.human.token })).body.state).toBe("done");
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "Waiting for GitHub." }));
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt).toContain("has connected GitHub");
    d.close();
    c.close();
  });

  it("someone else went on first: the late connection starts nothing", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await waitFor(a.runId, w.machineToken);
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "Waiting for GitHub." }));

    // The owner says 继续 in the topic: that run is the owner's (谁说话谁负责), and Alice's wait is gone.
    const { c: oc } = await Client.connect(w.owner.token);
    await oc.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: crypto.randomUUID(), body: "@dev 继续", mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.github.memberId).toBe(w.owner.id);
    await waitUntil(async () => (await api("/v1/me/github", { token: w.human.token })).body.waits.length === 0, 5000, "wait gone");

    fakeGithub({ id: githubUserId(), login: "alice-gh" });
    const dev = await api("/v1/me/github/device", { method: "POST", token: w.human.token });
    expect((await api(`/v1/me/github/device/${dev.body.deviceId}/poll`, { method: "POST", token: w.human.token })).body.state).toBe("done");
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "pushed" }));
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
    oc.close();
    d.close();
    c.close();
  });

  it("connected before the report arrives: no ask, no row, one resume and no 继续 besides", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await connect(w.human.id, { login: "alice-gh" });
    expect((await waitFor(a.runId, w.machineToken)).body).toEqual({ ok: true });
    expect(await env.DB.prepare("SELECT 1 FROM github_waits WHERE run_id = ?").bind(a.runId).first()).toBeNull();
    // A promise-only result: the GitHub resume goes on, the desk does not also say 继续.
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "好的，我来看看。" }));
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt).toContain("Alice has connected GitHub");
    await new Promise((r) => setTimeout(r, 400));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(2);
    expect(c.events().some((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("连接 GitHub"))).toBe(false);
    d.close();
    c.close();
  });

  it("a private card's answerer is never named: no name, no mention, no waitingOn, no name in the resume", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    // As a private card answer starts it: requested_by anonymous, requested_for the answerer.
    const desk = (env as unknown as BotsEnv).BOT_DESK.get((env as unknown as BotsEnv).BOT_DESK.idFromName(w.bot.id));
    await runInDurableObject(desk, async (_o, state) => {
      state.storage.sql.exec("UPDATE runs SET requested_by = 'anonymous', requested_for = ? WHERE id = ?", w.human.id, a.runId);
    });
    await waitFor(a.runId, w.machineToken);
    const ask = await c.waitEvent((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && e.data.message.body.includes("GitHub"));
    expect(ask.data.message.body).toBe("这一步要用 GitHub。连接 GitHub 后我会自己接着做。");
    expect(ask.data.message.mentions ?? []).toEqual([]);
    // The person still sees it among their own waits.
    expect((await api("/v1/me/github", { token: w.human.token })).body.waits).toHaveLength(1);

    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "Waiting for GitHub." }));
    const blocked = await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "blocked");
    expect(blocked.data.waitingOn ?? []).toEqual([]);
    expect(JSON.stringify(blocked)).not.toContain(w.human.id);

    fakeGithub({ id: githubUserId(), login: "alice-gh" });
    const dev = await api("/v1/me/github/device", { method: "POST", token: w.human.token });
    expect((await api(`/v1/me/github/device/${dev.body.deviceId}/poll`, { method: "POST", token: w.human.token })).body.state).toBe("done");
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt).toContain("has connected GitHub");
    expect(b.prompt).not.toContain("Alice has connected");
    d.close();
    c.close();
  });

  it("a deactivated person's waits go away", async () => {
    const w = await world();
    const { d } = await githubDaemon(w);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await waitFor(a.runId, w.machineToken);
    c.close();
    expect((await api(`/v1/members/${w.human.id}`, { method: "PATCH", token: w.owner.token, body: { deactivated: true } })).status).toBe(200);
    await waitUntil(async () => !(await env.DB.prepare("SELECT 1 FROM github_waits WHERE run_id = ?").bind(a.runId).first()), 5000, "wait removed");
    d.close();
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
