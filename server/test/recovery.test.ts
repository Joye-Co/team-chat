// 盯 1.2, 1.7, 1.14; 接 3.5; 机 2.6, 5.3: work that starts again continues instead of starting
// over. A restart keeps the CLI session and starts with a recovery note written by code; work
// that reached outside is not started over elsewhere; a daemon restart resumes the session; a
// run that moved is stopped on its old computer first thing when that one is back.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { DAEMON_RESTARTED, MAX_DAEMON_RESUMES, actedOutward, backgroundStep, outwardAction, recoveryNote, stepOf } from "../src/agent/recovery";
import { STALL_MS, TEXT } from "../src/bots/dispatch";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, api, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const benv = env as unknown as { BOT_DESK: DurableObjectNamespace; MACHINE_LINK: DurableObjectNamespace; GROUP_ROOM: DurableObjectNamespace };
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));
const linkOf = (machineId: string) => benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId));
const silence = (botId: string, runId: string, ms: number) =>
  runInDurableObject(deskOf(botId), (_o, state: DurableObjectState) => {
    state.storage.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", Date.now() - ms, runId);
  });
const deskRow = (botId: string, runId: string) =>
  runInDurableObject(deskOf(botId), (_o, state: DurableObjectState) =>
    state.storage.sql
      .exec<{ status: string; summary: string | null; session_id: string | null; stalled: number }>("SELECT status, summary, session_id, stalled FROM runs WHERE id = ?", runId)
      .one(),
  );
const botNotes = (c: Client, topicId: string, botId: string): string[] =>
  c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.authorId === botId && e.data.message.topicId === topicId)
    .map((e) => String(e.data.message.body));
async function secondMachine(w: World, name: string): Promise<{ token: string; id: string }> {
  const list = await api("/v1/machines", { token: w.owner.token });
  const labels = (list.body as { id: string; labels: string[] }[]).find((x) => x.id === w.machineId)?.labels ?? [];
  const m = await api("/v1/machines", { token: w.owner.token, body: { name, labels } });
  expect(m.status, JSON.stringify(m.body)).toBe(200);
  return { token: m.body.token, id: m.body.machine.id };
}
const PLAN = { type: "plan", items: [{ text: "改代码", state: "done" }, { text: "跑测试", state: "current" }] };
const PUSH = { type: "tool.call", itemId: "t-push", tool: "bash", title: "push", input: { command: "git push -u origin bot/abc" } };
const PR = { type: "tool.call", itemId: "t-pr", tool: "bash", title: "gh pr create --fill" };

