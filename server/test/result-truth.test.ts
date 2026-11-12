// Usable work line 3, acceptance path 6: the topic tells the truth about how a task ended.
// 接 4.2 / 5.2 (`chat-run report`), 接 5.1 (test counts from the run log, claims shown as claims),
// problem 5 (one result per run), 接 5.4 (a stop reads the same everywhere) and 接 6.1 (a message
// the room gave up on is said in the topic).
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { ACK_EMOJI } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { endedPhase } from "../src/bots/desk/report";
import { PROOF_TEXT, doneCard, testsFromLog, truthfulDoneCard } from "../src/bots/proof";
import { UNDELIVERED_TEXT } from "../src/bots/room";
import { TEXT } from "../src/bots/dispatch";
import { Daemon, type World, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as BotsEnv;

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

function botMessages(c: Client, topicId: string, botId: string): any[] {
  return c
    .events()
    .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === botId)
    .map((e) => e.data.message);
}

const lastRun = (c: Client, runId: string) => c.events().filter((e) => e.type === "run.updated" && e.data.run.id === runId).at(-1)?.data.run;
const lastTopic = (c: Client, topicId: string) =>
  c
    .events()
    .filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status)
    .at(-1)?.data;

/** A mention of the bot in a new topic; the daemon accepts the run. */
async function started(w: World, c: Client, d: Daemon, body = "@dev 修好登录"): Promise<{ topicId: string; runId: string }> {
  const topicId = await mention(c, w, body);
  const { runId } = await d.next((f) => f.t === "run.assign" && f.topicId === topicId);
  d.send({ t: "run.accept", runId });
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
  return { topicId, runId };
}

async function output(d: Daemon, runId: string, outputId: string, op: string, body: unknown): Promise<any> {
  d.event(runId, { type: "output", outputId, op, body });
  return d.next((f) => f.t === "run.output.result" && f.runId === runId && f.outputId === outputId);
}

async function ended(c: Client, runId: string): Promise<any> {
  await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.endedAt !== undefined);
  return lastRun(c, runId);
}

describe("report rules", () => {
  it("an ended run's phase follows its report while it is the member's latest run in the topic", () => {
    const waiting = { state: "waiting" as const, text: "等你给 staging 的地址" };
    const blocked = { state: "blocked" as const, text: "没有数据库密码" };
    expect(endedPhase("done", null, false)).toEqual({ phase: "done" });
    expect(endedPhase("done", { state: "done", text: "好了" }, false)).toEqual({ phase: "done" });
    expect(endedPhase("done", waiting, false)).toEqual({ phase: "needs_reply", waitingFor: "report" });
    expect(endedPhase("done", blocked, false)).toEqual({ phase: "needs_reply", waitingFor: "stuck" });
    expect(endedPhase("done", waiting, true)).toEqual({ phase: "done" });
    // A failed or stopped run ignores what it said.
    expect(endedPhase("failed", waiting, false)).toEqual({ phase: "failed" });
    expect(endedPhase("cancelled", blocked, false)).toEqual({ phase: "stopped" });
  });
});

