// 活动记录: admin events of the audit log copied by the 0013 trigger, filtered, owners only, 90 days.
import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { ActivityResponse } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { OFFLINE_GRACE_MS, weekStart } from "../src/activity/events";
import { resetPriceCache } from "../src/usage/prices";
import type { BotsEnv } from "../src/bots/env";
import { Daemon, mention, world } from "./daemon";
import { Client, api, createMember, waitUntil } from "./helpers";

async function activity(token: string, query = "") {
  const r = await api(`/v1/activity${query ? `?${query}` : ""}`, { token });
  return r;
}

describe("activity", () => {
  it("records level and budget changes, invites, machines and keys; owners read and filter them", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const level = await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { level: "owner" } });
    expect(level.status, JSON.stringify(level.body)).toBe(200);
    await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { weeklyBudgetCents: 5000 } });
    await api(`/v1/members/${alice.id}`, { method: "PATCH", token: owner.token, body: { level: "member" } });
    const m = await api("/v1/machines", { token: owner.token, body: { name: "runner-x", labels: [] } });
    expect(m.status).toBe(200);
    const machineId = m.body.machine.id as string;
    // Writes that are not admin events stay out.
    await api("/v1/invites", { token: owner.token, body: { name: "Lina", kind: "human", level: "member" } });

    expect((await activity(alice.token)).status).toBe(403);
    const all = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}`)).body);
    const kinds = all.entries.map((e) => [e.kind, e.action]);
    expect(kinds).toEqual([
      ["invite", "invite.create"],
      ["machine", "machine.create"],
      ["level", "member.update"],
      ["budget", "member.update"],
      ["level", "member.update"],
    ]);
    expect(all.entries[2]).toMatchObject({ target: alice.id, details: { level: ["owner", "member"] } });
    expect(all.entries[1]).toMatchObject({ machineId, target: machineId });

    const budget = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&kind=budget`)).body);
    expect(budget.entries.map((e) => e.details)).toEqual([{ weeklyBudgetCents: [null, 5000] }]);
    const onMachine = ActivityResponse.parse((await activity(owner.token, `machine=${machineId}`)).body);
    expect(onMachine.entries.map((e) => e.action)).toEqual(["machine.create"]);
    const page = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&limit=2`)).body);
    expect(page.hasMore).toBe(true);
    const next = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}&limit=2&before=${page.entries[1].id}`)).body);
    expect(next.entries[0].id).toBeLessThan(page.entries[1].id);
    expect((await activity(owner.token, "kind=chat")).status).toBe(400);
  });

  it("leaves out entries older than 90 days", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const old = Date.now() - 91 * 86_400_000;
    await env.DB.prepare("INSERT INTO audit (actor_id, action, target, details, at) VALUES (?, 'machine.revoke', 'm-old', '{}', ?)")
      .bind(owner.id, old)
      .run();
    const r = ActivityResponse.parse((await activity(owner.token, `actor=${owner.id}`)).body);
    expect(r.entries.some((e) => e.target === "m-old")).toBe(false);
  });
});

describe("activity the server notices", () => {
  const benv = env as unknown as BotsEnv;
  const linkOf = (machineId: string) => benv.MACHINE_LINK.get(benv.MACHINE_LINK.idFromName(machineId));
  const entries = async (token: string, q: string) => ActivityResponse.parse((await activity(token, q)).body).entries;
  /** The entries once `ok` holds (writes after a run are not awaited by the request). */
  const entriesWhen = async (token: string, q: string, ok: (e: Awaited<ReturnType<typeof entries>>) => boolean, what: string) => {
    for (let i = 0; i < 50; i++) {
      const e = await entries(token, q);
      if (ok(e)) return e;
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error(`timed out waiting for ${what}`);
  };

  it("records a failed run and, once a week, the AI member reaching its budget", async () => {
    const w = await world();
    await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { weeklyBudgetCents: 100 } });
    await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: [], machineId: w.machineId } });
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    const { c } = await Client.connect(w.human.token);
    await mention(c, w);
    const assign = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: assign.runId });
    // Codex reports its model and tokens, no cost: 1M input tokens at list price pass the $1 budget.
    resetPriceCache();
    const done = d.done(assign.runId, {
      status: "failed",
      usage: { inputTokens: 1_000_000, outputTokens: 0, models: [{ model: "gpt-5-codex", inputTokens: 1_000_000, outputTokens: 0 }] },
      error: "boom",
    });
    await d.acked(assign.runId, done);
    const got = await entriesWhen(w.owner.token, "kind=run,budget", (e) => e.some((x) => x.action === "run.failed") && e.some((x) => x.action === "budget.reached" && x.target === w.bot.id), "run and budget activity");
    const failed = got.find((e) => e.action === "run.failed");
    expect(failed).toMatchObject({ kind: "run", actorId: w.bot.id, machineId: w.machineId, details: { runId: assign.runId, requestedBy: w.human.id } });
    const reached = got.find((e) => e.action === "budget.reached");
    expect(reached).toMatchObject({ kind: "budget", target: w.bot.id, details: { budgetCents: 100 } });
    expect((reached?.details as { spentCents: number }).spentCents).toBeGreaterThanOrEqual(100);
    d.close();
  });

  it("records a machine offline only after the grace, and its return", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    // A quick reconnect within the grace writes nothing.
    const { d: d2 } = await Daemon.connect(w.machineToken, { labels: [] });
    await runDurableObjectAlarm(linkOf(w.machineId));
    expect((await entries(w.owner.token, `machine=${w.machineId}&kind=machine`)).filter((e) => e.action !== "machine.create")).toEqual([]);
    d2.close();
    await waitUntil(() => d2.closed, 5000, "closed again");
    // Away past the grace: 离线, then 重新上线 on the next hello.
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'offline_since'", String(Date.now() - OFFLINE_GRACE_MS - 1000));
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    const off = await entries(w.owner.token, `machine=${w.machineId}&kind=machine`);
    expect(off.map((e) => e.action)).toContain("machine.offline");
    const { d: d3 } = await Daemon.connect(w.machineToken, { labels: [] });
    const on = await entriesWhen(w.owner.token, `machine=${w.machineId}&kind=machine`, (e) => e.some((x) => x.action === "machine.online"), "back online");
    expect(on.find((e) => e.action === "machine.online")?.details).toMatchObject({ offlineMs: expect.any(Number) });
    d3.close();
  });

  it("counts weeks from Monday 00:00 in the team's zone", () => {
    // Wednesday 2026-11-11 10:00 in Shanghai is 02:00 UTC; its week starts Monday 2026-11-09 00:00 +08.
    expect(weekStart(Date.UTC(2026, 10, 11, 2), "Asia/Shanghai")).toBe(Date.UTC(2026, 10, 8, 16));
    // Monday 00:30 in Shanghai is still Sunday in UTC.
    expect(weekStart(Date.UTC(2026, 10, 8, 16, 30), "Asia/Shanghai")).toBe(Date.UTC(2026, 10, 8, 16));
    expect(weekStart(Date.UTC(2026, 10, 15, 23), "UTC")).toBe(Date.UTC(2026, 10, 9));
  });
});