describe("recovery rules", () => {
  it("finds outward actions and steps in the run events", () => {
    const ev = (event: Record<string, unknown>) => ({ t: "run.event", event });
    expect(outwardAction(ev(PUSH))).toBe("pushed with git: git push -u origin bot/abc");
    expect(outwardAction(ev(PR))).toBe("changed a pull request with gh: gh pr create --fill");
    expect(outwardAction(ev({ type: "tool.call", title: "git status" }))).toBeNull();
    expect(outwardAction(ev({ type: "tool.call", title: "git -C ../repo push origin HEAD" }))).toBe("pushed with git: git -C ../repo push origin HEAD");
    expect(outwardAction(ev({ type: "tool.call", title: "git -c http.extraHeader=x --no-pager push" }))).not.toBeNull();
    expect(outwardAction(ev({ type: "tool.call", title: 'git commit -m "push the fix"' }))).toBeNull();
    expect(outwardAction(ev({ type: "tool.call", title: "git log --oneline | grep push" }))).toBeNull();
    expect(outwardAction(ev({ type: "output", op: "card.post", body: { cardV2: { title: "方案" } } }))).toBe("posted a card in the topic: 方案");
    expect(outwardAction(ev({ type: "output", op: "member.handoff", body: {} }))).toBe("handed this work to another member");
    expect(outwardAction({ t: "run.done" })).toBeNull();
    expect(stepOf(ev({ type: "tool.call", title: "cargo   test" }))).toBe("cargo test");
    expect(stepOf(ev({ type: "message", text: "hi" }))).toBeNull();
  });

  it("writes a note that says where the work stood and what to check before repeating it", () => {
    const quiet = recoveryNote({ why: "stalled", tries: 1, plan: null, steps: [], actions: [], prs: [], artifacts: [] });
    expect(quiet).toContain("This is try 2");
    expect(quiet).toContain("do not start it over");
    expect(quiet).not.toContain("outside this machine");
    const facts = {
      why: DAEMON_RESTARTED,
      tries: 2,
      plan: [{ text: "跑测试", state: "current" as const }],
      steps: Array.from({ length: 25 }, (_, i) => `step ${i}`),
      actions: ["pushed with git: git push", "pushed with git: git push"],
      prs: ["https://github.com/o/r/pull/1 (open)"],
      artifacts: ['"方案" version 2'],
    };
    expect(actedOutward(facts)).toBe(true);
    const note = recoveryNote(facts);
    expect(note).toContain("(daemon restarted)");
    expect(note).toContain("- [current] 跑测试");
    expect(note).toContain("The last 20 steps");
    expect(note).not.toContain("- step 4\n");
    expect(note).toContain("- step 24");
    expect(note.match(/pushed with git/g)).toHaveLength(1);
    expect(note).toContain('- made 产出 "方案" version 2');
    expect(note).toContain("https://github.com/o/r/pull/1 (open)");
    expect(note).toContain("Do not push, open or change a pull request");
    expect(note).not.toContain("in the background");
  });

  // Verification pass 2, problem 4: a command the earlier try put in the background was stopped
  // with it; the note says so, and that the work is done only when nothing runs on.
  it("names the commands an earlier try left running in the background", () => {
    const ev = (event: Record<string, unknown>) => ({ t: "run.event", event });
    const bg = ev({ type: "tool.call", tool: "Bash", title: "sleep 90", input: { command: "sleep   90", run_in_background: true } });
    expect(backgroundStep(bg)).toBe("sleep 90");
    expect(backgroundStep(ev({ type: "tool.call", tool: "Bash", title: "ls", input: { command: "ls" } }))).toBeNull();
    expect(backgroundStep(ev({ type: "tool.call", tool: "Bash", title: "ls", input: { command: "ls", run_in_background: false } }))).toBeNull();
    expect(backgroundStep({ t: "run.done" })).toBeNull();
    const note = recoveryNote({ why: DAEMON_RESTARTED, tries: 1, plan: null, steps: ["sleep 90"], actions: [], prs: [], artifacts: [], background: ["sleep 90", "sleep 90"] });
    expect(note).toContain("ran in the background; any still running when that work stopped was stopped with it and may not have finished");
    expect(note).not.toContain("did not finish");
    expect(note.match(/- sleep 90/g)).toHaveLength(2);
    expect(note).toContain("wait for each in the foreground before you end your turn");
  });
});