describe("chat-run report (接 4.2 / 5.2)", () => {
  it("a run that ends normally but reports blocked leaves the topic blocked on the requester, not done", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    expect(await output(d, runId, "o1", "run.report", { state: "stuck" })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await output(d, runId, "o2", "run.report", { state: "blocked", text: "  " })).toMatchObject({ ok: false, error: { code: "invalid" } });
    expect(await output(d, runId, "o3", "run.report", { state: "blocked", text: "需要 staging 的数据库密码" })).toMatchObject({ ok: true, result: { state: "blocked" } });
    const words = "我做不了：需要你给我 staging 的数据库密码。\n12 passed";
    await d.acked(runId, d.done(runId, { status: "succeeded", resultText: words }));
    const run = await ended(c, runId);
    expect(run).toMatchObject({ status: "done", phase: "needs_reply", waitingFor: "stuck", summary: "需要 staging 的数据库密码" });
    await waitUntil(() => lastTopic(c, topicId)?.status === "blocked", 5000, "topic blocked");
    expect(lastTopic(c, topicId)).toMatchObject({ status: "blocked", waitingOn: [w.human.id] });
    // The member's words go out as they are, to the requester; no 做完了 card.
    const said = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.id === run.resultMessageId), 5000, "result");
    expect(said.body).toBe(words);
    expect(said.cardV2).toBeUndefined();
    expect(said.mentions).toEqual([w.human.id]);

    // The requester answers: the next run of the member makes the old one read as done.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "@dev 密码在 1Password 里", mentions: [w.bot.id] });
    const next = await d.next((f) => f.t === "run.assign" && f.runId !== runId);
    expect(next.topicId).toBe(topicId);
    await waitUntil(() => lastRun(c, runId)?.phase === "done", 5000, "old run answered");
    expect(lastRun(c, runId)).toMatchObject({ status: "done", phase: "done" });
    expect(lastRun(c, runId).waitingFor).toBeUndefined();
    c.close();
    d.close();
  });

  it("a waiting report waits for the requester's answer; the latest report counts; done and no report are done", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const a = await started(w, c, d);
    await output(d, a.runId, "o1", "run.report", { state: "blocked", text: "先卡住" });
    await output(d, a.runId, "o2", "run.report", { state: "waiting", text: "等你选：周五还是周六发" });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "" }));
    expect(await ended(c, a.runId)).toMatchObject({ status: "done", phase: "needs_reply", waitingFor: "report" });
    await waitUntil(() => lastTopic(c, a.topicId)?.status === "waiting_answer", 5000, "topic waits");
    expect(lastTopic(c, a.topicId).waitingOn).toEqual([w.human.id]);
    // With no last words the report text is what the member says.
    await waitUntil(() => botMessages(c, a.topicId, w.bot.id).some((m) => m.body === "等你选：周五还是周六发"), 5000, "report text");

    const b = await started(w, c, d, "@dev 换个话题");
    await output(d, b.runId, "o1", "run.report", { state: "done", text: "发好了" });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "发好了。" }));
    expect(await ended(c, b.runId)).toMatchObject({ status: "done", phase: "done" });
    await waitUntil(() => lastTopic(c, b.topicId)?.status === "done", 5000, "done");

    const e = await started(w, c, d, "@dev 第三件");
    await d.acked(e.runId, d.done(e.runId, { status: "succeeded", resultText: "好了。" }));
    expect(await ended(c, e.runId)).toMatchObject({ status: "done", phase: "done" });
    await waitUntil(() => lastTopic(c, e.topicId)?.status === "done", 5000, "done without a report");
    c.close();
    d.close();
  });

  it("a person's message that reaches the run after its report clears it", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    await output(d, runId, "o1", "run.report", { state: "blocked", text: "需要测试账号" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "账号在 1Password 里" });
    const input = await d.next((f) => f.t === "run.input" && f.runId === runId);
    d.event(runId, { type: "input.accepted", inputId: input.inputId });
    await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "登录修好了。" }));
    expect(await ended(c, runId)).toMatchObject({ status: "done", phase: "done" });
    await waitUntil(() => lastTopic(c, topicId)?.status === "done", 5000, "done after the answer");
    c.close();
    d.close();
  });

  it("a failed run ignores its report", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    await output(d, runId, "o1", "run.report", { state: "waiting", text: "等你回复" });
    await d.acked(runId, d.done(runId, { status: "failed", error: "boom" }));
    expect(await ended(c, runId)).toMatchObject({ status: "failed", phase: "failed" });
    await waitUntil(() => lastTopic(c, topicId)?.status === "blocked", 5000, "failed topic");
    c.close();
    d.close();
  });
});

