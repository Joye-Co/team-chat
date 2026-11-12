// M4 要批准的动作 (docs/protocol.md "Approval rules for AI members", machine-protocol.md "Gated
// actions"): the team's checked kinds go to daemons whose CLI stops them, the card names the
// kind and goes to the kind's approver, 这件事里都同意 answers with scope "run" and the piece of
// work keeps it.
import { env, runInDurableObject } from "cloudflare:test";
import { DEFAULT_APPROVAL_POLICY, DEFAULT_GATE_PATTERNS } from "@chat/protocol";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import { TEXT, approvesWholeWork, gatesRuntime } from "../src/bots/dispatch";
import { applyPolicyUpdate, approverOf, patternProblem, policyPrompt, runGates, teamPolicy } from "../src/settings/rules";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, createMember, protocolViolations, waitUntil } from "./helpers";

const benv = env as unknown as { BOT_DESK: DurableObjectNamespace };
const deskOf = (botId: string) => benv.BOT_DESK.get(benv.BOT_DESK.idFromName(botId));

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

afterEach(async () => {
  await env.DB.prepare("DELETE FROM approval_policies").run();
});

describe("gate rules", () => {
  it("merge team patterns into the defaults and leave out kinds approved for the work", () => {
    const team = teamPolicy(null);
    expect(team).toEqual(DEFAULT_APPROVAL_POLICY);
    const all = runGates(team);
    expect(all.map((g) => g.kind)).toEqual(["mergeMain", "deployProd", "prodConfig", "deleteData", "externalPublish"]);
    expect(all[0].patterns).toEqual([...DEFAULT_GATE_PATTERNS.mergeMain]);
    const p = applyPolicyUpdate(
      team,
      { askBefore: { externalPublish: false }, patterns: { deployProd: ["\\bdeploy-prod\\.sh\\b"] }, approvers: { deployProd: "owners" } },
      7,
    );
    const gates = runGates(p, ["mergeMain"]);
    expect(gates.map((g) => g.kind)).toEqual(["deployProd", "prodConfig", "deleteData"]);
    expect(gates[0].patterns).toContain("\\bdeploy-prod\\.sh\\b");
    expect(gates[0].patterns).toContain(DEFAULT_GATE_PATTERNS.deployProd[0]);
    // [] clears a kind; other kinds and approvers stay.
    const cleared = applyPolicyUpdate(p, { patterns: { deployProd: [] }, approvers: { mergeMain: "requester" } }, 8);
    expect(cleared.patterns).toBeUndefined();
    expect(cleared.approvers).toEqual({ deployProd: "owners", mergeMain: "requester" });
    expect(approverOf(cleared, "deployProd")).toBe("owners");
    expect(approverOf(cleared, "mergeMain")).toBe("requester");
    expect(approverOf(cleared, null)).toBe("requester");
    // Stored policies of any older shape still read.
    expect(teamPolicy({ ...p, patterns: { deployProd: [] }, approvers: { deployProd: "anyone" } } as never)).not.toHaveProperty("approvers");
  });

  it("check team patterns against the shared regex subset", () => {
    expect(patternProblem("\\bdeploy\\.sh\\b")).toBeNull();
    expect(patternProblem("(unclosed")).not.toBeNull();
    expect(patternProblem("deploy(?!-dry)")).not.toBeNull();
    expect(patternProblem("(?<=x)y")).not.toBeNull();
    expect(patternProblem("(a)\\1")).not.toBeNull();
    for (const list of Object.values(DEFAULT_GATE_PATTERNS)) for (const x of list) expect(patternProblem(x), x).toBeNull();
  });

  it("write the gated rule: the machine asks, the topic only for other ways and spending", () => {
    const team = teamPolicy(null);
    const plain = policyPrompt(team) as string;
    // Gated actions go to the computer's approvers: an admin on a team computer.
    expect(plain).toContain("stop, ask a team admin for approval in the topic");
    expect(plain).toContain("- merge into the main branch");
    // Spending keeps the team's approver.
    expect(plain).toContain("Also: before you spend more than ¥50 at once (paid APIs, cloud resources, purchases), stop, ask the person who asked you");
    const gated = policyPrompt(team, true) as string;
    expect(gated).toContain("these need the approval of a team admin");
    expect(gated).toContain("the machine asks the approver");
    expect(gated).toContain("do not ask for it in the topic yourself");
    expect(gated).toContain("merge into the main branch");
    expect(gated).toContain("first ask a team admin in the topic");
    expect(gated).toContain("before you spend more than ¥50 at once (paid APIs, cloud resources, purchases), ask the person who asked you");
    // On a personal computer, its owner.
    expect(policyPrompt(team, false, "owner")).toContain("stop, ask the owner of this computer for approval in the topic");
    const own = policyPrompt(team, true, "owner") as string;
    expect(own).toContain("these need the approval of the owner of this computer");
    expect(own).toContain("first ask the owner of this computer in the topic");
    expect(own).not.toContain("a team admin");
    // Per-kind approvers no longer change who approves a gated action; the team approver still
    // answers spending.
    const owners = applyPolicyUpdate(team, { approvers: { deployProd: "owners", mergeMain: "requester" }, approver: "owners" }, 1);
    const ownersText = policyPrompt(owners) as string;
    expect(ownersText).not.toContain("(ask");
    expect(ownersText).toContain("stop, ask a team admin for approval");
    expect(ownersText).toContain("¥50 at once (paid APIs, cloud resources, purchases), stop, ask a team owner");
    const spendOnly = applyPolicyUpdate(team, { askBefore: { mergeMain: false, deployProd: false, prodConfig: false, deleteData: false, externalPublish: false } }, 1);
    expect(policyPrompt(spendOnly)).toBe("Team rule: before you spend more than ¥50 at once (paid APIs, cloud resources, purchases), stop, ask the person who asked you for approval in the topic and wait for the answer.");
    const none = applyPolicyUpdate(team, { askBefore: { mergeMain: false, deployProd: false, prodConfig: false, deleteData: false, externalPublish: false }, maxSpendCny: null }, 1);
    expect(policyPrompt(none, true)).toBeUndefined();
  });

  it("read 这件事里都同意 and which machines stop gated commands", () => {
    expect(approvesWholeWork("这件事里都同意")).toBe(true);
    expect(approvesWholeWork("@dev 都同意。")).toBe(true);
    expect(approvesWholeWork("同意")).toBe(false);
    const cli = { runtime: "claude", kind: "claude" as const, command: "claude", available: true };
    expect(gatesRuntime({ info: { features: ["gates"] } as never, clis: [{ ...cli, gates: true }] }, "claude")).toBe(true);
    expect(gatesRuntime({ info: { features: ["gates"] } as never, clis: [cli] }, "claude")).toBe(false);
    expect(gatesRuntime({ info: { features: [] } as never, clis: [{ ...cli, gates: true }] }, "claude")).toBe(false);
    expect(gatesRuntime({ info: { features: ["gates"] } as never, clis: [{ ...cli, gates: true }] }, "codex")).toBe(false);
  });
});