describe("a restart continues the work", () => {
  it("resumes the session with the note, and never moves work that reached outside to another computer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "session", sessionId: "sess-a" });
    d.event(a.runId, PLAN);
    d.event(a.runId, PUSH);
    await d.acked(a.runId, d.event(a.runId, { type: "tool.result", itemId: "t-push", ok: true }));
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, { features: ["resume"] });

    // Silent: interrupted and continued in the same session, with the note.
    await silence(w.bot.id, a.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await d.next((f) => f.t === "run.cancel" && f.runId === a.runId);
    const same = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(same.resumeSessionId).toBe("sess-a");
    expect(same.prompt).toContain("Recovery note");
    expect(same.prompt).toContain("(stalled)");
    expect(same.prompt).toContain("- [current] 跑测试");
    expect(same.prompt).toContain("pushed with git: git push -u origin bot/abc");
    expect(same.prompt).toContain("Before you do anything else, check");
    // The old run keeps its session for the topic's next run.
    expect(await deskRow(w.bot.id, a.runId)).toMatchObject({ status: "cancelled", session_id: "sess-a" });
    d.send({ t: "run.accept", runId: same.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === same.runId && e.data.run.status === "running");

    // Silent again: another computer is free, but the work already pushed: people decide.
    await silence(w.bot.id, same.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const gaveUp = TEXT.gaveUp(10, ["打断后重来"]);
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(gaveUp), 5000, "gave up note");
    expect(b.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    expect(await deskRow(w.bot.id, same.runId)).toMatchObject({ status: "blocked", stalled: 1 });
    await d.acked(same.runId, d.done(same.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    b.close();
    c.close();
  });

  it("resumes the runs a daemon restart left unfinished, and fails with the reason only when it cannot", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    d.event(a.runId, { type: "session", sessionId: "sess-a" });
    const last = d.event(a.runId, PR);
    await d.acked(a.runId, last);
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");

    // The daemon comes back and ends the run it found unfinished (machine-protocol.md).
    const { d: d2, welcome } = await Daemon.connect(w.machineToken, { features: ["resume"], runs: [{ runId: a.runId, lastSeq: last + 1, done: true }] });
    expect(welcome.runs).toEqual([{ runId: a.runId, ackedSeq: last }]);
    d2.seq.set(a.runId, last);
    await d2.acked(a.runId, d2.done(a.runId, { status: "failed", error: DAEMON_RESTARTED, sessionId: "sess-a" }));
    let run = await d2.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(run.resumeSessionId).toBe("sess-a");
    expect(run.prompt).toContain("(daemon restarted)");
    expect(run.prompt).toContain("changed a pull request with gh: gh pr create --fill");
    expect(await deskRow(w.bot.id, a.runId)).toMatchObject({ status: "cancelled", summary: `restarted: ${DAEMON_RESTARTED}` });
    expect(botNotes(c, topicId, w.bot.id).filter((x) => x.startsWith("没有完成"))).toEqual([]);

    // A daemon that keeps restarting under the same work: it is resumed a few times, then the
    // run fails and says why.
    const seen = new Set([a.runId]);
    for (let i = 1; i < MAX_DAEMON_RESUMES; i++) {
      seen.add(run.runId);
      d2.send({ t: "run.accept", runId: run.runId });
      await d2.acked(run.runId, d2.done(run.runId, { status: "failed", error: DAEMON_RESTARTED, sessionId: "sess-a" }));
      run = await d2.next((f) => f.t === "run.assign" && !seen.has(f.runId));
      expect(run.resumeSessionId).toBe("sess-a");
      expect(run.prompt).toContain(`This is try ${i + 2}`);
    }
    d2.send({ t: "run.accept", runId: run.runId });
    await d2.acked(run.runId, d2.done(run.runId, { status: "failed", error: DAEMON_RESTARTED, sessionId: "sess-a" }));
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(TEXT.interrupted), 5000, "interrupted note");
    expect(await deskRow(w.bot.id, run.runId)).toMatchObject({ status: "failed" });
    await new Promise((r) => setTimeout(r, 200));
    expect(d2.frames.filter((f) => f.t === "run.assign" && !seen.has(f.runId) && f.runId !== run.runId)).toEqual([]);
    d2.close();
    c.close();
  });

  it("carries what people sent while the daemon was down into the resumed run", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    const last = d.event(a.runId, { type: "session", sessionId: "sess-a" });
    await d.acked(a.runId, last);
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    // An input the CLI never took (as a card answer is kept while the run's computer is away).
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec(
        "INSERT INTO inputs (id, run_id, message_id, author_id, body, at) VALUES (?, ?, ?, ?, ?, ?)",
        "in-1",
        a.runId,
        "msg-while-down",
        w.human.id,
        "选方案 B",
        Date.now(),
      );
    });
    const { d: d2 } = await Daemon.connect(w.machineToken, { features: ["resume"], runs: [{ runId: a.runId, lastSeq: last + 1, done: true }] });
    d2.seq.set(a.runId, last);
    await d2.acked(a.runId, d2.done(a.runId, { status: "failed", error: DAEMON_RESTARTED, sessionId: "sess-a" }));
    const run = await d2.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(run.resumeSessionId).toBe("sess-a");
    expect(run.prompt).toContain("选方案 B");
    // Carried once: no follow-up run starts for it besides.
    await new Promise((r) => setTimeout(r, 200));
    expect(d2.frames.filter((f) => f.t === "run.assign" && f.runId !== run.runId)).toEqual([]);
    d2.send({ t: "run.accept", runId: run.runId });
    await d2.acked(run.runId, d2.done(run.runId, { status: "succeeded", resultText: "ok" }));
    d2.close();
    c.close();
  });

  it("does not move work whose earlier try made 产出 to another computer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.event(a.runId, { type: "session", sessionId: "sess-a" }));
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, { features: ["resume"] });

    await silence(w.bot.id, a.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const same = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    d.send({ t: "run.accept", runId: same.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === same.runId && e.data.run.status === "running");
    // The first try made 产出 (its version keeps that run's id).
    await runInDurableObject(benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(w.gid)), (_o, state: DurableObjectState) => {
      const now = Date.now();
      state.storage.sql.exec(
        "INSERT INTO artifacts (id, topic_id, title, kind, current_version, created_by, created_at, updated_at) VALUES ('01JBX0ART000000000000000RC', ?, '方案', 'html', 1, ?, ?, ?)",
        topicId,
        w.bot.id,
        now,
        now,
      );
      state.storage.sql.exec(
        "INSERT INTO artifact_versions (artifact_id, version, entry, files, created_by, created_at, run_id) VALUES ('01JBX0ART000000000000000RC', 1, 'index.html', '[]', ?, ?, ?)",
        w.bot.id,
        now,
        a.runId,
      );
    });

    await silence(w.bot.id, same.runId, STALL_MS + 1000);
    await runDurableObjectAlarm(deskOf(w.bot.id));
    const gaveUp = TEXT.gaveUp(10, ["打断后重来"]);
    await waitUntil(() => botNotes(c, topicId, w.bot.id).includes(gaveUp), 5000, "gave up note");
    expect(b.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    expect(await deskRow(w.bot.id, same.runId)).toMatchObject({ status: "blocked", stalled: 1 });
    await d.acked(same.runId, d.done(same.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    b.close();
    c.close();
  });

  it("stops the old copy first thing when a computer the run moved away from comes back", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    const s1 = d.event(a.runId, PUSH);
    await d.acked(a.runId, s1);
    const other = await secondMachine(w, "mbp");
    const { d: b } = await Daemon.connect(other.token, {});
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    await runInDurableObject(linkOf(w.machineId), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'last_heartbeat_at'", String(Date.now() - 3 * 60_000));
    });
    await runDurableObjectAlarm(deskOf(w.bot.id));
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET away_since = ? WHERE id = ?", Date.now() - 31 * 60_000, a.runId);
    });
    await runDurableObjectAlarm(deskOf(w.bot.id));

    // Moved after the wait: a new session there, with the note naming the push.
    const moved = await b.next((f) => f.t === "run.assign");
    expect(moved.resumeSessionId).toBeUndefined();
    expect(moved.prompt).toContain("(machine away)");
    expect(moved.prompt).toContain("pushed with git: git push -u origin bot/abc");

    // The old computer comes back still holding the run: the welcome leaves it out, so the daemon
    // stops it before anything else, and nothing for it is queued.
    const { d: back, welcome } = await Daemon.connect(w.machineToken, { runs: [{ runId: a.runId, lastSeq: s1 + 1, done: false }] });
    expect(welcome.runs).toEqual([]);
    back.seq.set(a.runId, s1);
    await back.acked(a.runId, back.event(a.runId, { type: "message", role: "assistant", itemId: "m1", text: "late" }));
    expect(back.frames.filter((f) => f.runId === a.runId && f.t !== "ack")).toEqual([]);
    b.send({ t: "run.accept", runId: moved.runId });
    await b.acked(moved.runId, b.done(moved.runId, { status: "succeeded", resultText: "ok" }));
    back.close();
    b.close();
    c.close();
  });
});
