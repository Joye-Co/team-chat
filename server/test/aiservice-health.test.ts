// The AI 服务 is dependable and says when it is not working (usable work line 4): per-use
// switches, the status flag every member reads, the last call of each use, the failing alert and
// its 活动记录 entries, a key that cannot be read, one retry policy, tokens on 用量, and the
// weekly budget leaving out runs on a subscription sign-in. Provider calls go to a fake fetch.
import { ActivityResponse, AiServiceConfig, AiServiceStatus, TeamUsage } from "@chat/protocol";
import { env } from "cloudflare:test";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SECRET_MISSING, TEST_CHECK_MS, requireSecret } from "../src/aiservice";
import { callModel, completionUsage } from "../src/aiservice/providers";
import { BACKOFF_MS, retryAfterMs, retryable } from "../src/aiservice/retry";
import { callTool } from "../src/aiservice/tools";
import { FAIL_STREAK, KEY_UNREADABLE, recordCall, tracker, useConfig } from "../src/aiservice/uses";
import type { UsageRun } from "../src/bots/desk/external";
import { paidRuns } from "../src/budget";
import type { Env } from "../src/core/env";
import { resetPriceCache } from "../src/usage/prices";
import { Daemon, HEALTH, downViolations, mention, world } from "./daemon";
import { Client, api, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const realFetch = globalThis.fetch;
const KEY = "sk-deepseek-health-77aa";
const DS = { provider: "deepseek" as const, model: "deepseek-chat", baseUrl: null, apiKey: KEY };
const noSleep = async () => {};

async function clean(): Promise<void> {
  await env.DB.batch(
    ["ai_service", "ai_service_switches", "ai_service_health", "ai_service_alert", "ai_service_calls"].map((t) => env.DB.prepare(`DELETE FROM ${t}`)),
  );
}

async function aiOn(token: string): Promise<void> {
  const r = await api("/v1/settings/ai", { method: "PUT", token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: KEY } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
}

/** A fetch that answers with the given responses in turn and counts its calls. */
function scripted(answers: (Response | Error)[]): { fetch: typeof fetch; calls: () => number } {
  let n = 0;
  return {
    fetch: (async () => {
      const a = answers[Math.min(n++, answers.length - 1)];
      if (a instanceof Error) throw a;
      return a.clone();
    }) as unknown as typeof fetch,
    calls: () => n,
  };
}

const completion = (content: string, usage?: Record<string, unknown>) => Response.json({ choices: [{ message: { role: "assistant", content } }], ...(usage ? { usage } : {}) });

beforeEach(clean);
afterEach(() => {
  vi.restoreAllMocks();
});

describe("switches and status", () => {
  it("tells every member whether the AI 服务 is set up and which uses are on; only admins switch", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    expect(AiServiceStatus.parse((await api("/v1/settings/ai/status", { token: member.token })).body)).toEqual({
      configured: false,
      frontDesk: false,
      titles: false,
      memory: false,
    });
    await aiOn(owner.token);
    const on = await api("/v1/settings/ai/status", { token: member.token });
    expect(on.body).toEqual({ configured: true, frontDesk: true, titles: true, memory: true });
    expect(JSON.stringify(on.body)).not.toContain("deepseek");

    expect((await api("/v1/settings/ai/switches", { method: "PUT", token: member.token, body: { frontDesk: false } })).status).toBe(403);
    expect((await api("/v1/settings/ai/switches", { method: "PUT", token: owner.token, body: {} })).status).toBe(400);
    const off = await api("/v1/settings/ai/switches", { method: "PUT", token: owner.token, body: { frontDesk: false } });
    expect(off.status).toBe(200);
    expect(AiServiceConfig.parse(off.body).switches).toEqual({ frontDesk: false, titles: true, memory: true });
    expect((await api("/v1/settings/ai/status", { token: member.token })).body).toEqual({ configured: true, frontDesk: false, titles: true, memory: true });
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'settings.ai' ORDER BY id DESC LIMIT 1").first<{ details: string }>();
    expect(JSON.parse(audit!.details)).toMatchObject({ switches: { after: { frontDesk: false } } });

    // A switched-off use works as if nothing were set up; the others keep the model.
    expect(await useConfig(env as Env, "front_desk")).toBeNull();
    expect(await useConfig(env as Env, "title")).toMatchObject({ provider: "deepseek", apiKey: KEY });
    expect(await useConfig(env as Env, "tidy")).not.toBeNull();
    await api("/v1/settings/ai/switches", { method: "PUT", token: owner.token, body: { frontDesk: true, memory: false } });
    expect(await useConfig(env as Env, "front_desk")).not.toBeNull();
    expect(await useConfig(env as Env, "retro")).toBeNull();
  });

  it("starts the run when the front desk is switched off", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    await api("/v1/settings/ai/switches", { method: "PUT", token: w.owner.token, body: { frontDesk: false } });
    let deskCalls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
      const req = new Request(input, init);
      if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
      if (((await req.json()) as { tools?: unknown }).tools) deskCalls++;
      return completion("标题");
    });
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w, "@dev 修一下设置页");
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).not.toContain("Work order");
    expect(deskCalls).toBe(0);
    d.close();
    c.close();
  });
});

