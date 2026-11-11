// 思考强度 (docs/protocol.md "Bot runs", machine-protocol.md "Models and effort"): the runner's
// effort is stored (absent keeps, null clears), reaches only daemons with feature "effort", and
// machines show the models and efforts their CLIs report.
import { afterAll, describe, expect, it } from "vitest";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

describe("effort", () => {
  it("stores the effort, keeps it when absent, clears it with null", async () => {
    const w = await world();
    const put = (body: Record<string, unknown>) => api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", ...body } });
    expect((await put({ model: "gpt-6-sol", effort: "high" })).body).toMatchObject({ model: "gpt-6-sol", effort: "high" });
    expect((await put({ model: "gpt-6-sol" })).body.effort).toBe("high");
    expect((await put({ effort: null })).body.effort).toBeUndefined();
    expect((await put({ effort: "High" })).status).toBe(400);
  });

  it("sends the effort only to a daemon with feature effort", async () => {
    const w = await world();
    const runner = await api(`/v1/bots/${w.bot.id}/runner`, { method: "GET", token: w.owner.token });
    await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: runner.body.labels, effort: "xhigh", model: "gpt-6-astra" } });
    const models = [{ id: "gpt-6-astra", name: "GPT-6-Astra", efforts: ["low", "xhigh"], isDefault: true }];
    const { d } = await Daemon.connect(w.machineToken, { features: ["effort"], models, efforts: ["low", "xhigh"] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a).toMatchObject({ model: "gpt-6-astra", effort: "xhigh" });
    const machines = await api("/v1/machines", { method: "GET", token: w.owner.token });
    const mine = (machines.body as any[]).find((m) => m.id === w.machineId);
    expect(mine.clis[0].models[0]).toMatchObject({ id: "gpt-6-astra", efforts: ["low", "xhigh"] });
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    d.close();

    const old = await Daemon.connect(w.machineToken, {});
    await mention(c, w, "@dev again");
    const b = await old.d.next((f) => f.t === "run.assign");
    expect(b.effort).toBeUndefined();
    expect(b.model).toBe("gpt-6-astra");
    old.d.close();
    c.close();
  });

  it("uses the team's model and effort for a CLI when the member has none of its own", async () => {
    const w = await world();
    const runner = (await api(`/v1/bots/${w.bot.id}/runner`, { method: "GET", token: w.owner.token })).body;
    // Members read the team's choices, only owners change them.
    expect((await api("/v1/settings/models", { token: w.human.token })).body).toEqual({ workers: [] });
    expect((await api(`/v1/settings/models/${runner.runtime}`, { method: "PUT", token: w.human.token, body: { model: "opus" } })).status).toBe(403);
    expect((await api("/v1/settings/models/Claude Code", { method: "PUT", token: w.owner.token, body: { model: "opus" } })).status).toBe(400);
    const set = await api(`/v1/settings/models/${runner.runtime}`, { method: "PUT", token: w.owner.token, body: { model: "team-model", effort: "high" } });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    expect(set.body).toMatchObject({ runtime: runner.runtime, model: "team-model", effort: "high" });
    // Absent keeps, null clears.
    expect((await api(`/v1/settings/models/${runner.runtime}`, { method: "PUT", token: w.owner.token, body: { effort: "low" } })).body).toMatchObject({ model: "team-model", effort: "low" });
    expect((await api("/v1/settings/models", { token: w.human.token })).body.workers).toEqual([expect.objectContaining({ runtime: runner.runtime, model: "team-model", effort: "low" })]);

    const { d } = await Daemon.connect(w.machineToken, { features: ["effort"], efforts: ["low", "high"] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const a = await d.next((f) => f.t === "run.assign");
    expect(a).toMatchObject({ model: "team-model", effort: "low" });
    d.send({ t: "run.accept", runId: a.runId });
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));

    // The member's own model wins; the team's effort still fills in.
    await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: runner.runtime, labels: runner.labels, model: "own-model" } });
    await mention(c, w, "@dev again");
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b).toMatchObject({ model: "own-model", effort: "low" });
    d.close();
    c.close();
  });
});
