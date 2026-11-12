// Work goes to the right computer by ownership (docs/plans/usable/design-roles.zh.md R4, 3.3 and
// 3.6; docs/protocol.md "Roles: 所有者 and personal machines"): a personal machine takes only its
// owner's work, checked machines come first and are waited for, a run no machine may take ends at
// once, hand-overs follow the person's machine, and on a personal machine only the person the run
// works for and the owner talk to it, approve it or pause it.
import { afterAll, describe, expect, it } from "vitest";
import { type DispatchCtx, type MachineView, TEXT, candidates, mayTake, personalBlock, placesFor, reachable } from "../src/bots/dispatch";
import { repoOn } from "../src/bots/desk/runs";
import { Daemon, HEALTH, type World, downViolations, mention, world } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

describe("ownership rules", () => {
  const now = Date.UTC(2026, 10, 13, 4, 0);
  const m = (id: string, more: Partial<MachineView> = {}): MachineView => ({
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
    ...more,
  });
  const runner = { runtime: "codex", labels: ["mac"] };
  const ctx = (requestedFor: string | null, more: Partial<DispatchCtx> = {}): DispatchCtx => ({ botId: "bot", requestedFor, ...more });
  const ids = (xs: MachineView[]) => xs.map((x) => x.id);

  it("sends one person's work to his own computer and nobody else's there", () => {
    const own = m("a-mac", { ownerId: "a", personalBots: new Map() });
    // Alone: an AI member placed nowhere lands on his computer.
    expect(ids(candidates([own], runner, now, [], ctx("a")))).toEqual(["a-mac"]);
    // Someone else's request does not.
    expect(candidates([own], runner, now, [], ctx("b"))).toEqual([]);
    // Without the ownership context every computer counts (the rules before).
    expect(ids(candidates([own], runner, now))).toEqual(["a-mac"]);
    // A team computer takes anyone's work.
    expect(ids(candidates([own, m("team")], runner, now, [], ctx("b")))).toEqual(["team"]);
  });

  it("lets a hook use a personal computer only when its owner placed the member there", () => {
    const own = m("a-mac", { ownerId: "a", personalBots: new Map() });
    const pinned = { ...runner, machineId: "a-mac" };
    expect(mayTake(own, pinned, ctx(null, { updatedBy: "a" }))).toBe(true);
    expect(mayTake(own, pinned, ctx(null, { updatedBy: "admin" }))).toBe(false);
    // Not pinned there: labels alone never let a hook in.
    expect(mayTake(own, runner, ctx(null, { updatedBy: "a" }))).toBe(false);
  });

  it("uses only the person's checked computers, and the one the work came from", () => {
    const checked = m("a-mac", { ownerId: "a", personalBots: new Map([["bot", null]]) });
    const other = m("a-linux", { ownerId: "a", personalBots: new Map(), labels: ["linux"] });
    const team = m("team");
    // A member pinned to the team computer still runs on the computer its person checked.
    const pinned = { ...runner, machineId: "team" };
    expect(placesFor([team, checked], pinned, ctx("a"))).toEqual({ machines: [checked], checked: true });
    // Anyone else gets the team computer.
    expect(ids(placesFor([team, checked], pinned, ctx("b")).machines)).toEqual(["team"]);
    // Handed over from his other computer: that one first, then the checked one.
    expect(ids(placesFor([team, checked, other], runner, ctx("a", { fromMachine: "a-linux" })).machines)).toEqual(["a-linux", "a-mac"]);
    // From someone else's computer: no.
    expect(mayTake(other, runner, ctx("b", { fromMachine: "a-linux" }))).toBe(false);
  });

  it("knows when no computer may ever take the run, whoever is online", () => {
    const own = m("a-mac", { ownerId: "a", personalBots: new Map(), connected: false });
    const pinned = { ...runner, machineId: "a-mac" };
    expect(reachable([own], pinned, ctx("a"))).toBe(true);
    expect(reachable([own], pinned, ctx("b"))).toBe(false);
    expect(personalBlock([own], pinned, ctx("b"))).toBe("a");
    // Nothing at all to take it: the usual no-computer text, not someone's computer.
    expect(personalBlock([], pinned, ctx("b"))).toBeNull();
    // Without the runtime it is no place either.
    expect(personalBlock([m("team", { clis: [] })], runner, ctx("b"))).toBeNull();
    expect(TEXT.personalOnly("dev", "Joye")).toBe("dev在Joye的电脑上干活，只做Joye交代的事。可以请管理员把它放到团队的电脑上，或者在你自己的电脑上用它。");
    expect(TEXT.waitPersonal(["alice-mac"])).toBe("在等alice-mac上线。不想等的话，在设置 › 电脑里取消勾选。");
  });

  it("gives a personal computer only its owner's code folder", () => {
    const repo = { repo: { path: "/srv/repo" } };
    expect(repoOn(repo, undefined, "bot")).toEqual({ path: "/srv/repo" });
    expect(repoOn(repo, { ownerId: undefined }, "bot")).toEqual({ path: "/srv/repo" });
    expect(repoOn(repo, { ownerId: "a", personalBots: new Map([["bot", "/Users/a/code"]]) }, "bot")).toEqual({ path: "/Users/a/code" });
    expect(repoOn(repo, { ownerId: "a", personalBots: new Map([["bot", null]]) }, "bot")).toBeUndefined();
    expect(repoOn({ repo: { url: "https://example.com/r.git" } }, { ownerId: "a", personalBots: new Map() }, "bot")).toEqual({ url: "https://example.com/r.git" });
  });
});