describe("a key that cannot be read", () => {
  it("answers a mention with a run and records the front desk's failure (模 1.5)", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    // The Worker secret changed since the key was saved: the stored ciphertext no longer opens.
    await env.DB.prepare("UPDATE ai_service SET key_cipher = 'v1.AAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA'").run();
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w, "@dev 看看日志");
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).not.toContain("Work order");
    const cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: w.owner.token })).body);
    expect(cfg.uses?.find((u) => u.use === "front_desk")).toMatchObject({ ok: false, error: KEY_UNREADABLE, failures: 1 });
    // Saving without a new key says what to do instead of a bare 500.
    const kept = await api("/v1/settings/ai", { method: "PUT", token: w.owner.token, body: { provider: "deepseek", model: "deepseek-chat" } });
    expect(kept.status).toBe(400);
    expect(kept.body.error).toMatchObject({ code: "invalid", reason: "ai.key_unreadable" });
    expect(kept.body.error.message).toContain("HOOK_SECRET_KEY");
    d.close();
    c.close();
  });

  it("names the missing Worker secret (模 1.6)", () => {
    expect(() => requireSecret({ ...env, HOOK_SECRET_KEY: undefined } as unknown as Env)).toThrowError(SECRET_MISSING);
    expect(() => requireSecret({ ...env, HOOK_SECRET_KEY: "short" } as unknown as Env)).toThrowError(/HOOK_SECRET_KEY/);
    expect(() => requireSecret(env as Env)).not.toThrow();
  });
});

