// M4 要批准的动作 (docs/protocol.md "Approval rules for AI members", machine-protocol.md "Gated
// actions"): the team's checked kinds go to daemons whose CLI stops them, the card names the
// kind and goes to the kind's approver, 这件事里都同意 answers with scope "run" and the piece of
// work keeps it.
import { env } from "cloudflare:test";
import { DEFAULT_APPROVAL_POLICY, DEFAULT_GATE_PATTERNS } from "@chat/protocol";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import { approvesWholeWork, gatesRuntime } from "../src/bots/dispatch";
import { applyPolicyUpdate, approverOf, patternProblem, policyPrompt, runGates, teamPolicy } from "../src/settings/rules";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, protocolViolations, waitUntil } from "./helpers";

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
    expect(plain).toContain("stop, ask the person who asked you");
    const gated = policyPrompt(team, true) as string;
    expect(gated).toContain("the machine asks the approver");
    expect(gated).toContain("do not ask for it in the topic yourself");
    expect(gated).toContain("merge into the main branch");
    expect(gated).toContain("¥50");
    const owners = applyPolicyUpdate(team, { approvers: { deployProd: "owners" } }, 1);
    expect(policyPrompt(owners)).toContain("deploy or publish to production (ask a team owner)");
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
    // mergeMain's approver is the owners: the owner is asked, the requester cannot answer.
    expect(card.mentions).toEqual([w.owner.id]);
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approveall:daemon-gate-1" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "这件事里都同意" });
    await d.next((f) => f.t === "run.input" && f.text === "这件事里都同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approveall:daemon-gate-1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve", scope: "run" });
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

  it("answer a plain 同意 without scope, and keep the requester as approver of other kinds", async () => {
    const w = await world();
    await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { approvers: { mergeMain: "owners" } } });
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
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
    expect(card.mentions).toEqual([w.human.id]);
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    const ans = await d.next((f) => f.t === "run.approval");
    expect(ans).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    expect(ans.scope).toBeUndefined();
    c.close();
    d.close();
  });

  it("count a plain 好 as input, and a 好 that replies to the approval card as the answer", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["gates"], gates: true });
    const { c } = await Client.connect(w.human.token);
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
    // 好 in a reply to the card answers it.
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "好", replyTo: { messageId: card.id } });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    c.close();
    d.close();
  });

  it("ask the requester, and say so on the card, when the approver is the owners and the group has none", async () => {
    const w = await world();
    await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { approvers: { mergeMain: "owners" } } });
    await env.DB.prepare("DELETE FROM group_members WHERE group_id = ? AND member_id = ?").bind(w.gid, w.owner.id).run();
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
    expect(card.card.blocks[0]).toEqual({ type: "text", text: "这个群里没有管理员，先请你确认。" });
    expect(card.body).toContain("这个群里没有管理员");
    // The requester may answer this one.
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:daemon-gate-1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "daemon-gate-1", decision: "approve" });
    c.close();
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
    expect(a.prompt).toContain("stop, ask the person who asked you for approval in the topic");
    c.close();
    d.close();
  });
});
