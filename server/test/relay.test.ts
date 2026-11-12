// Usable (design-relay R3): relays (按顺序做). A person writes the order in a topic; the room
// checks the members, wakes one step after the other from the states the desks report, and
// stops when a step fails, is blocked, is stopped or leaves code unpushed for another member.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { TopicRelay, mainTopicId } from "@chat/protocol";
import type { BotsEnv } from "../src/bots/env";
import { RelayParseError, parseRelay } from "../src/bots/relay/parse";
import { RELAY_RECHECK_MS, giveUpRelayRow, publishLeftRelays, relayStepState, startRelay } from "../src/bots/relay/engine";
import { requesterLeft } from "../src/bots/chains";
import type { RoomContext, RoomTx } from "../src/core/registry";
import { relayOrderText } from "../src/bots/desk/relay";
import { Daemon, type World, downViolations, world } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createMember, protocolViolations } from "./helpers";

const benv = env as unknown as BotsEnv;

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

async function addBot(w: World, name: string): Promise<TestMember> {
  const bot = await createMember({ kind: "bot", name });
  const runner = await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token });
  expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: runner.body.labels } })).status).toBe(200);
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: bot.id } })).status).toBe(200);
  return bot;
}

function room(gid: string) {
  return benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(gid));
}

async function inRoom<T>(gid: string, fn: (sql: SqlStorage) => T): Promise<T> {
  return runInDurableObject(room(gid), (_o, state: DurableObjectState) => fn(state.storage.sql)) as Promise<T>;
}

async function started(d: Daemon, botId: string, after: string[] = []): Promise<any> {
  const a = await d.next((f) => f.t === "run.assign" && f.botMemberId === botId && !after.includes(f.runId));
  d.send({ t: "run.accept", runId: a.runId });
  return a;
}

/** The newest relay the client saw on the topic. */
async function relayIs(c: Client, topicId: string, pred: (r: TopicRelay) => boolean): Promise<{ relay: TopicRelay; status?: string; waitingOn?: string[] }> {
  const e = await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.relay && pred(e.data.relay), 8000).catch((err) => {
    console.log("RELAYDEBUG", JSON.stringify(c.events().filter((e) => e.type === "topic.updated").map((e) => ({ t: e.topicId, ...e.data }))));
    throw err;
  });
  return { relay: TopicRelay.parse(e.data.relay), status: e.data.status, waitingOn: e.data.waitingOn };
}

const members = [
  { id: "A", name: "小码" },
  { id: "B", name: "小审" },
  { id: "C", name: "小码2" },
  { id: "H", name: "Joye" },
];