describe("health and the failing alert", () => {
  it(`raises one alert and one activity entry after ${FAIL_STREAK} failures in a row, and clears both on the next success`, async () => {
    const owner = await createMember({ level: "owner" });
    await aiOn(owner.token);
    const e = env as Env;
    const t0 = Date.now();
    await recordCall(e, "front_desk", "deepseek-chat", { ok: true, usage: { inputTokens: 100, outputTokens: 10 } }, t0);
    for (let i = 1; i < FAIL_STREAK; i++) await recordCall(e, "front_desk", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + i);
    let cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body);
    expect(cfg.failing).toBeUndefined();
    expect(cfg.uses?.[0]).toMatchObject({ use: "front_desk", ok: false, failures: FAIL_STREAK - 1, okAt: t0 });

    await recordCall(e, "front_desk", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + 10);
    await recordCall(e, "tidy", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + 11);
    await recordCall(e, "front_desk", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + 12);
    cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body);
    expect(cfg.failing).toEqual({ since: t0 + 10, use: "front_desk", error: "401 Authentication Fails" });
    let activity = ActivityResponse.parse((await api("/v1/activity?kind=key", { token: owner.token })).body);
    expect(activity.entries.filter((x) => x.action === "ai.failing")).toHaveLength(1);
    expect(activity.entries.find((x) => x.action === "ai.failing")).toMatchObject({ kind: "key", target: "front_desk", details: { error: "401 Authentication Fails" } });

    // A success of another use does not clear it while the front desk still fails.
    await recordCall(e, "title", "deepseek-chat", { ok: true }, t0 + 20);
    expect(AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body).failing).toBeDefined();
    await recordCall(e, "front_desk", "deepseek-chat", { ok: true }, t0 + 30);
    cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body);
    expect(cfg.failing).toBeUndefined();
    expect(cfg.uses?.find((u) => u.use === "front_desk")).toMatchObject({ ok: true, failures: 0, okAt: t0 + 30 });
    activity = ActivityResponse.parse((await api("/v1/activity?kind=key", { token: owner.token })).body);
    expect(activity.entries.filter((x) => x.action === "ai.recovered")).toHaveLength(1);

    // Saving the config again starts a clean record.
    for (let i = 0; i < FAIL_STREAK; i++) await recordCall(e, "summary", "deepseek-chat", { ok: false, error: "500 busy" }, t0 + 40 + i);
    expect(AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body).failing).toBeDefined();
    await aiOn(owner.token);
    cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body);
    expect(cfg.failing).toBeUndefined();
    expect(cfg.uses).toBeUndefined();
  });

  it("notifies the admins once when the alert is raised", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    await aiOn(owner.token);
    const admin = await Client.connect(owner.token, { features: ["settings/1"] });
    const oldAdmin = await Client.connect(owner.token);
    const other = await Client.connect(member.token, { features: ["settings/1"] });
    const e = env as Env;
    const t0 = Date.now();
    for (let i = 0; i < FAIL_STREAK + 2; i++) await recordCall(e, "tidy", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + i);
    const got = await admin.c.next((f) => f.t === "transient" && f.event.type === "ai.failing");
    expect(got.event.failing).toEqual({ since: t0 + FAIL_STREAK - 1, use: "tidy", error: "401 Authentication Fails" });
    await new Promise((r) => setTimeout(r, 200));
    expect(admin.c.frames.filter((f) => f.t === "transient" && f.event?.type === "ai.failing")).toHaveLength(1);
    expect(oldAdmin.c.frames.some((f) => f.t === "transient" && f.event?.type === "ai.failing")).toBe(false);
    expect(other.c.frames.some((f) => f.t === "transient" && f.event?.type === "ai.failing")).toBe(false);
    admin.c.close();
    oldAdmin.c.close();
    other.c.close();
  });

  it("takes the alert down when the failing use is switched off", async () => {
    const owner = await createMember({ level: "owner" });
    await aiOn(owner.token);
    const e = env as Env;
    const t0 = Date.now();
    for (let i = 0; i < FAIL_STREAK; i++) await recordCall(e, "front_desk", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + i);
    for (let i = 0; i < FAIL_STREAK; i++) await recordCall(e, "retro", "deepseek-chat", { ok: false, error: "401 Authentication Fails" }, t0 + 10 + i);
    await recordCall(e, "title", "deepseek-chat", { ok: true }, t0 + 20);
    expect(AiServiceConfig.parse((await api("/v1/settings/ai", { token: owner.token })).body).failing).toBeDefined();

    // The front desk off: the review still fails, so the alert stays.
    let cfg = AiServiceConfig.parse((await api("/v1/settings/ai/switches", { method: "PUT", token: owner.token, body: { frontDesk: false } })).body);
    expect(cfg.failing).toBeDefined();
    expect(cfg.uses?.map((u) => u.use).sort()).toEqual(["retro", "title"]);
    // The memory uses off too: nothing that is on fails any more.
    cfg = AiServiceConfig.parse((await api("/v1/settings/ai/switches", { method: "PUT", token: owner.token, body: { memory: false } })).body);
    expect(cfg.failing).toBeUndefined();
    expect(cfg.uses?.map((u) => u.use)).toEqual(["title"]);
    const activity = ActivityResponse.parse((await api("/v1/activity?kind=key", { token: owner.token })).body);
    const recovered = activity.entries.filter((x) => x.action === "ai.recovered" && x.at >= t0 && x.details?.switchedOff);
    expect(recovered).toHaveLength(1);
    expect(recovered[0]).toMatchObject({ actorId: owner.id, details: { switchedOff: ["memory"] } });
  });

  it("records a front desk that falls back on a provider error", async () => {
    const w = await world();
    await aiOn(w.owner.token);
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
      const req = new Request(input, init);
      if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
      const body = (await req.json()) as { tools?: unknown };
      if (!body.tools) return completion("标题");
      return Response.json({ error: { message: "Insufficient Balance" } }, { status: 402 });
    });
    const { d } = await Daemon.connect(w.machineToken, {});
    const { c } = await Client.connect(w.human.token);
    await mention(c, w, "@dev 修登录页");
    await d.next((f) => f.t === "run.assign");
    const cfg = AiServiceConfig.parse((await api("/v1/settings/ai", { token: w.owner.token })).body);
    expect(cfg.uses?.find((u) => u.use === "front_desk")).toMatchObject({ ok: false, error: "402 Insufficient Balance", failures: 1 });
    d.close();
    c.close();
  });
});