/** The world's runner labels. */
async function labelsOf(w: World): Promise<string[]> {
  return (await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token })).body.labels;
}

/** A personal machine of `person` with the world's runner labels. */
async function personalMachine(w: World, person: TestMember, name: string): Promise<{ id: string; token: string }> {
  const r = await api("/v1/machines", { token: person.token, body: { name, labels: await labelsOf(w), personal: true } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return { id: r.body.machine.id, token: r.body.token };
}

async function check(person: TestMember, machineId: string, botId: string, repoPath?: string): Promise<void> {
  const r = await api(`/v1/machines/${machineId}/bots/${botId}`, { method: "PUT", token: person.token, body: repoPath ? { repoPath } : {} });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
}

/** Until the owner's machine list shows the machine disconnected. */
async function waitUntilOffline(owner: TestMember, machineId: string): Promise<void> {
  for (let i = 0; i < 100; i++) {
    const list = await api("/v1/machines", { token: owner.token });
    if (list.body.find((x: { id: string; connected: boolean }) => x.id === machineId)?.connected === false) return;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error("machine still connected");
}

async function inGroup(w: World, id: string): Promise<void> {
  await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: id } });
}

function botSaid(c: Client, w: World, text: string): any {
  return c.events().find((e) => e.type === "message.created" && e.data.message.authorId === w.bot.id && String(e.data.message.body).includes(text))?.data.message;
}