describe("stop (接 5.4)", () => {
  it("a run the machine reports cancelled leaves the topic as a person's stop does", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const a = await started(w, c, d);
    await d.acked(a.runId, d.done(a.runId, { status: "cancelled" }));
    expect(await ended(c, a.runId)).toMatchObject({ status: "cancelled", phase: "stopped" });
    await waitUntil(() => lastTopic(c, a.topicId)?.status === "open", 5000, "machine stop: open");

    const b = await started(w, c, d, "@dev 另一件");
    expect((await api(`/v1/runs/${b.runId}/cancel`, { token: w.human.token, body: {} })).status).toBe(200);
    expect(await ended(c, b.runId)).toMatchObject({ status: "cancelled", phase: "stopped" });
    await waitUntil(() => lastTopic(c, b.topicId)?.status === "open", 5000, "person stop: open");
    expect(botMessages(c, a.topicId, w.bot.id).map((m) => m.body)).toContain(TEXT.cancelled);
    expect(botMessages(c, b.topicId, w.bot.id).map((m) => m.body)).toContain(TEXT.cancelled);
    c.close();
    d.close();
  });
});

describe("a cancel the daemon gives a reason for (接 5.4)", () => {
  it("leaves the topic blocked: the work did not finish", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    // (The time limit is not such a cancel any more: the work goes on, supervise.test.ts 盯 1.6.)
    await d.acked(runId, d.done(runId, { status: "cancelled", error: "the CLI was killed by the system" }));
    expect(await ended(c, runId)).toMatchObject({ status: "cancelled", phase: "stopped" });
    await waitUntil(() => lastTopic(c, topicId)?.status === "blocked", 5000, "timed out: blocked");
    c.close();
    d.close();
  });
});

describe("one result per run (problem 5)", () => {
  it("a done card the CLI posted itself is the result: the last message is not posted again", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    // A working card does not count as the result.
    const working = { v: 2, rev: 1, intent: "working", title: "在做", fallback: "在做", blocks: [{ type: "text", id: "t", text: "在做" }] };
    expect(await output(d, runId, "o1", "card.post", { cardV2: working })).toMatchObject({ ok: true });
    const done = { v: 2, rev: 1, intent: "done", title: "做完了：登录修好了", fallback: "做完了：登录修好了", blocks: [{ type: "text", id: "t", text: "登录修好了" }] };
    const posted = await output(d, runId, "o2", "card.post", { cardV2: done });
    expect(posted).toMatchObject({ ok: true });
    await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "做完了：登录修好了\n\n登录修好了" }));
    const run = await ended(c, runId);
    expect(run).toMatchObject({ status: "done", resultMessageId: posted.result.messageId });
    await new Promise((r) => setTimeout(r, 200));
    const mine = botMessages(c, topicId, w.bot.id);
    expect(mine.map((m) => m.cardV2?.intent ?? "text")).toEqual(["working", "done"]);
    c.close();
    d.close();
  });
});