describe("parsing an arrow line", () => {
  it("reads the order from the body, with each separator, notes and the goal", () => {
    expect(parseRelay("@小码 写 → @小审 审 → @小码 按意见改\n给设置页加深色模式", members)).toEqual({
      goal: "给设置页加深色模式",
      steps: [
        { memberId: "A", note: "写" },
        { memberId: "B", note: "审" },
        { memberId: "A", note: "按意见改" },
      ],
    });
    // One line: the text after the last @ is the goal; -> and => work too.
    expect(parseRelay("@小码 -> @小审 => @小码 给设置页加深色模式", members)).toEqual({
      goal: "给设置页加深色模式",
      steps: [{ memberId: "A" }, { memberId: "B" }, { memberId: "A" }],
    });
    // The longest name wins: @小码2 is not @小码 with the note 2.
    expect(parseRelay("@小码2 → @小码 收尾", members)?.steps.map((s) => s.memberId)).toEqual(["C", "A"]);
  });

  it("is no relay without an arrow line, and refuses a broken one", () => {
    expect(parseRelay("@小码 看一下 a -> b 的区别", members)).toBeNull();
    expect(parseRelay("看一下 @小码 → @小审", members)).toBeNull();
    expect(parseRelay("@小码 写一下", members)).toBeNull();
    const bad = (body: string) => expect(() => parseRelay(body, members)).toThrow(RelayParseError);
    bad("@小码 → 小审 → @小码 x"); // a part without an @
    bad("@小码 → @没有这个人 x"); // a name nobody mentioned
    bad("@小码 → @小码 x"); // the same member twice in a row
    bad("@小码 → @小审"); // no goal
    bad("@小码 → @小审 → @小码 → @小审 → @小码 → @小审 → @小码 x"); // 7 steps
    expect(parseRelay("@小码 → @小审 → @小码 → @小审 → @小码 → @小审 x", members)?.steps).toHaveLength(6);
  });

  it("cuts a long goal and notes the way TopicRelay counts them (UTF-16), never splitting an emoji", () => {
    const goal = "a" + "😀".repeat(700);
    const note = "b" + "😀".repeat(80);
    const d = parseRelay(`@小码 ${note} → @小审 x\n${goal}`, members)!;
    expect(d.goal.length).toBeLessThanOrEqual(1000);
    expect(d.goal).toBe("a" + "😀".repeat(499));
    expect(d.steps[0]!.note!.length).toBeLessThanOrEqual(100);
    expect(d.steps[0]!.note).toBe("b" + "😀".repeat(49));
    const relay = { id: "R", state: "running", current: 0, goal: d.goal, startedBy: "H", updatedAt: 1, steps: d.steps.map((s) => ({ memberId: s.memberId, ...(s.note ? { note: s.note } : {}), state: "pending" })) };
    expect(TopicRelay.safeParse(relay).success).toBe(true);
  });

  it("writes the step's order with its place, the step before and where its code is", () => {
    const text = relayOrderText({
      index: 1,
      names: ["小码", "小审", "小码"],
      requesterName: "Joye",
      goal: "加深色模式",
      topicId: "T1",
      rootRequest: "@小码 写 → @小审 审 → @小码 改\n加深色模式",
      prev: { name: "小码", result: "PR #12 好了", code: { branch: "dark", head: "abcdef1234567", pushed: true }, artifacts: [{ artifactId: "a1", version: 2, title: "设计稿", parentVersion: 1 }] },
    });
    expect(text).toContain("You do step 2 of 3");
    expect(text).toContain("Check the result of the step before you");
    expect(text).toContain("PR #12 好了");
    expect(text).toContain("Code: branch dark at abcdef123456, pushed.");
    expect(text).toContain("设计稿 · a a1 v2 (from v1)");
    expect(text).toContain("push your branch before you end");
  });
});

describe("starting a relay (send)", () => {
  it("refuses a member that cannot take the work, a broken line and a second relay; leaves bots and the 群聊 alone", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const off = await addBot(w, "off");
    expect((await api(`/v1/members/${off.id}`, { method: "PATCH", token: w.owner.token, body: { deactivated: true } })).status).toBeLessThan(300);
    const { c } = await Client.connect(w.human.token);
    const send = (body: string, mentions: string[], topicId?: string) =>
      c.request({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body, mentions, ...(topicId ? { topicId } : {}) });

    const offline = await send(`@dev → @off → @dev 加深色模式`, [w.bot.id, off.id]);
    expect(offline).toMatchObject({ ok: false, error: { code: "invalid", reason: "relay.member_unavailable", memberIds: [off.id] } });
    // A person is no AI member either.
    const person = await send(`@dev → @Alice 加深色模式`, [w.bot.id, w.human.id]);
    expect(person.error).toMatchObject({ reason: "relay.member_unavailable", memberIds: [w.human.id] });
    expect((await send(`@dev → @dev 加深色模式`, [w.bot.id])).error).toMatchObject({ code: "invalid", reason: "relay.bad_steps" });

    const first = await send(`@dev → @qa 加深色模式`, [w.bot.id, qa.id]);
    expect(first.ok, JSON.stringify(first)).toBe(true);
    const topicId = first.result.message.topicId;
    expect(first.result.topic.relay).toMatchObject({ state: "running" });
    await relayIs(c, topicId, (r) => r.state === "running" && r.steps.length === 2);
    const second = await send(`@qa → @dev 再来`, [w.bot.id, qa.id], topicId);
    expect(second.error).toMatchObject({ code: "conflict", reason: "relay.running" });

    // In the 群聊 an arrow line is a plain message.
    expect((await send(`@dev → @qa 加深色模式`, [w.bot.id, qa.id], mainTopicId(w.gid))).ok).toBe(true);
    // An AI member's arrow message is a plain message.
    const botSent = await api(`/v1/groups/${w.gid}/messages`, { token: w.bot.token, body: { clientMsgId: clientMsgId(), body: `@dev → @qa 加深色模式`, mentions: [w.bot.id, qa.id] } });
    expect(botSent.status, JSON.stringify(botSent.body)).toBe(200);
    expect(await inRoom(w.gid, (sql) => sql.exec("SELECT count(*) AS n FROM topic_relays").one().n)).toBe(1);
    c.close();
  });
});