describe("dispatch by ownership", () => {
  it("waits for the person's checked computer without holding up others, and moves when unchecked", async () => {
    const w = await world();
    const mac = await personalMachine(w, w.human, "alice-mac");
    await check(w.human, mac.id, w.bot.id);
    await inGroup(w, w.owner.id);
    // Alice's laptop was set up, then closed.
    const { d: laptop } = await Daemon.connect(mac.token);
    laptop.close();
    await waitUntilOffline(w.human, mac.id);
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);

    const topicId = await mention(c, w);
    await waitUntil(() => botSaid(c, w, TEXT.waitPersonal(["alice-mac"])), 5000, "wait note");
    const waiting = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.waitingFor === "offline")).data.run;
    expect(waiting).toMatchObject({ requestedFor: w.human.id, phase: "waiting_computer", queuePosition: 1 });

    // Joye's request goes to the team computer at once, and is first in its own queue.
    const res = await co.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev check the build", mentions: [w.bot.id] });
    const joyes = await d.next((f) => f.t === "run.assign" && f.topicId === res.message.topicId);
    expect(d.frames.filter((f) => f.t === "run.assign" && f.runId === waiting.id)).toEqual([]);
    d.send({ t: "run.accept", runId: joyes.runId });

    // Unchecked: the waiting run is placed again now, on the team computer with the member's folder.
    expect((await api(`/v1/machines/${mac.id}/bots/${w.bot.id}`, { method: "DELETE", token: w.human.token })).status).toBe(200);
    const moved = await d.next((f) => f.t === "run.assign" && f.runId === waiting.id);
    expect(moved.repo).toEqual({ path: "/tmp/repo", baseBranch: "main" });
    d.close();
    c.close();
    co.close();
  });

  it("says at once when the member works only on someone else's computer, and refuses its retry", async () => {
    const w = await world();
    const own = await personalMachine(w, w.owner, "joye-mac");
    const labels = await labelsOf(w);
    const pin = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels, machineId: own.id } });
    expect(pin.status, JSON.stringify(pin.body)).toBe(200);
    await inGroup(w, w.owner.id);
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);

    const topicId = await mention(c, w);
    const ended = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.endedAt !== undefined)).data.run;
    expect(ended).toMatchObject({ status: "failed", requestedFor: w.human.id });
    await waitUntil(() => botSaid(c, w, TEXT.personalOnly("dev", "Joye")), 5000, "personal only note");
    expect(d.frames.filter((f) => f.t === "run.assign")).toEqual([]);
    const retry = await api(`/v1/runs/${ended.id}/retry`, { token: w.human.token, body: {} });
    expect(retry.status).toBe(409);
    expect(retry.body.error?.reason).toBe("machine.personal_requester");

    // Joye's own request runs on his computer.
    const { d: mine } = await Daemon.connect(own.token);
    const { c: co } = await Client.connect(w.owner.token);
    const res = await co.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev mine", mentions: [w.bot.id] });
    const a = await mine.next((f) => f.t === "run.assign" && f.topicId === res.message.topicId);
    // On a personal computer only the owner's folder counts: none here, so no repo path.
    expect(a.repo).toBeUndefined();
    d.close();
    mine.close();
    c.close();
    co.close();
  });

  it("keeps a personal computer's run to its person and owner: input, approval and pause", async () => {
    const w = await world();
    const mac = await personalMachine(w, w.human, "alice-mac");
    await check(w.human, mac.id, w.bot.id, "/Users/alice/code");
    await inGroup(w, w.owner.id);
    const { d } = await Daemon.connect(mac.token);
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.repo).toEqual({ path: "/Users/alice/code", baseBranch: "main" });
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    d.event(a.runId, { type: "approval.request", approvalId: "g1", title: "rm -rf build", detail: "rm -rf build" });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "waiting");

    // The admin can neither approve nor talk to it, nor pause it.
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev also look at the docs", mentions: [w.bot.id] });
    const pause = await api(`/v1/runs/${a.runId}/pause`, { token: w.owner.token, body: {} });
    expect(pause.status).toBe(403);
    // The person it works for can.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ runId: a.runId, approvalId: "g1", decision: "approve" });
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.filter((f) => f.t === "run.approval")).toHaveLength(1);
    expect(d.frames.filter((f) => f.t === "run.input")).toEqual([]);
    d.close();
    c.close();
    co.close();
  });

  it("sends a follow-up of someone else to the team computer in a new session", async () => {
    const w = await world();
    const mac = await personalMachine(w, w.human, "alice-mac");
    await check(w.human, mac.id, w.bot.id);
    await inGroup(w, w.owner.id);
    const { d: md } = await Daemon.connect(mac.token, { features: ["resume"], cli: { resume: true } });
    const { d: td } = await Daemon.connect(w.machineToken, { features: ["resume"], cli: { resume: true } });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const a = await md.next((f) => f.t === "run.assign");
    md.send({ t: "run.accept", runId: a.runId });
    md.event(a.runId, { type: "session", sessionId: "s-1", runtime: "codex" });
    await md.acked(a.runId, md.done(a.runId, { status: "succeeded", resultText: "done", sessionId: "s-1" }));
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.endedAt !== undefined);

    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev one more thing", mentions: [w.bot.id] });
    const b = await td.next((f) => f.t === "run.assign");
    expect(b.topicId).toBe(topicId);
    expect(b.resumeSessionId).toBeUndefined();
    expect(md.frames.filter((f) => f.t === "run.assign" && f.runId !== a.runId)).toEqual([]);
    md.close();
    td.close();
    c.close();
    co.close();
  });

  it("refuses a part for a member that works only for someone else", async () => {
    const w = await world();
    const ops = await createMember({ kind: "bot", name: "ops" });
    const joyes = await personalMachine(w, w.owner, "joye-mac");
    const put = await api(`/v1/bots/${ops.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: await labelsOf(w), machineId: joyes.id } });
    expect(put.status).toBe(200);
    await inGroup(w, ops.id);
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "output", outputId: "o1", op: "member.assign", body: { member: "ops", goal: "部署一下" } });
    const refused = await d.next((f) => f.t === "run.output.result" && f.outputId === "o1");
    expect(refused).toMatchObject({ ok: false, error: { reason: "delegation.not_for_requester" } });
    expect(refused.error.message).toContain("ops works only for Joye");
    d.close();
    c.close();
  });

  it("hands work over on the person's computer", async () => {
    const w = await world();
    const qa = await createMember({ kind: "bot", name: "qa" });
    // qa is placed on the team computer: only the hand-over brings it to Alice's.
    const put = await api(`/v1/bots/${qa.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: await labelsOf(w), machineId: w.machineId } });
    expect(put.status).toBe(200);
    await inGroup(w, qa.id);
    const mac = await personalMachine(w, w.human, "alice-mac");
    await check(w.human, mac.id, w.bot.id);
    const { d: md } = await Daemon.connect(mac.token, { maxParallelRuns: 4 });
    const { d: td } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await md.next((f) => f.t === "run.assign");
    md.send({ t: "run.accept", runId: a.runId });

    // Handed over to qa: qa goes on on Alice's computer, not the team one.
    md.event(a.runId, { type: "output", outputId: "o2", op: "member.handoff", body: { member: "qa", goal: "补测试" } });
    const res = await md.next((f) => f.t === "run.output.result" && f.outputId === "o2");
    expect(res.ok, JSON.stringify(res)).toBe(true);
    await md.next((f) => f.t === "run.assign" && f.botMemberId === qa.id);
    expect(td.frames.filter((f) => f.t === "run.assign")).toEqual([]);
    md.close();
    td.close();
    c.close();
  });
});