describe("测试连接 (模 1.7)", () => {
  it("tries each check once so the answer arrives inside the clients' 30 s timeout", async () => {
    expect(2 * TEST_CHECK_MS).toBeLessThan(30_000);
    const owner = await createMember({ level: "owner" });
    let calls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
      const req = new Request(input, init);
      if (!/api\.deepseek\.com/.test(req.url)) return realFetch(input as RequestInfo, init);
      calls++;
      return Response.json({ error: { message: "busy" } }, { status: 503 });
    });
    const started = Date.now();
    const r = await api("/v1/settings/ai/test", { method: "POST", token: owner.token, body: { provider: "deepseek", model: "deepseek-chat", apiKey: KEY } });
    expect(r.status).toBe(200);
    expect(r.body).toMatchObject({ ok: false, error: "503 busy", text: { ok: false, error: "503 busy" } });
    expect(calls).toBe(1);
    expect(Date.now() - started).toBeLessThan(5_000);
  });
});

describe("one retry policy", () => {
  it("retries rate limits, server errors and timeouts on the completions path, never a bad key", async () => {
    const busy = scripted([Response.json({ error: { message: "busy" } }, { status: 503 }), completion("OK")]);
    expect(await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: busy.fetch, sleep: noSleep })).toMatchObject({ ok: true, text: "OK" });
    expect(busy.calls()).toBe(2);

    const slept: number[] = [];
    const limited = scripted([new Response("{}", { status: 429, headers: { "retry-after": "2" } }), new Error("The operation was aborted due to timeout"), completion("OK")]);
    const r = await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: limited.fetch, sleep: async (ms) => void slept.push(ms) });
    expect(r.ok).toBe(true);
    expect(slept).toEqual([2000, BACKOFF_MS[1]]);

    const down = scripted([Response.json({ error: { message: "down" } }, { status: 500 })]);
    expect(await callTool(DS, { system: "s", prompt: "p", maxTokens: 8, tool: { name: "t", description: "t", schema: { type: "object" } } }, { fetch: down.fetch, sleep: noSleep })).toEqual({
      ok: false,
      error: "500 down",
    });
    expect(down.calls()).toBe(3);

    const badKey = scripted([Response.json({ error: { message: "Authentication Fails" } }, { status: 401 })]);
    expect(await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: badKey.fetch, sleep: noSleep })).toEqual({ ok: false, error: "401 Authentication Fails" });
    expect(badKey.calls()).toBe(1);
  });

  it("retries the Anthropic path the same way (the SDK's own retries are off)", async () => {
    const message = {
      id: "msg_1",
      type: "message",
      role: "assistant",
      model: "claude-haiku-5-5",
      content: [{ type: "tool_use", id: "tu_1", name: "t", input: { a: 1 } }],
      stop_reason: "tool_use",
      stop_sequence: null,
      usage: { input_tokens: 50, output_tokens: 5, cache_creation_input_tokens: 1200 },
    };
    const overloaded = scripted([
      Response.json({ type: "error", error: { type: "overloaded_error", message: "Overloaded" } }, { status: 529 }),
      Response.json({ type: "error", error: { type: "rate_limit_error", message: "slow down" } }, { status: 429 }),
      Response.json(message),
    ]);
    const cfg = { provider: "anthropic" as const, model: "claude-haiku-5-5", baseUrl: null, apiKey: "sk-ant-test-0000" };
    const tool = { name: "t", description: "t", schema: { type: "object" } };
    const r = await callTool(cfg, { system: "s", prompt: "p", maxTokens: 8, tool }, { fetch: overloaded.fetch, sleep: noSleep });
    expect(r).toEqual({ ok: true, input: { a: 1 }, usage: { inputTokens: 50, outputTokens: 5, cacheWriteTokens: 1200 } });
    expect(overloaded.calls()).toBe(3);

    const denied = scripted([Response.json({ type: "error", error: { type: "authentication_error", message: "invalid x-api-key" } }, { status: 401 })]);
    expect((await callTool(cfg, { system: "s", prompt: "p", maxTokens: 8, tool }, { fetch: denied.fetch, sleep: noSleep })).ok).toBe(false);
    expect(denied.calls()).toBe(1);
  });

  it("knows what to retry and reads retry-after", () => {
    expect([undefined, 408, 429, 500, 529].every(retryable)).toBe(true);
    expect([400, 401, 402, 403, 404, 422].some(retryable)).toBe(false);
    expect(retryAfterMs("3")).toBe(3000);
    expect(retryAfterMs("600")).toBe(10_000);
    expect(retryAfterMs(new Date(Date.now() + 1_000).toUTCString(), Date.now())).toBeLessThanOrEqual(1_000);
    expect(retryAfterMs(null)).toBeUndefined();
  });
});