describe("running a relay", () => {
  it("wakes each step in order, stops on a failure, retries, and ends done", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({
      t: "send",
      groupId: w.gid,
      clientMsgId: clientMsgId(),
      body: "@qa 审 → @dev 写 → @qa 审\n给设置页加深色模式",
      mentions: [w.bot.id, qa.id], // mentions are deduplicated and in another order than the body
    });
    const topicId = sent.message.topicId;
    // Step 1: only qa starts; the message is not handed to dev by its @.
    const s1 = await started(d, qa.id);
    expect(s1.topicId).toBe(topicId);
    expect(s1.prompt).toContain("You do step 1 of 3");
    expect(s1.prompt).toContain("This step: 审");
    expect(s1.prompt).toContain("Goal: 给设置页加深色模式");
    await new Promise((r) => setTimeout(r, 200));
    expect(d.frames.some((f) => f.t === "run.assign" && f.botMemberId === w.bot.id)).toBe(false);
    await relayIs(c, topicId, (r) => r.steps[0].state === "working" && r.steps[0].runId === s1.runId);
    await d.acked(s1.runId, d.done(s1.runId, { status: "succeeded", resultText: "审完了：要用系统颜色", code: { branch: "dark", head: "abcdef1", pushed: true } }));

    // Step 2: dev gets qa's result and code; the topic stays in progress.
    const s2 = await started(d, w.bot.id);
    expect(s2.prompt).toContain("You do step 2 of 3");
    expect(s2.prompt).toContain("审完了：要用系统颜色");
    expect(s2.prompt).toContain("Code: branch dark at abcdef1, pushed.");
    const moved = await relayIs(c, topicId, (r) => r.current === 1 && r.steps[1].state === "working");
    expect(moved.status).toBe("in_progress");
    // dev fails: the relay stops there and the topic waits for whoever started it.
    await d.acked(s2.runId, d.done(s2.runId, { status: "failed", error: "boom" }));
    const paused = await relayIs(c, topicId, (r) => r.state === "paused");
    expect(paused.relay).toMatchObject({ current: 1, reason: "step_failed" });
    expect(paused.relay.steps[1].state).toBe("failed");
    expect(paused.status).toBe("blocked");
    expect(paused.waitingOn).toEqual([w.human.id]);

    // Retry: the same step again, by any person in the group.
    const retried = await api(`/v1/groups/${w.gid}/relays/${paused.relay.id}/retry`, { method: "POST", token: w.owner.token });
    expect(retried.status, JSON.stringify(retried.body)).toBe(200);
    expect(retried.body).toMatchObject({ state: "running", current: 1 });
    expect((await api(`/v1/groups/${w.gid}/relays/${paused.relay.id}/retry`, { method: "POST", token: w.owner.token })).status).toBe(409);
    const s2b = await started(d, w.bot.id, [s2.runId]);
    await d.acked(s2b.runId, d.done(s2b.runId, { status: "succeeded", resultText: "写好了", code: { branch: "dark", head: "abcdef2", pushed: true } }));

    // Step 3 and the end.
    const s3 = await started(d, qa.id, [s1.runId]);
    expect(s3.prompt).toContain("You do step 3 of 3");
    expect(s3.prompt).toContain("写好了");
    await d.acked(s3.runId, d.done(s3.runId, { status: "succeeded", resultText: "没问题" }));
    const done = await relayIs(c, topicId, (r) => r.state === "done");
    expect(done.relay.steps.map((s) => s.state)).toEqual(["done", "done", "done"]);
    expect(done.status).toBe("done");
    expect(await inRoom(w.gid, (sql) => sql.exec<{ status: string }>("SELECT status FROM topics WHERE id = ?", topicId).one().status)).toBe("done");
    d.close();
    c.close();
  });

  it("a person stops it: the step's run stops and the relay is stopped", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev → @qa 加深色模式", mentions: [w.bot.id, qa.id] });
    const topicId = sent.message.topicId;
    const s1 = await started(d, w.bot.id);
    const { relay } = await relayIs(c, topicId, (r) => r.steps[0].state === "working");
    // An AI member cannot stop it.
    expect((await api(`/v1/groups/${w.gid}/relays/${relay.id}/stop`, { method: "POST", token: w.bot.token })).status).toBe(403);
    const stop = await api(`/v1/groups/${w.gid}/relays/${relay.id}/stop`, { method: "POST", token: w.human.token });
    expect(stop.status, JSON.stringify(stop.body)).toBe(200);
    expect(stop.body).toMatchObject({ state: "stopped", stoppedBy: w.human.id });
    await d.next((f) => f.t === "run.cancel" && f.runId === s1.runId);
    await relayIs(c, topicId, (r) => r.state === "stopped" && r.steps[0].state === "stopped");
    expect((await api(`/v1/groups/${w.gid}/relays/${relay.id}/stop`, { method: "POST", token: w.human.token })).status).toBe(409);
    expect((await api(`/v1/groups/${w.gid}/relays/${relay.id}/retry`, { method: "POST", token: w.human.token })).status).toBe(409);
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.some((f) => f.t === "run.assign" && f.botMemberId === qa.id)).toBe(false);
    d.close();
    c.close();
  });

  it("stopping the step's run pauses it; a retry is refused after the third", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev → @qa 加深色模式", mentions: [w.bot.id, qa.id] });
    const topicId = sent.message.topicId;
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      const s = await started(d, w.bot.id, seen);
      seen.push(s.runId);
      expect((await api(`/v1/runs/${s.runId}/cancel`, { method: "POST", token: w.human.token })).status).toBe(200);
      const p = await relayIs(c, topicId, (r) => r.state === "paused" && r.steps[0].runId === s.runId);
      expect(p.relay).toMatchObject({ reason: "step_cancelled", stoppedBy: w.human.id });
      const retry = await api(`/v1/groups/${w.gid}/relays/${p.relay.id}/retry`, { method: "POST", token: w.human.token });
      if (i < 3) expect(retry.status, JSON.stringify(retry.body)).toBe(200);
      else expect([retry.status, retry.body.error]).toMatchObject([409, { code: "conflict", reason: "relay.retry_limit" }]);
      if (i === 3) break;
    }
    d.close();
    c.close();
  });

  it("does not go on to another member past unpushed code", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev → @qa 加深色模式", mentions: [w.bot.id, qa.id] });
    const s1 = await started(d, w.bot.id);
    await d.acked(s1.runId, d.done(s1.runId, { status: "succeeded", resultText: "好了", code: { branch: "dark", head: "abcdef1", pushed: false } }));
    const p = await relayIs(c, sent.message.topicId, (r) => r.state === "paused");
    expect(p.relay).toMatchObject({ reason: "not_pushed", current: 0 });
    d.close();
    c.close();
  });

  it("a step waits while its member works in the topic, and starts when that run ends", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.human.token);
    const first = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@qa 先看看日志", mentions: [qa.id] });
    const topicId = first.message.topicId;
    const busy = await started(d, qa.id);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@qa → @dev 修好日志", mentions: [w.bot.id, qa.id] });
    await relayIs(c, topicId, (r) => r.steps[0].state === "queued");
    await d.acked(busy.runId, d.done(busy.runId, { status: "succeeded", resultText: "看完了" }));
    const s1 = await started(d, qa.id, [busy.runId]);
    expect(s1.prompt).toContain("You do step 1 of 2");
    await relayIs(c, topicId, (r) => r.steps[0].state === "working" && r.steps[0].runId === s1.runId);
    d.close();
    c.close();
  });

  it("a step handed over (转交) ends with the other member's run; a part not back holds it", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const ux = await addBot(w, "ux");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 6 });
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev → @qa 加深色模式", mentions: [w.bot.id, qa.id] });
    const topicId = sent.message.topicId;
    const s1 = await started(d, w.bot.id);
    // dev gives a part out, then ends: the step waits for the part and dev's follow-up.
    d.event(s1.runId, { type: "output", outputId: "o1", op: "member.assign", body: { member: "ux", goal: "画图标" } });
    expect((await d.next((f) => f.t === "run.output.result" && f.outputId === "o1")).ok).toBe(true);
    const part = await started(d, ux.id);
    await d.acked(s1.runId, d.done(s1.runId, { status: "succeeded", resultText: "等图标" }));
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.some((f) => f.t === "run.assign" && f.botMemberId === qa.id)).toBe(false);
    // The part comes back: dev continues once, then hands the rest over to ux (转交).
    await d.acked(part.runId, d.done(part.runId, { status: "succeeded", resultText: "图标好了" }));
    const follow = await started(d, w.bot.id, [s1.runId]);
    d.event(follow.runId, { type: "output", outputId: "o2", op: "member.handoff", body: { member: "ux", goal: "接着做完" } });
    expect((await d.next((f) => f.t === "run.output.result" && f.outputId === "o2")).ok).toBe(true);
    const ho = await started(d, ux.id, [part.runId]);
    await relayIs(c, topicId, (r) => r.steps[0].memberId === ux.id && r.steps[0].runId === ho.runId);
    await d.acked(follow.runId, d.done(follow.runId, { status: "succeeded", resultText: "交给 ux 了" }));
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.some((f) => f.t === "run.assign" && f.botMemberId === qa.id)).toBe(false);
    await d.acked(ho.runId, d.done(ho.runId, { status: "succeeded", resultText: "ux 做完了" }));
    const s2 = await started(d, qa.id);
    expect(s2.prompt).toContain("ux 做完了");
    d.close();
    c.close();
  });
});