describe("gated approvals", () => {
  it("keep team patterns and per-kind approvers in the settings, refusing patterns the daemon cannot run", async () => {
    const w = await world();
    const bad = await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { patterns: { deployProd: ["deploy(?!-dry)"] } } });
    expect(bad.status).toBe(400);
    const up = await api("/v1/settings/approvals", {
      method: "PATCH",
      token: w.owner.token,
      body: { patterns: { deployProd: ["\\bdeploy-prod\\.sh\\b"] }, approvers: { mergeMain: "owners" } },
    });
    expect(up.status, JSON.stringify(up.body)).toBe(200);
    expect(up.body).toMatchObject({ patterns: { deployProd: ["\\bdeploy-prod\\.sh\\b"] }, approvers: { mergeMain: "owners" }, approver: "requester" });
    const got = await api("/v1/settings/approvals", { token: w.human.token });
    expect(got.body).toEqual(up.body);
  });

  it("send gates to a gated CLI, ask the kind's approver with 这件事里都同意, and keep it for the work", async () => {
    const w = await world();
    await api("/v1/settings/approvals", {
      method: "PATCH",
      token: w.owner.token,
      body: { patterns: { mergeMain: ["\\bship-it\\b"] }, approvers: { mergeMain: "owners" } },
    });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: w.owner.id } });
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w, "@dev ship it");
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.gates.map((g: { kind: string }) => g.kind)).toEqual(["mergeMain", "deployProd", "prodConfig", "deleteData", "externalPublish"]);
    expect(a.gates[0].patterns).toEqual([...DEFAULT_GATE_PATTERNS.mergeMain, "\\bship-it\\b"]);
    expect(a.prompt).toContain("the machine asks the approver");
    const runId = a.runId;
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");

    d.event(runId, { type: "approval.request", approvalId: "daemon-gate-1", title: "合并到 main", detail: "gh pr merge 12", kind: "mergeMain", command: "gh pr merge 12" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "approval card",
    );
    expect(card.card.title).toBe("需要你确认：合并到 main");
    expect(card.card.blocks).toEqual([{ type: "code", text: "gh pr merge 12" }]);
    expect(card.card.actions.map((x: { id: string; label: string }) => [x.id, x.label])).toEqual([
      ["approve:daemon-gate-1", "同意"],
      ["approveall:daemon-gate-1", "这件事里都同意"],
      ["deny:daemon-gate-1", "拒绝"],
    ]);
    expect(card.body).toContain("这件事里都同意");
    // A gated action on a team computer: the admin is asked, the requester cannot answer.
    expect(card.mentions).toEqual([w.owner.id]);
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approveall:daemon-gate-1" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "这件事里都同意" });
    await d.next((f) => f.t === "run.input" && f.text === "这件事里都同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approveall:daemon-gate-1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve", scope: "run" });
    // The card changes in place: no buttons, the outcome in the title, who and when in one line.
    const closed = (await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === card.id && e.data.card?.actions.length === 0)).data;
    expect(closed.card).toMatchObject({ title: "已同意：合并到 main", tone: "success" });
    expect(closed.card.blocks[0]).toEqual({ type: "code", text: "gh pr merge 12" });
    expect(closed.card.blocks[1].text).toMatch(/同意了，这件事里同类的操作不再问 · \d\d:\d\d$/);
    d.event(runId, { type: "approval.resolved", approvalId: "daemon-gate-1", decision: "approve" });

    // The requester's message the CLI never took becomes a follow-up run: the same piece of
    // work, so merges are no longer gated.
    const seq = d.done(runId, { status: "succeeded", resultText: "merged" });
    await d.acked(runId, seq);
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== runId);
    expect(b.gates.map((g: { kind: string }) => g.kind)).toEqual(["deployProd", "prodConfig", "deleteData", "externalPublish"]);
    d.send({ t: "run.accept", runId: b.runId });
    const seq2 = d.done(b.runId, { status: "failed", error: "tests failed" });
    await d.acked(b.runId, seq2);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.status === "failed");
    // A retry of the follow-up keeps it too.
    const again = await api(`/v1/runs/${b.runId}/retry`, { token: w.human.token, body: {} });
    expect(again.status, JSON.stringify(again.body)).toBe(200);
    const r3 = await d.next((f) => f.t === "run.assign" && f.runId === again.body.id);
    expect(r3.gates.map((g: { kind: string }) => g.kind)).not.toContain("mergeMain");
    c.close();
    co.close();
    d.close();
  });

  it("answer an admin's plain 同意 without scope, whoever the kind's approver says", async () => {
    const w = await world();
    // A per-kind approver is kept but no longer decides a gated action (design-github-identity 3.5).
    await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { approvers: { externalPublish: "requester" } } });
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    d.event(a.runId, { type: "approval.request", approvalId: "daemon-gate-1", title: "对外发布", kind: "externalPublish", command: "npm publish" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "approval card",
    );
    expect(card.mentions).toEqual([w.owner.id]);
    // The requester's 同意 is plain input.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    await d.next((f) => f.t === "run.input" && f.text === "同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    const ans = await d.next((f) => f.t === "run.approval");
    expect(ans).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    expect(ans.scope).toBeUndefined();
    const closed = (await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === card.id && e.data.card)).data;
    expect(closed.card).toMatchObject({ title: "已同意：对外发布", actions: [] });
    d.event(a.runId, { type: "approval.resolved", approvalId: "daemon-gate-1", decision: "approve" });

    // A second card that the run ends without: it says it was not needed, the buttons go.
    d.event(a.runId, { type: "approval.request", approvalId: "daemon-gate-2", title: "对外发布", kind: "externalPublish", command: "npm publish --tag next" });
    const second = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.kind === "card" && e.data.message.id !== card.id)?.data.message,
      5000,
      "second approval card",
    );
    await d.acked(a.runId, d.done(a.runId, { status: "failed", error: "gave up" }));
    const expired = (await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === second.id && e.data.card)).data;
    expect(expired.card).toMatchObject({ title: "没用上：对外发布", tone: "neutral", actions: [] });
    c.close();
    co.close();
    d.close();
  });

  it("count a plain 好 as input, and an admin's 好 that replies to the approval card as the answer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    d.event(a.runId, { type: "approval.request", approvalId: "daemon-gate-1", title: "对外发布", kind: "externalPublish", command: "npm publish" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "approval card",
    );
    // 好 / 行 / ok said in the topic: plain input for the run, nothing is approved.
    let other = "";
    for (const body of ["好的", "行", "ok"]) {
      other = (await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body })).message.id;
      await d.next((f) => f.t === "run.input" && f.text === body);
    }
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    // 好 that quotes another message is input too.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "好", replyTo: { messageId: other } });
    await d.next((f) => f.t === "run.input" && f.text.includes("好"));
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    // The requester's 好 in a reply to the card is input: a gated action is not theirs to approve.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "好", replyTo: { messageId: card.id } });
    await waitUntil(() => d.frames.filter((f) => f.t === "run.input" && f.text !== "好的" && f.text.includes("好")).length === 2, 5000, "the requester's 好 as input");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    // An admin's 好 in a reply to the card answers it.
    await co.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "好", replyTo: { messageId: card.id } });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    c.close();
    co.close();
    d.close();
  });

  it("let nobody answer a gated action when the group has no admin, and say so to the requester", async () => {
    const w = await world();
    // A per-kind approver of "requester" does not bring back the requester.
    await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { approvers: { mergeMain: "requester" } } });
    expect((await api(`/v1/groups/${w.gid}/members/${w.owner.id}`, { method: "DELETE", token: w.owner.token })).status).toBe(200);
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    d.event(a.runId, { type: "approval.request", approvalId: "daemon-gate-1", title: "合并到 main", kind: "mergeMain", command: "gh pr merge 3" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "approval card",
    );
    expect(card.mentions).toEqual([w.human.id]);
    expect(card.card.blocks[0]).toEqual({ type: "text", text: TEXT.approvalNoAdmin });
    expect(card.body).toContain(TEXT.approvalNoAdmin);
    expect(card.body).not.toContain(TEXT.approvalNoOwner);
    // The requester cannot answer it, by card or by text; the run keeps waiting.
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:daemon-gate-1" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    await d.next((f) => f.t === "run.input" && f.text === "同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    // An admin who joins the group can.
    expect((await api(`/v1/groups/${w.gid}/members`, { token: w.human.token, body: { memberId: w.owner.id } })).status).toBe(200);
    const { c: co } = await Client.connect(w.owner.token);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:daemon-gate-1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    c.close();
    co.close();
    d.close();
  });

  it("let an intern answer a plain approval on a team computer but only an admin a merge to main, and wait on the admins", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    // A plain approval (no kind): the requester, as before.
    d.event(a.runId, { type: "approval.request", approvalId: "plain-1", title: "git push origin bot/x" });
    const plain = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "plain card",
    );
    expect(plain.mentions).toEqual([w.human.id]);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "plain-1", decision: "approve" });
    d.event(a.runId, { type: "approval.resolved", approvalId: "plain-1", decision: "approve" });

    // A merge to main: the admins are asked and the topic waits on them.
    d.event(a.runId, { type: "approval.request", approvalId: "gate-1", title: "合并到 main", kind: "mergeMain", command: "gh pr merge 7" });
    const gated = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.kind === "card" && e.data.message.id !== plain.id)?.data.message,
      5000,
      "gated card",
    );
    expect(gated.mentions).toEqual([w.owner.id]);
    expect(gated.card.blocks).toEqual([{ type: "code", text: "gh pr merge 7" }]);
    await c.waitEvent((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status === "waiting_answer" && e.data.waitingOn?.[0] === w.owner.id);
    await c.ok({ t: "card.action", groupId: w.gid, messageId: gated.id, actionId: "approveall:gate-1" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    await d.next((f) => f.t === "run.input" && f.text === "同意");
    expect(d.frames.filter((f) => f.t === "run.approval" && f.approvalId === "gate-1")).toEqual([]);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: gated.id, actionId: "approve:gate-1" });
    expect(await d.next((f) => f.t === "run.approval" && f.approvalId === "gate-1")).toMatchObject({ decision: "approve" });
    const audit = await env.DB.prepare("SELECT actor_id FROM audit WHERE action = 'run.approval' AND target = ? ORDER BY id DESC LIMIT 1")
      .bind(a.runId)
      .first<{ actor_id: string }>();
    expect(audit?.actor_id).toBe(w.owner.id);
    c.close();
    co.close();
    d.close();
  });

  it("let only the owner of a personal computer answer a gated action there", async () => {
    const w = await world();
    const intern = await createMember({ name: "Ivy" });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: intern.id } });
    // Alice's own computer, with the member checked on it.
    const labels = (await api(`/v1/bots/${w.bot.id}/runner`, { token: w.owner.token })).body.labels;
    const mac = await api("/v1/machines", { token: w.human.token, body: { name: "alice-mac", labels, personal: true } });
    expect(mac.status, JSON.stringify(mac.body)).toBe(200);
    expect((await api(`/v1/machines/${mac.body.machine.id}/bots/${w.bot.id}`, { method: "PUT", token: w.human.token, body: {} })).status).toBe(200);
    const { d } = await Daemon.connect(mac.body.token, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const { c: ci } = await Client.connect(intern.token);
    const topicId = await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    expect(a.prompt).toContain("these need the approval of the owner of this computer");
    // Someone else asked for this piece of work (for example a follow-up the run took).
    await runInDurableObject(deskOf(w.bot.id), (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE runs SET requested_by = ? WHERE id = ?", intern.id, a.runId);
    });

    d.event(a.runId, { type: "approval.request", approvalId: "gate-1", title: "合并到 main", kind: "mergeMain", command: "gh pr merge 9" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "gated card",
    );
    expect(card.mentions).toEqual([w.human.id]);
    // Neither the admin nor the requester answers it.
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:gate-1" });
    await ci.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:gate-1" });
    await new Promise((r) => setTimeout(r, 100));
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:gate-1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "gate-1", decision: "approve" });
    d.event(a.runId, { type: "approval.resolved", approvalId: "gate-1", decision: "approve" });

    // The owner leaves the group: the requester is told, and still cannot answer.
    expect((await api(`/v1/groups/${w.gid}/members/${w.human.id}`, { method: "DELETE", token: w.human.token })).status).toBe(200);
    d.event(a.runId, { type: "approval.request", approvalId: "gate-2", title: "部署", kind: "deployProd", command: "./deploy.sh prod" });
    const second = await waitUntil(
      () => ci.events().find((e) => e.type === "message.created" && e.data.message.kind === "card" && e.data.message.id !== card.id)?.data.message,
      5000,
      "second card",
    );
    expect(second.mentions).toEqual([intern.id]);
    expect(second.card.blocks[0]).toEqual({ type: "text", text: TEXT.approvalNoMachineOwner });
    await ci.ok({ t: "card.action", groupId: w.gid, messageId: second.id, actionId: "approve:gate-2" });
    await co.ok({ t: "card.action", groupId: w.gid, messageId: second.id, actionId: "approve:gate-2" });
    await new Promise((r) => setTimeout(r, 100));
    expect(d.frames.filter((f) => f.t === "run.approval" && f.approvalId === "gate-2")).toEqual([]);
    c.close();
    co.close();
    ci.close();
    d.close();
  });

  it("keep the rules in the prompt and send no gates to a CLI that cannot stop commands", async () => {
    const w = await world();
    // The daemon knows gates, its CLI does not report that it stops commands (an older daemon).
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.gates).toBeUndefined();
    expect(a.prompt).toContain("stop, ask a team admin for approval in the topic");
    c.close();
    d.close();
  });
});