describe("a done card the CLI posts tells the truth (接 5.1 with problem 5)", () => {
  const card = (blocks: unknown[]) => ({ v: 2, rev: 1, intent: "done", title: "做完了", fallback: "做完了", blocks });

  it("takes test counts from the log, and shows counts or passes without evidence as claims", () => {
    const blocks = [
      { type: "text", id: "t", text: "好了" },
      { type: "evidence", id: "tests", label: "测试：40 个通过", state: "pass", tests: { passed: 40, failed: 0 } },
      { type: "evidence", id: "a1", label: "能登录", state: "pass" },
      { type: "evidence", id: "a2", label: "E2E", state: "pass", url: "https://ci.example.com/1" },
      { type: "group", id: "g", blocks: [{ type: "evidence", id: "a3", label: "文档", state: "pass", detail: "写了" }] },
    ];
    const none = truthfulDoneCard(card(blocks), null) as any;
    expect(none.blocks[1]).toEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.testsClaimed(40, 0), state: "info" });
    expect(none.blocks[2]).toEqual({ type: "evidence", id: "a1", label: "能登录", state: "info", detail: PROOF_TEXT.claimed });
    expect(none.blocks[3]).toEqual(blocks[3]);
    expect(none.blocks[4].blocks[0]).toEqual({ type: "evidence", id: "a3", label: "文档", state: "info", detail: `${PROOF_TEXT.claimed}：写了` });
    const logged = truthfulDoneCard(card(blocks), { passed: 9, failed: 1 }) as any;
    expect(logged.blocks[1]).toEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.tests(9, 1), state: "fail", tests: { passed: 9, failed: 1 } });
    // A card that is not a done card passes unchanged.
    const working = { ...card(blocks), intent: "working" };
    expect(truthfulDoneCard(working, null)).toBe(working);
  });

  it("the CLI's own done card shows only the tests the run's log shows", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    const claims = card([
      { type: "text", id: "t", text: "好了" },
      { type: "evidence", id: "tests", label: "测试：40 个通过", state: "pass", tests: { passed: 40, failed: 0 } },
      { type: "evidence", id: "a1", label: "能登录", state: "pass" },
    ]);
    expect(await output(d, runId, "o1", "card.post", { cardV2: claims })).toMatchObject({ ok: true });
    await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "好了" }));
    await ended(c, runId);
    const posted = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.cardV2?.intent === "done"), 5000, "done card");
    expect(posted.cardV2.blocks).toContainEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.testsClaimed(40, 0), state: "info" });
    expect(posted.cardV2.blocks).toContainEqual({ type: "evidence", id: "a1", label: "能登录", state: "info", detail: PROOF_TEXT.claimed });
    expect(botMessages(c, topicId, w.bot.id).filter((m) => m.cardV2?.intent === "done")).toHaveLength(1);

    const b = await started(w, c, d, "@dev 再跑一次");
    d.event(b.runId, { type: "tool.call", itemId: "c1", tool: "shell", title: "bun test", input: { command: "bun test" } });
    d.event(b.runId, { type: "tool.result", itemId: "c1", ok: true, output: " 9 pass\n 0 fail" });
    const res = await output(d, b.runId, "o1", "card.post", { cardV2: claims });
    expect(res).toMatchObject({ ok: true });
    expect(res.result.message.cardV2.blocks).toContainEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.tests(9, 0), state: "pass", tests: { passed: 9, failed: 0 } });
    c.close();
    d.close();
  });
});