describe("tokens on 用量", () => {
  it("counts cached input apart, records each call by use, and shows one line", async () => {
    expect(completionUsage({ prompt_tokens: 1000, completion_tokens: 50, prompt_cache_hit_tokens: 800 })).toEqual({ inputTokens: 200, outputTokens: 50, cacheReadTokens: 800 });
    expect(completionUsage({ prompt_tokens: 100, completion_tokens: 5, prompt_tokens_details: { cached_tokens: 40 } })).toEqual({ inputTokens: 60, outputTokens: 5, cacheReadTokens: 40 });

    resetPriceCache();
    const owner = await createMember({ level: "owner" });
    await aiOn(owner.token);
    const e = env as Env;
    const ok = scripted([completion("标题", { prompt_tokens: 1_000_000, completion_tokens: 1000, prompt_cache_hit_tokens: 0 })]);
    await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: ok.fetch, track: tracker(e, "title", DS) });
    await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: ok.fetch, track: tracker(e, "title", DS) });
    const bad = scripted([Response.json({ error: { message: "nope" } }, { status: 401 })]);
    await callModel(DS, { system: "s", prompt: "p", maxTokens: 8 }, { fetch: bad.fetch, track: tracker(e, "front_desk", DS) });
    await recordCall(e, "front_desk", "mystery-model-1", { ok: true, usage: { inputTokens: 10, outputTokens: 2 } });

    const u = TeamUsage.parse((await api(`/v1/usage?from=${Date.now() - 86_400_000}&to=${Date.now() + 1}`, { token: owner.token })).body);
    expect(u.service).toMatchObject({ calls: 4, failures: 1, inputTokens: 2_000_010, outputTokens: 2002 });
    // An unknown model's tokens are left out of the cost, never priced as another model.
    expect(u.service?.unpricedTokens).toBeGreaterThanOrEqual(12);
    expect(u.service?.uses.map((x) => [x.use, x.calls])).toEqual([
      ["title", 2],
      ["front_desk", 2],
    ]);
    // Not in the runs' total.
    expect(u.total.inputTokens).toBe(0);
  });
});