describe("the relay rows of the outbox", () => {
  it("a step that cannot be handed over in time pauses the relay (undelivered)", async () => {
    const w = await world();
    const calls: any[] = [];
    await inRoom(w.gid, (sql) => {
      const fake = {
        sql,
        groupId: w.gid,
        commit: <T>(fn: (tx: RoomTx) => T) => fn({} as RoomTx),
        updateTopic: (_tx: RoomTx, _a: string, topicId: string, change: unknown) => calls.push({ topicId, change }),
        wakeAt: () => {},
      } as unknown as RoomContext;
      sql.exec("INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count, owner_id, last_message_at) VALUES ('T9', 't', 'open', 'h', 1, 'M9', 0, 0, NULL, 1)");
      const r = startRelay(fake, {} as RoomTx, w.human.id, "T9", "M9", { goal: "x", steps: [{ memberId: "A" }, { memberId: "B" }] });
      const row = sql.exec<{ payload: string }>("SELECT payload FROM bot_outbox WHERE kind = 'relay'").one();
      giveUpRelayRow(fake, { error: () => {} } as never, row.payload);
      const stored = sql.exec<{ state: string; reason: string }>("SELECT state, reason FROM topic_relays WHERE id = ?", r.id).one();
      expect(stored).toEqual({ state: "paused", reason: "undelivered" });
      sql.exec("DELETE FROM bot_outbox");
    });
    const last = calls[calls.length - 1];
    expect(last.change).toMatchObject({ status: "blocked", waitingOn: [w.human.id], extra: { relay: { state: "paused", reason: "undelivered" } } });
  });

  it("stops with its chain when the person who started it leaves", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const fake = { sql, groupId: w.gid, commit: <T>(fn: (tx: RoomTx) => T) => fn({} as RoomTx), updateTopic: () => {}, wakeAt: () => {} } as unknown as RoomContext;
      sql.exec("INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count, owner_id, last_message_at) VALUES ('T8', 't', 'open', 'h', 1, 'M8', 0, 0, NULL, 1)");
      const r = startRelay(fake, {} as RoomTx, "H1", "T8", "M8", { goal: "x", steps: [{ memberId: "A" }, { memberId: "B" }] });
      requesterLeft(sql, "H1");
      const stored = sql.exec<{ state: string; reason: string; stopped_by: string | null }>("SELECT state, reason, stopped_by FROM topic_relays WHERE id = ?", r.id).one();
      expect(stored).toEqual({ state: "stopped", reason: "requester_left", stopped_by: null });
      // Leaving the group stops it in the event writer, where nothing is published: the hook
      // after the commit publishes the topic with the stopped relay.
      const calls: { topicId: string; change: any }[] = [];
      const pub = { ...fake, updateTopic: (_tx: RoomTx, _a: string, topicId: string, change: unknown) => calls.push({ topicId, change }) } as unknown as RoomContext;
      expect(publishLeftRelays(pub, "H1")).toBe(1);
      expect(calls[calls.length - 1]).toMatchObject({ topicId: "T8", change: { extra: { relay: { state: "stopped", reason: "requester_left" } } } });
      expect(publishLeftRelays(pub, "someone-else")).toBe(0);
      sql.exec("DELETE FROM bot_outbox");
    });
  });

  it("leaving the group stops the relay its requester started, and the topic shows it", async () => {
    const w = await world();
    const qa = await addBot(w, "qa");
    const { d } = await Daemon.connect(w.machineToken, { maxParallelRuns: 4 });
    const { c } = await Client.connect(w.owner.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@qa → @dev 加深色模式", mentions: [w.bot.id, qa.id] });
    const topicId = sent.message.topicId;
    const s1 = await started(d, qa.id);
    // The step ended, the next one has not started: no run's end would publish the stop.
    await d.acked(s1.runId, d.done(s1.runId, { status: "failed", error: "boom" }));
    await relayIs(c, topicId, (r) => r.state === "paused");
    c.close();
    const { c: o } = await Client.connect(w.human.token);
    expect((await api(`/v1/groups/${w.gid}/members/${w.owner.id}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    const stopped = await relayIs(o, topicId, (r) => r.state === "stopped");
    expect(stopped.relay.reason).toBe("requester_left");
    d.close();
    o.close();
  });

  it("a busy step whose member's run ended before the room heard it is handed over again, not left queued", async () => {
    const w = await world();
    await inRoom(w.gid, (sql) => {
      const fake = { sql, groupId: w.gid, commit: <T>(fn: (tx: RoomTx) => T) => fn({} as RoomTx), updateTopic: () => {}, wakeAt: () => {} } as unknown as RoomContext;
      sql.exec("INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count, owner_id, last_message_at) VALUES ('T7', 't', 'open', 'h', 1, 'M7', 0, 0, NULL, 1)");
      const r = startRelay(fake, {} as RoomTx, "H1", "T7", "M7", { goal: "x", steps: [{ memberId: "A" }, { memberId: "B" }] });
      const seq = r.steps[0].seq;
      sql.exec("DELETE FROM bot_outbox");
      const step = () => (JSON.parse(sql.exec<{ steps: string }>("SELECT steps FROM topic_relays WHERE id = ?", r.id).one().steps) as { state: string }[])[0].state;
      // A's busy run ended (its run.updated was handled) while the room waited for the desk.
      const before = Date.now();
      relayStepState(fake, { relayId: r.id, index: 0, seq, state: "queued" });
      expect(step()).toBe("pending");
      const rows = sql.exec<{ next_at: number }>("SELECT next_at FROM bot_outbox WHERE kind = 'relay'").toArray();
      expect(rows).toHaveLength(1);
      expect(rows[0].next_at).toBeGreaterThanOrEqual(before + RELAY_RECHECK_MS);
      // A still works in the topic: the step waits for that run's end.
      sql.exec("DELETE FROM bot_outbox");
      sql.exec("INSERT INTO topic_bots (topic_id, bot_id, run_id, working, started_at) VALUES ('T7', 'A', 'RUN1', 1, 1)");
      relayStepState(fake, { relayId: r.id, index: 0, seq, state: "queued" });
      expect(step()).toBe("queued");
      expect(sql.exec("SELECT 1 FROM bot_outbox WHERE kind = 'relay'").toArray()).toHaveLength(0);
      sql.exec("DELETE FROM topic_bots WHERE topic_id = 'T7'");
    });
  });
});