describe("test counts and acceptance from the run log (接 5.1)", () => {
  it("reads the last test command's results, and only from command tools", () => {
    expect(
      testsFromLog([
        { type: "tool.call", itemId: "c1", tool: "shell", title: "cargo test", input: { command: "cargo test" } },
        { type: "tool.result", itemId: "c1", ok: false, output: "test result: FAILED. 3 passed; 1 failed" },
        { type: "tool.call", itemId: "c2", tool: "Bash", title: "Bash", input: { command: "cargo test --workspace" } },
        { type: "tool.result", itemId: "c2", ok: true, output: "test result: ok. 3 passed; 0 failed\ntest result: ok. 4 passed; 0 failed" },
        // Not a test command, and not a command tool: neither counts.
        { type: "tool.call", itemId: "c3", tool: "shell", title: "cat notes.txt" },
        { type: "tool.result", itemId: "c3", ok: true, output: "99 passed" },
        { type: "tool.call", itemId: "c4", tool: "Read", title: "test/report.txt" },
        { type: "tool.result", itemId: "c4", ok: true, output: "50 passed" },
      ]),
    ).toEqual({ passed: 7, failed: 0 });
    expect(testsFromLog([{ type: "tool.result", itemId: "x", ok: true, output: "12 passed" }])).toBeNull();
    expect(testsFromLog([])).toBeNull();
  });

  it("a count only the last words name is a claim; a criterion without evidence is a claim", () => {
    const card = doneCard({
      runId: "r1",
      result: "好了\n12 passed",
      prs: [],
      artifacts: [],
      acceptance: [
        { text: "能登录", state: "pass" },
        { text: "测试全过", state: "pass", evidence: { text: "我跑过了" } },
        { text: "E2E 全过", state: "pass", evidence: { url: "https://ci.example.com/1" } },
        { text: "文档", state: "fail" },
      ],
    })!;
    expect(card.blocks).toContainEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.testsClaimed(12, 0), state: "info" });
    expect(card.blocks).toContainEqual({ type: "evidence", id: "acceptance-1", label: "能登录", state: "info", detail: PROOF_TEXT.claimed });
    expect(card.blocks).toContainEqual({ type: "evidence", id: "acceptance-2", label: "测试全过", state: "info", detail: `${PROOF_TEXT.claimed}：我跑过了` });
    expect(card.blocks).toContainEqual({ type: "evidence", id: "acceptance-3", label: "E2E 全过", state: "pass", url: "https://ci.example.com/1" });
    expect(card.blocks).toContainEqual({ type: "evidence", id: "acceptance-4", label: "文档", state: "fail" });
    const recorded = doneCard({ runId: "r1", result: "好了\n12 passed", prs: [], artifacts: [], acceptance: [], tests: { passed: 7, failed: 1 } })!;
    expect(recorded.blocks).toContainEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.tests(7, 1), state: "fail", tests: { passed: 7, failed: 1 } });
  });

  it("the done card counts the tests the run ran, not the ones it names", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const { topicId, runId } = await started(w, c, d);
    d.event(runId, { type: "tool.call", itemId: "c1", tool: "shell", title: "bun test", input: { command: "bun test" } });
    d.event(runId, { type: "tool.result", itemId: "c1", ok: true, output: " 9 pass\n 0 fail" });
    await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "修好了。\n全部 40 passed" }));
    const card = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.cardV2), 5000, "done card");
    expect(card.cardV2.blocks).toContainEqual({ type: "evidence", id: "tests", label: PROOF_TEXT.tests(9, 0), state: "pass", tests: { passed: 9, failed: 0 } });
    c.close();
    d.close();
  });
});

describe("a message the room gave up on (接 6.1)", () => {
  it("is said in the topic by the member and its 👀 goes", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const room = benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(w.gid));
    const outbox = () =>
      runInDurableObject(room, (_i, state: DurableObjectState) =>
        state.storage.sql.exec<{ attempts: number }>("SELECT attempts FROM bot_outbox").toArray(),
      );
    // The bot's desk cannot take the mention (it reads bot_runners).
    await env.DB.exec("ALTER TABLE bot_runners RENAME TO bot_runners_gone");
    try {
      const sent = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "@dev 修一下登录页", mentions: [w.bot.id] });
      const topicId = sent.message.topicId as string;
      for (let i = 0; !(await outbox()).some((r) => r.attempts >= 1); i++) {
        if (i > 200) throw new Error("timed out waiting for the failed hand-off");
        await new Promise((r) => setTimeout(r, 20));
      }
      // As if the member had put its 👀 on it before it failed.
      await room.rpc(w.gid, "reactions.set", { id: w.bot.id, level: "member", kind: "bot" }, { messageId: sent.message.id, emoji: ACK_EMOJI, on: true });
      // An hour later the room gives up.
      await runInDurableObject(room, (_i, state: DurableObjectState) => {
        state.storage.sql.exec("UPDATE bot_outbox SET next_at = 0, created_at = 0");
      });
      await runDurableObjectAlarm(room);
      const note = await waitUntil(() => botMessages(c, topicId, w.bot.id).find((m) => m.body.startsWith("「")), 5000, "undelivered note");
      expect(note.body).toBe(UNDELIVERED_TEXT("@dev 修一下登录页"));
      await c.waitEvent((e) => e.type === "reaction.changed" && e.data.messageId === sent.message.id && e.actorId === w.bot.id && e.data.on === false);
      expect(await outbox()).toEqual([]);
      expect(d.frames.filter((f) => f.t === "run.assign")).toHaveLength(0);
    } finally {
      await env.DB.exec("ALTER TABLE bot_runners_gone RENAME TO bot_runners");
    }
    c.close();
    d.close();
  });
});