describe("budget and subscriptions (模 4.1)", () => {
  const run = (machineId: string | null, runtime: string | null): UsageRun => ({
    endedAt: 1,
    requestedBy: "h",
    machineId,
    runtime,
    topicId: "t",
    costUsd: 1,
    inputTokens: 0,
    outputTokens: 0,
  });

  it("leaves out runs whose machine's CLI is signed in with a subscription", async () => {
    const methods: Record<string, Record<string, string>> = { m1: { claude: "subscription", codex: "api_key" }, m2: {} };
    const runs = [run("m1", "claude"), run("m1", "codex"), run("m2", "claude"), run(null, "claude")];
    const paid = await paidRuns(runs, async (id) => methods[id] ?? {});
    expect(paid).toEqual([runs[1], runs[2], runs[3]]);
  });

  it("goes by the sign-in a run was sent with, not the machine's sign-in now", async () => {
    const now: Record<string, Record<string, string>> = { m1: { claude: "api_key" }, m2: { claude: "subscription" } };
    const asked: string[] = [];
    const runs = [
      { ...run("m1", "claude"), loginMethod: "subscription" },
      { ...run("m2", "claude"), loginMethod: "api_key" },
      run("m2", "claude"),
    ];
    const paid = await paidRuns(runs, async (id) => (asked.push(id), now[id] ?? {}));
    // The first ran on a subscription although m1 is on a key now; the second ran on a key.
    expect(paid).toEqual([runs[1]]);
    // Only the old run without a kept sign-in asks the machine.
    expect(asked).toEqual(["m2"]);
  });

  it("starts a run past the budget when the AI member runs on a subscription", async () => {
    resetPriceCache();
    const w = await world();
    await api(`/v1/members/${w.bot.id}`, { method: "PATCH", token: w.owner.token, body: { weeklyBudgetCents: 100 } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    // The machine's Codex is signed in with a subscription.
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: {
        at: Date.now(),
        uptimeSecs: 100,
        cpuPct: 1,
        cpuThreads: 8,
        memTotalBytes: 1e10,
        memAvailableBytes: 5e9,
        netRxBps: 0,
        netTxBps: 0,
        vmSlots: 0,
        runs: [],
        logins: [{ runtime: "codex", state: "logged_in", method: "subscription" }],
      },
    });
    await d.next((f) => f.t === "heartbeat.ack");
    await mention(c, w, "@dev first job");
    const first = await d.next((f) => f.t === "run.assign");
    d.send({ t: "run.accept", runId: first.runId });
    const done = d.done(first.runId, {
      status: "succeeded",
      resultText: "ok",
      usage: { inputTokens: 1_000_000, outputTokens: 0, models: [{ model: "gpt-5-codex", inputTokens: 1_000_000, outputTokens: 0 }] },
    });
    await d.acked(first.runId, done);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.endedAt);
    // The operator moves Codex to an API key afterwards: the first run still ran on the subscription.
    d.send({
      t: "heartbeat",
      at: Date.now(),
      health: HEALTH,
      runs: [],
      metrics: {
        at: Date.now(),
        uptimeSecs: 101,
        cpuPct: 1,
        cpuThreads: 8,
        memTotalBytes: 1e10,
        memAvailableBytes: 5e9,
        netRxBps: 0,
        netTxBps: 0,
        vmSlots: 0,
        runs: [],
        logins: [{ runtime: "codex", state: "logged_in", method: "api_key" }],
      },
    });
    await waitUntil(() => d.frames.filter((f) => f.t === "heartbeat.ack").length >= 2, 5000, "second heartbeat");
    // $1.25 at list price is over the $1 budget, but the subscription paid for it: the next run starts.
    await mention(c, w, "@dev second job");
    await waitUntil(() => d.frames.filter((f) => f.t === "run.assign").length === 2, 5000, "second run");
    d.close();
    c.close();
  });
});
