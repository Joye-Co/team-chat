// Usable (design-relay R1, 接 1.1/1.2/1.4/1.6/1.12, 盯 3.1/3.2): several AI members in one topic.
// A mention goes to each AI member it names, a message without an @ to the member that most
// recently started a run there; the topic's status is rolled up from every member working in it.
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { ACK_EMOJI } from "@chat/protocol";
import { roomMigrations } from "../src/core/registry";
import { runMigrations } from "../src/core/migrations";
import type { BotsEnv } from "../src/bots/env";
import { addressedParticipants, rollUp, type TopicBotRow } from "../src/bots/participants";
import { cardOwner, handOffEach, messageTargets } from "../src/bots/room";
import { isStopRequest } from "../src/agent/supervise";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, type TestMember, api, clientMsgId, createMember, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

/** A second runner-backed AI member in the world's group (runs on any machine). */
async function addBot(w: World, name: string): Promise<TestMember> {
  const bot = await createMember({ kind: "bot", name });
  expect((await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: bot.id } })).status).toBe(200);
  expect((await api(`/v1/bots/${bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex" } })).status).toBe(200);
  return bot;
}

function room(gid: string) {
  return benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(gid));
}

/** The topic row as the room has it now. */
async function topicRow(gid: string, topicId: string): Promise<{ status: string; owner_id: string | null; waiting_on: string }> {
  return runInDurableObject(room(gid), (_o, state: DurableObjectState) =>
    state.storage.sql.exec("SELECT status, owner_id, waiting_on FROM topics WHERE id = ?", topicId).one(),
  ) as never;
}

async function rows(gid: string, topicId: string): Promise<{ bot_id: string; working: number; run_id: string | null }[]> {
  return runInDurableObject(room(gid), (_o, state: DurableObjectState) =>
    state.storage.sql.exec("SELECT bot_id, working, run_id FROM topic_bots WHERE topic_id = ? ORDER BY started_at", topicId).toArray(),
  ) as never;
}

function lastParticipants(c: Client, topicId: string): any[] | undefined {
  return c
    .events()
    .filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.participants)
    .at(-1)?.data.participants;
}

async function finish(d: Daemon, c: Client, runId: string, status: "succeeded" | "failed" = "succeeded"): Promise<void> {
  await d.acked(runId, d.done(runId, status === "succeeded" ? { status, resultText: "ok" } : { status, error: "boom" }));
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.endedAt !== undefined);
}

const row = (bot: string, x: Partial<TopicBotRow>): TopicBotRow => ({
  topic_id: "t",
  bot_id: bot,
  run_id: null,
  working: 0,
  report: null,
  waiting_on: "[]",
  started_at: 0,
  ended_at: null,
  ...x,
});

describe("routing and roll-up rules", () => {
  it("send a message to each AI member it names, else to the DM peer or the routing owner", () => {
    const usable = (id: string) => id.startsWith("bot");
    expect(messageTargets({ authorId: "h", mentions: ["botA", "botB", "botA", "h2"] }, "botC", undefined, usable)).toEqual([
      { id: "botA", mentioned: true },
      { id: "botB", mentioned: true },
    ]);
    expect(messageTargets({ authorId: "h", mentions: [] }, "botC", undefined, usable)).toEqual([{ id: "botC", mentioned: false }]);
    // A message to people only is not a follow-up for the owner.
    expect(messageTargets({ authorId: "h", mentions: ["h2"] }, "botC", undefined, usable)).toEqual([]);
    expect(messageTargets({ authorId: "h", mentions: [] }, null, "botD", usable)).toEqual([{ id: "botD", mentioned: true }]);
    expect(messageTargets({ authorId: "h", mentions: [] }, "botX", undefined, () => false)).toEqual([]);
    // Without an @, a message that quotes or answers a participant goes to it, not the owner.
    expect(messageTargets({ authorId: "h", mentions: [] }, "botC", undefined, usable, ["botA"])).toEqual([{ id: "botA", mentioned: false }]);
    expect(messageTargets({ authorId: "h", mentions: [] }, "botC", undefined, usable, ["h2"])).toEqual([{ id: "botC", mentioned: false }]);
    // An @ still wins.
    expect(messageTargets({ authorId: "h", mentions: ["botB"] }, "botC", undefined, usable, ["botA"])).toEqual([{ id: "botB", mentioned: true }]);
  });

  it("hand a message to every member even when one of them fails, and retry only the failed one", async () => {
    const targets = [{ id: "botA" }, { id: "botB" }];
    const sent: string[] = [];
    let recorded: string[] = [];
    let failA = true;
    const send = async (t: { id: string }) => {
      if (t.id === "botA" && failA) {
        failA = false;
        throw new Error("desk down");
      }
      sent.push(t.id);
    };
    await expect(handOffEach(targets, [], send, (d) => (recorded = d))).rejects.toThrow("desk down");
    expect(sent).toEqual(["botB"]);
    expect(recorded).toEqual(["botB"]);
    await handOffEach(targets, recorded, send, (d) => (recorded = d));
    expect(sent).toEqual(["botB", "botA"]);
    expect(recorded.sort()).toEqual(["botA", "botB"]);
  });

  it("roll the topic status up from the members working and the ones that ended after the newest start", () => {
    expect(rollUp([row("a", { working: 1, report: "in_progress", started_at: 1 }), row("b", { report: "done", started_at: 2, ended_at: 3 })])).toEqual({ status: "in_progress", waitingOn: [] });
    expect(rollUp([row("a", { working: 1, report: "waiting_answer", waiting_on: '["h"]', started_at: 1 }), row("b", { working: 1, report: "in_progress", started_at: 2 })])).toEqual({ status: "waiting_answer", waitingOn: ["h"] });
    // A got stuck, then B started and finished: only B counts.
    expect(rollUp([row("a", { report: "blocked", started_at: 1, ended_at: 2 }), row("b", { report: "done", started_at: 3, ended_at: 4 })])).toEqual({ status: "done", waitingOn: [] });
    // Both at once, one stuck: stuck.
    expect(rollUp([row("a", { report: "blocked", started_at: 1, ended_at: 5 }), row("b", { report: "done", started_at: 2, ended_at: 9 })])).toEqual({ status: "blocked", waitingOn: [] });
    expect(rollUp([row("a", { report: "waiting_review", waiting_on: '["h"]', started_at: 1, ended_at: 5 }), row("b", { report: "done", started_at: 2, ended_at: 9 })])).toEqual({ status: "waiting_review", waitingOn: ["h"] });
    expect(rollUp([row("a", { started_at: 1, ended_at: 1 })])).toBeNull();
    // A stalled run that has not ended keeps the topic stuck after B finished.
    expect(rollUp([row("a", { working: 1, report: "blocked", started_at: 1 }), row("b", { report: "done", started_at: 3, ended_at: 4 })])).toEqual({ status: "blocked", waitingOn: [] });
  });

  it("read a plain stop word, and nothing longer", () => {
    for (const t of ["停", "停！", "别做了", "@dev 停下", "Stop."]) expect(isStopRequest(t), t).toBe(true);
    for (const t of ["停车场在哪", "别做了这个改那个", "不要停"]) expect(isStopRequest(t), t).toBe(false);
  });
});

describe("several AI members in one topic", () => {
  it("give a message that mentions two AI members to each of them, and list both as working", async () => {
    const w = await world();
    const ops = await addBot(w, "ops");
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev @ops 各看一下", mentions: [w.bot.id, ops.id] });
    const topicId = sent.message.topicId as string;
    const a = await d.next((f) => f.t === "run.assign" && f.botMemberId === w.bot.id);
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === ops.id);
    expect(a.topicId).toBe(topicId);
    expect(b.topicId).toBe(topicId);
    // Each acknowledges the request.
    await waitUntil(() => c.events().filter((e) => e.type === "reaction.changed" && e.data.messageId === sent.message.id && e.data.emoji === ACK_EMOJI).length === 2);
    // Both runs mark their member working on their own: wait for the second mark, not just the second row.
    await waitUntil(() => lastParticipants(c, topicId)?.filter((p) => p.working).length === 2, 5000, "two working participants");
    // Rows are oldest first; the two runs start in their own desks, so either may come first.
    expect(lastParticipants(c, topicId)?.map((p) => [p.memberId, p.working])).toEqual(
      expect.arrayContaining([
        [w.bot.id, true],
        [ops.id, true],
      ]),
    );
    d.send({ t: "run.accept", runId: a.runId });
    d.send({ t: "run.accept", runId: b.runId });
    // One done, one failed at the same time: the topic is stuck, not done.
    await finish(d, c, a.runId);
    expect((await topicRow(w.gid, topicId)).status).toBe("in_progress");
    await finish(d, c, b.runId, "failed");
    await waitUntil(async () => (await topicRow(w.gid, topicId)).status === "blocked", 5000, "blocked");
    expect((await rows(w.gid, topicId)).map((r) => r.working)).toEqual([0, 0]);
    c.close();
    d.close();
  });

  it("make the member that started a run last the routing owner, and finish the topic when it is done after another got stuck", async () => {
    const w = await world();
    const ops = await addBot(w, "ops");
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 做第一部分");
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await finish(d, c, a.runId, "failed");
    await waitUntil(async () => (await topicRow(w.gid, topicId)).status === "blocked", 5000, "blocked");
    expect((await topicRow(w.gid, topicId)).owner_id).toBe(w.bot.id);

    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@ops 你来", mentions: [ops.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === ops.id);
    await waitUntil(async () => (await topicRow(w.gid, topicId)).owner_id === ops.id, 5000, "owner ops");
    const owned = c.events().filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.ownerId).map((e) => e.data.ownerId);
    expect(owned).toEqual([w.bot.id, ops.id]);
    d.send({ t: "run.accept", runId: b.runId });
    await finish(d, c, b.runId);
    await waitUntil(async () => (await topicRow(w.gid, topicId)).status === "done", 5000, "done");

    // A follow-up without an @ from the requester goes to ops (the routing owner) and starts its run (接 1.4).
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "再加个测试" });
    const again = await d.next((f) => f.t === "run.assign" && f.runId !== b.runId && f.runId !== a.runId);
    expect(again.botMemberId).toBe(ops.id);
    expect(again.prompt).toContain("再加个测试");
    c.close();
    d.close();
  });

  it("feed a working run only what is meant for it, and stop it on a plain 停 from its requester", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const { runId } = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");

    // Another person's talk is not input (接 1.6); a stop word from them stops nothing.
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "Alice 这个周五前要吗" });
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "停" });
    // The requester's own message is input.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "周五前就行" });
    await d.next((f) => f.t === "run.input" && f.text === "周五前就行");
    // Another person who mentions the bot is input too.
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 顺便看下日志", mentions: [w.bot.id] });
    await d.next((f) => f.t === "run.input" && f.text === "@dev 顺便看下日志");
    expect(d.frames.filter((f) => f.t === "run.input").map((f) => f.text)).toEqual(["周五前就行", "@dev 顺便看下日志"]);
    expect(d.frames.some((f) => f.t === "run.cancel")).toBe(false);

    // 接 1.12: the requester's 停 stops the run (no front desk is configured).
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "停！" });
    expect(await d.next((f) => f.t === "run.cancel")).toMatchObject({ runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "cancelled");
    expect(d.frames.filter((f) => f.t === "run.input").map((f) => f.text)).toEqual(["周五前就行", "@dev 顺便看下日志"]);
    await waitUntil(async () => (await rows(w.gid, topicId))[0]?.working === 0, 5000, "not working");

    // After it ended, other people's talk without an @ starts nothing, and after a stop neither
    // does the requester's own reply.
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "辛苦" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "好的我自己来" });
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(1);
    c.close();
    co.close();
    d.close();
  });

  it("send a card press to the member that made the card", async () => {
    const w = await world();
    const ops = await addBot(w, "ops");
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev @ops 各看一下", mentions: [w.bot.id, ops.id] });
    const topicId = sent.message.topicId as string;
    await d.next((f) => f.t === "run.assign" && f.botMemberId === ops.id);
    await waitUntil(async () => (await rows(w.gid, topicId)).length === 2, 5000, "two rows");
    // dev is the routing owner or not: either way a press on ops's card goes to ops, and on a
    // card of a member that never ran here to the routing owner.
    const owner = (await topicRow(w.gid, topicId)).owner_id;
        const outsider = "01JBX0BOT000000000000000ZZ";
    const who = await runInDurableObject(room(w.gid), (_o, state: DurableObjectState) => [
      cardOwner(state.storage.sql, topicId, ops.id),
      cardOwner(state.storage.sql, topicId, w.bot.id),
      cardOwner(state.storage.sql, topicId, outsider),
    ]);
    expect(who).toEqual([ops.id, w.bot.id, owner]);
    c.close();
    d.close();
  });

  it("keep participant rows right when a desk never reports, and drop them when the bot is released", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const { c: cb } = await Client.connect(w.bot.token);
    const t = await cb.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "a topic" });
    const topicId = t.message.topicId as string;
    const run = { id: "01JBX0RUN000000000000000AA", botId: w.bot.id, groupId: w.gid, topicId, status: "running", createdAt: Date.now(), updatedAt: Date.now() };
    const actor = { id: w.bot.id, level: "member", kind: "bot" } as const;
    expect(await room(w.gid).rpc(w.gid, "bots.runUpdated", actor, { run })).toMatchObject({ ok: true });
    expect(await rows(w.gid, topicId)).toEqual([{ bot_id: w.bot.id, working: 1, run_id: run.id }]);
    // The run ends without the desk's report (the machine lost it): the row stops working.
    expect(await room(w.gid).rpc(w.gid, "bots.runUpdated", actor, { run: { ...run, status: "failed", updatedAt: Date.now() + 1, endedAt: Date.now() } })).toMatchObject({ ok: true });
    expect(await rows(w.gid, topicId)).toEqual([{ bot_id: w.bot.id, working: 0, run_id: run.id }]);
    await room(w.gid).rpc(w.gid, "bots.release", null, { botId: w.bot.id });
    expect(await rows(w.gid, topicId)).toEqual([]);
    // Clients learn the member left the topic's participants.
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && Array.isArray(e.data.participants) && e.data.participants.length === 0);
    c.close();
    cb.close();
  });

  it("send a quote of a member that is not the routing owner to that member's run", async () => {
    const w = await world();
    const ops = await addBot(w, "ops");
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev @ops 各看一下", mentions: [w.bot.id, ops.id] });
    const topicId = sent.message.topicId as string;
    const a = await d.next((f) => f.t === "run.assign" && f.botMemberId === w.bot.id);
    const b = await d.next((f) => f.t === "run.assign" && f.botMemberId === ops.id);
    d.send({ t: "run.accept", runId: a.runId });
    d.send({ t: "run.accept", runId: b.runId });
    await waitUntil(async () => (await rows(w.gid, topicId)).filter((r) => r.working === 1).length === 2, 5000, "both working");
    const owner = (await topicRow(w.gid, topicId)).owner_id;
    const other = owner === ops.id ? w.bot : ops;
    const otherRun = owner === ops.id ? a.runId : b.runId;
    // The member that is not the routing owner says something; the requester quotes it without an @.
    const { c: cb } = await Client.connect(other.token);
    const said = await cb.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "用方案一还是二？" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "方案二", replyTo: { messageId: said.message.id } });
    const input = await d.next((f) => f.t === "run.input" && String(f.text).includes("方案二"));
    expect(input.runId).toBe(otherRun);
    await new Promise((r) => setTimeout(r, 300));
    expect(d.frames.filter((f) => f.t === "run.input").map((f) => f.runId)).toEqual([otherRun]);
    cb.close();
    c.close();
    d.close();
  });

  it("address a typed approval answer to the member that waits for it", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "a topic" });
    const topicId = sent.message.topicId as string;
    const got = await runInDurableObject(room(w.gid), (_o, state: DurableObjectState) => {
      const sql = state.storage.sql;
      const put = (bot: string, report: string, waitingOn: string[]) =>
        sql.exec("INSERT INTO topic_bots (topic_id, bot_id, working, report, waiting_on, started_at) VALUES (?, ?, 1, ?, ?, 1)", topicId, bot, report, JSON.stringify(waitingOn));
      put("botA", "waiting_answer", [w.human.id]);
      put("botB", "in_progress", []);
      return [
        addressedParticipants(sql, topicId, w.human.id, "同意"),
        addressedParticipants(sql, topicId, w.human.id, "周五前就行"),
        addressedParticipants(sql, topicId, w.owner.id, "同意"),
        addressedParticipants(sql, topicId, w.human.id, "周五前就行", "botB"),
        addressedParticipants(sql, topicId, w.human.id, "周五前就行", "botZ"),
      ];
    });
    expect(got).toEqual([["botA"], [], [], ["botB"], []]);
    c.close();
  });

  it("migrate an older room: its claims become participants and topics list them", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "old topic" });
    const topicId = sent.message.topicId as string;
    const back = await runInDurableObject(room(w.gid), (_o, state: DurableObjectState) => {
      const sql = state.storage.sql;
      // The room as it was before participants-1, with one claim.
      sql.exec("DROP TABLE topic_bots");
      sql.exec("DELETE FROM migrations WHERE name = 'participants-1'");
      sql.exec("INSERT INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)", topicId, w.bot.id, 1000);
      const applied = runMigrations(state.storage, roomMigrations);
      return { applied, rows: sql.exec("SELECT bot_id, working, started_at, ended_at FROM topic_bots WHERE topic_id = ?", topicId).toArray() };
    });
    expect(back.applied).toEqual(["participants-1"]);
    expect(back.rows).toEqual([{ bot_id: w.bot.id, working: 0, started_at: 1000, ended_at: 1000 }]);
    // A title change returns the topic with its participants.
    const upd = await c.ok({ t: "topic.update", groupId: w.gid, topicId, title: "新标题" });
    expect(upd.topic.participants).toEqual([{ memberId: w.bot.id, working: false, lastRunAt: 1000 }]);
    c.close();
  });
});
