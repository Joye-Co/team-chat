import { env } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { type CheckResult, INITIAL, step } from "../src/alerts";
import type { Target } from "../src/config";
import { BACKUP_MAX_AGE_MS, type ProbeResult, backupCheck, probe } from "../src/probe";
import { handleFetch } from "../src/index";
import { runMonitor } from "../src/run";

const T0 = Date.UTC(2026, 10, 3, 0, 0, 0);
const MIN = 60_000;

function check(target: string, at: number, ok: boolean, error = "health: 503"): CheckResult {
  return { target, at, ok, healthMs: ok ? 20 : null, rttMs: ok ? 80 : null, error: ok ? null : error };
}

describe("alert state machine", () => {
  it("raises one down alert after the threshold and one recovery alert", () => {
    let s = { ...INITIAL };
    const alerts: string[] = [];
    const seq = [true, false, false, false, false, true, true];
    seq.forEach((ok, i) => {
      const r = step(s, check("p", T0 + i * 5 * MIN, ok), 2);
      s = r.next;
      if (r.alert) alerts.push(`${r.alert.kind}@${i}`);
    });
    expect(alerts).toEqual(["down@2", "up@5"]);
  });

  it("stays quiet when failures do not reach the threshold", () => {
    let s = { ...INITIAL };
    for (const [i, ok] of [false, true, false, true, false, true].entries()) {
      const r = step(s, check("p", T0 + i * MIN, ok), 2);
      expect(r.alert).toBeNull();
      s = r.next;
    }
  });

  it("keeps the start time of an outage for the recovery alert", () => {
    let s = step(INITIAL, check("p", T0, false), 1).next;
    s = step(s, check("p", T0 + 5 * MIN, false), 1).next;
    const up = step(s, check("p", T0 + 30 * MIN, true), 1).alert;
    expect(up).toMatchObject({ kind: "up", downSince: T0, error: "health: 503" });
  });
});

describe("MonitorState", () => {
  it("persists state, queues alerts until sent, and summarizes checks", async () => {
    const s = env.MONITOR_STATE.get(env.MONITOR_STATE.idFromName("unit"));
    expect(await s.record([check("p", T0, false)], 2)).toEqual([]);
    const raised = await s.record([check("p", T0 + 5 * MIN, false)], 2);
    expect(raised.map((a) => a.alert.kind)).toEqual(["down"]);
    expect(await s.record([check("p", T0 + 10 * MIN, false)], 2)).toEqual([]);
    expect((await s.pending()).map((p) => p.id)).toEqual([raised[0].id]);
    await s.markFailed(raised[0].id);
    expect((await s.pending())[0].attempts).toBe(1);
    await s.markSent(raised[0].id);
    expect(await s.pending()).toEqual([]);
    const up = await s.record([check("p", T0 + 15 * MIN, true)], 2);
    expect(up.map((a) => a.alert.kind)).toEqual(["up"]);
    expect(await s.targetState("p")).toMatchObject({ status: "ok", fails: 0, alerted: false });
    const [sum] = await s.summary(["p"], T0, T0 + DAY);
    expect(sum).toMatchObject({ target: "p", checks: 4, failures: 3, uptime: 0.25, p50: 80, p95: 80 });
  });
});

const DAY = 24 * 60 * 60 * 1000;

/** Fake environment: which targets are up; records every HTTP post the monitor makes. */
function harness() {
  const up: Record<string, boolean> = { production: true, staging: true };
  const posts: { url: string; auth: string; body: any }[] = [];
  let clock = T0 + 2 * 60 * MIN; // 02:00 UTC
  const fakeProbe = async (t: Target, token: string | null, topicId: string | null): Promise<ProbeResult> => {
    expect(token).toMatch(/^tc_test_/);
    const ok = up[t.name];
    return {
      target: t.name,
      at: clock,
      ok,
      healthMs: ok ? 10 : null,
      rttMs: ok ? 50 : null,
      error: ok ? null : "round trip: timeout",
      opsGroupId: ok ? `ops-${t.name}` : null,
      topicId: topicId ?? `rt-${t.name}`,
    };
  };
  const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const body = JSON.parse(String(init?.body));
    posts.push({ url, auth: new Headers(init?.headers).get("authorization") ?? "", body });
    const topicId = body.topicId ?? `alerts-${new URL(url).host}`;
    return Response.json({ message: { id: crypto.randomUUID(), topicId } });
  }) as typeof fetch;
  const tick = (min = 5) => {
    clock += min * MIN;
  };
  const run = () => runMonitor(env, { probe: fakeProbe, fetcher, now: () => clock, log: () => {} });
  return { up, posts, tick, run };
}

describe("monitor run", () => {
  it("alerts through a healthy env, once per outage, and sends one daily summary", async () => {
    const h = harness();
    // All healthy: the daily summary goes out once.
    let r = await h.run();
    expect(r.summary).toBe(true);
    expect(h.posts).toHaveLength(1);
    expect(h.posts[0].url).toBe("https://prod.test/v1/groups/ops-production/messages");
    expect(h.posts[0].body.card.title).toContain("每日运行摘要");
    h.posts.length = 0;
    h.tick();
    expect((await h.run()).summary).toBe(false);

    // Production goes down: alert on the second failed check, posted through staging.
    h.up.production = false;
    h.tick();
    r = await h.run();
    expect(r.raised).toBe(0);
    h.tick();
    r = await h.run();
    expect(r).toMatchObject({ raised: 1, posted: 1 });
    expect(h.posts).toHaveLength(1);
    expect(h.posts[0].url).toBe("https://staging.test/v1/groups/ops-staging/messages");
    expect(h.posts[0].auth).toBe("Bearer tc_test_staging_token_value_0000000000");
    expect(h.posts[0].body).toMatchObject({ card: { tone: "danger" }, title: "监测告警" });
    expect(h.posts[0].body.body).toContain("production");
    // Still down: no more alerts.
    for (let i = 0; i < 5; i++) {
      h.tick();
      await h.run();
    }
    expect(h.posts).toHaveLength(1);

    // Everything down: the staging alert waits until some env is healthy again.
    h.up.staging = false;
    h.tick();
    await h.run();
    h.tick();
    r = await h.run();
    expect(r).toMatchObject({ raised: 1, posted: 0, pending: 1 });
    expect(h.posts).toHaveLength(1);

    // Production comes back: it carries the queued staging alert and its own recovery.
    h.up.production = true;
    h.tick();
    r = await h.run();
    expect(r).toMatchObject({ raised: 1, posted: 2, pending: 0 });
    const recent = h.posts.slice(1);
    expect(recent.map((p) => p.url)).toEqual([
      "https://prod.test/v1/groups/ops-production/messages",
      "https://prod.test/v1/groups/ops-production/messages",
    ]);
    expect(recent[0].body.body).toContain("【告警】staging");
    expect(recent[1].body.body).toContain("【恢复】production");
    // The alert topic is reused after the first post.
    expect(recent[1].body.topicId).toBe("alerts-prod.test");

    // Next day: one more summary.
    h.tick(24 * 60);
    h.up.staging = true;
    r = await h.run();
    expect(r.summary).toBe(true);
  });
});

/** A fake API: health, a WebSocket that answers hello/send like the server, and the archive status. */
function fakeApi(opts: { archivedAt: number | null; probeGroup?: boolean } = { archivedAt: null }) {
  const sent: any[] = [];
  const requests: string[] = [];
  const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input));
    requests.push(url.pathname);
    if (url.pathname === "/v1/health") return Response.json({ ok: true, version: "1" });
    if (url.pathname === "/v1/ops/archive") return Response.json({ lastCompleteAt: opts.archivedAt, lastRun: null });
    if (url.pathname === "/v1/ws") {
      expect(new Headers(init?.headers).get("authorization")).toMatch(/^Bearer tc_test_/);
      const pair = new WebSocketPair();
      const server = pair[1];
      server.accept();
      server.addEventListener("message", (e) => {
        const f = JSON.parse(String(e.data));
        if (f.t === "hello") {
          const groups = [{ group: { id: "g-ops", kind: "group", name: "ops", memberIds: ["bot", "alice", "bob", "ci"] } }];
          if (opts.probeGroup !== false) groups.push({ group: { id: "g-probe", kind: "group", name: "监测", memberIds: ["bot"] } });
          const members = [
            { id: "bot", kind: "bot" },
            { id: "alice", kind: "human" },
            { id: "bob", kind: "human", deactivated: true },
            { id: "ci", kind: "bot" },
          ];
          server.send(JSON.stringify({ t: "welcome", me: { id: "bot" }, members, groups }));
        } else if (f.t === "send") {
          sent.push(f);
          const message = { id: crypto.randomUUID(), clientMsgId: f.clientMsgId, topicId: "rt" };
          server.send(JSON.stringify({ rid: f.rid, ok: true, result: { message, topic: { id: "rt" } } }));
          server.send(JSON.stringify({ t: "event", event: { type: "message.created", data: { message } } }));
        }
      });
      return new Response(null, { status: 101, webSocket: pair[0] });
    }
    return new Response("not found", { status: 404 });
  }) as typeof fetch;
  return { fetcher, sent, requests };
}

const PROD: Target = { name: "production", baseUrl: "https://prod.test", opsGroup: "ops", probeGroup: "监测", tokenSecret: "MONITOR_TOKEN_PRODUCTION" };

describe("probe", () => {
  it("round-trips in the probe group, names the people of the ops group and checks the archive", async () => {
    const api = fakeApi({ archivedAt: T0 - 60 * MIN });
    const r = await probe(PROD, "tc_test_production_token_value_000000", null, { fetcher: api.fetcher, now: () => T0 });
    expect(r).toMatchObject({ ok: true, opsGroupId: "g-ops", topicId: "rt", oncall: ["alice"] });
    expect(api.sent.map((f) => f.groupId)).toEqual(["g-probe"]);
    expect(r.backup).toMatchObject({ check: "backup", ok: true });

    const missing = fakeApi({ archivedAt: T0, probeGroup: false });
    const m = await probe(PROD, "tc_test_production_token_value_000000", null, { fetcher: missing.fetcher, now: () => T0 });
    expect(m).toMatchObject({ ok: false, error: "round trip: monitor bot is not in the probe group" });
  });

  it("fails the archive check when the newest complete pass is too old or missing", async () => {
    const token = "tc_test_production_token_value_000000";
    const stale = await backupCheck(fakeApi({ archivedAt: T0 - BACKUP_MAX_AGE_MS - MIN }).fetcher, PROD, token, () => T0);
    expect(stale).toMatchObject({ ok: false, error: "backup: last complete archive 26 h ago" });
    const never = await backupCheck(fakeApi({ archivedAt: null }).fetcher, PROD, token, () => T0);
    expect(never).toMatchObject({ ok: false, error: "backup: no complete archive yet" });
  });
});

describe("monitor targets", () => {
  it("reaches deployed targets through their service bindings", async () => {
    const targets = [
      { ...PROD, service: "API_PRODUCTION" },
      { name: "staging", baseUrl: "https://staging.test", opsGroup: "ops", tokenSecret: "MONITOR_TOKEN_STAGING", service: "API_NOPE" },
    ];
    const seen: string[] = [];
    const viaProbe = async (t: Target, _token: string | null, topicId: string | null, o: any): Promise<ProbeResult> => {
      const res = await o.fetcher(`${t.baseUrl}/v1/health`);
      seen.push(`${t.name}:${(await res.json()).via}`);
      return { target: t.name, at: T0, ok: false, healthMs: null, rttMs: null, error: "health: 500", opsGroupId: null, topicId };
    };
    const global = (async () => {
      throw new Error("global fetch must not be used for a target with a service binding");
    }) as typeof fetch;
    const r = await runMonitor({ ...env, TARGETS: JSON.stringify(targets) } as any, {
      probe: viaProbe,
      fetcher: global,
      now: () => T0,
      log: () => {},
    });
    expect(seen).toEqual(["production:API_PRODUCTION"]);
    expect(r.results.find((x) => x.target === "staging")?.error).toBe("config: service binding API_NOPE is missing");
  });
});

describe("alerts reach people", () => {
  it("mentions the people on call, and falls back to no mentions when the group refuses them", async () => {
    const posts: any[] = [];
    let refuse = false;
    let up = true;
    let backupOk = true;
    let clock = T0 + 2 * 60 * MIN;
    const fakeProbe = async (t: Target, _token: string | null, topicId: string | null): Promise<ProbeResult> => {
      const ok = t.name === "staging" || up;
      return {
        target: t.name,
        at: clock,
        ok,
        healthMs: ok ? 10 : null,
        rttMs: ok ? 50 : null,
        error: ok ? null : "health: 503",
        opsGroupId: ok ? `ops-${t.name}` : null,
        topicId,
        oncall: [`oncall-${t.name}`],
        backup: ok && t.name === "production"
          ? { target: t.name, check: "backup", at: clock, ok: backupOk, healthMs: null, rttMs: null, error: backupOk ? null : "backup: no complete archive yet" }
          : undefined,
      };
    };
    const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body));
      posts.push({ url: String(input), body });
      if (refuse && body.mentions) return Response.json({ error: { code: "invalid", message: "mention of a member outside this group" } }, { status: 400 });
      return Response.json({ message: { id: crypto.randomUUID(), topicId: body.topicId ?? "alerts" } });
    }) as typeof fetch;
    const run = () => runMonitor(env, { probe: fakeProbe, fetcher, now: () => clock, log: () => {} });
    const tick = () => {
      clock += 5 * MIN;
    };
    await run(); // summary
    posts.length = 0;

    up = false;
    tick();
    await run();
    tick();
    await run();
    const down = posts.find((p) => p.body.body.includes("【告警】production"));
    expect(down.url).toBe("https://staging.test/v1/groups/ops-staging/messages");
    // Staging ids: the alert is posted in staging's ops group.
    expect(down.body.mentions).toEqual(["oncall-staging"]);

    refuse = true;
    up = true;
    tick();
    await run();
    const recovered = posts.filter((p) => p.body.body.includes("【恢复】production"));
    expect(recovered.map((p) => p.body.mentions)).toEqual([["oncall-production"], undefined]);

    // The archive check has its own alert: 备份过期, then 备份已恢复.
    refuse = false;
    posts.length = 0;
    backupOk = false;
    tick();
    await run();
    tick();
    await run();
    expect(posts.map((p) => p.body.card.title)).toEqual(["production 备份过期"]);
    expect(posts[0].body.mentions).toEqual(["oncall-production"]);
    backupOk = true;
    tick();
    await run();
    expect(posts.map((p) => p.body.card.title)).toEqual(["production 备份过期", "production 备份已恢复"]);
  });
});

describe("manual test alert", () => {
  const SECRET = "test-admin-secret-0123456789abcdef";
  const withSecret = { ...env, MONITOR_ADMIN_SECRET: SECRET } as any;
  let clock = T0 + 30 * DAY;

  function deps(up: Record<string, boolean>, posts: { url: string; body: any }[]) {
    const fakeProbe = async (t: Target, _token: string | null, topicId: string | null): Promise<ProbeResult> => {
      const ok = up[t.name];
      return {
        target: t.name,
        at: clock,
        ok,
        healthMs: ok ? 10 : null,
        rttMs: ok ? 50 : null,
        error: ok ? null : "health: 503",
        opsGroupId: ok ? `ops-${t.name}` : null,
        topicId,
        oncall: [`oncall-${t.name}`],
      };
    };
    const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body));
      posts.push({ url: String(input), body });
      return Response.json({ message: { id: crypto.randomUUID(), topicId: body.topicId ?? "alerts" } });
    }) as typeof fetch;
    return { probe: fakeProbe, fetcher, now: () => clock, log: () => {} };
  }
  const req = (auth: string | null, body?: unknown, method = "POST") =>
    new Request("https://monitor.test/test-alert", {
      method,
      headers: auth ? { authorization: auth } : {},
      body: body === undefined ? undefined : JSON.stringify(body),
    });

  it("is off without the secret and refuses a wrong or missing one", async () => {
    const posts: any[] = [];
    const d = deps({ production: true, staging: true }, posts);
    expect((await handleFetch(req(`Bearer ${SECRET}`), env as any, d)).status).toBe(404);
    expect((await handleFetch(req(null), withSecret, d)).status).toBe(401);
    expect((await handleFetch(req("Bearer wrong-secret-wrong-secret-00"), withSecret, d)).status).toBe(401);
    expect((await handleFetch(req(`Bearer ${SECRET}`, undefined, "GET"), withSecret, d)).status).toBe(405);
    expect((await handleFetch(req(`Bearer ${SECRET}`, { sink: 5 }), withSecret, d)).status).toBe(400);
    expect((await handleFetch(req(`Bearer ${SECRET}`, { sink: "nope" }), withSecret, d)).status).toBe(400);
    expect((await handleFetch(new Request("https://monitor.test/"), withSecret, d)).status).toBe(404);
    expect(posts).toEqual([]);
  });

  it("posts one test-worded alert through the real alert path to the chosen env, without changing alert state", async () => {
    const posts: { url: string; body: any }[] = [];
    const d = deps({ production: true, staging: true }, posts);
    const s = env.MONITOR_STATE.get(env.MONITOR_STATE.idFromName("monitor"));
    const before = await s.pending();
    const res = await handleFetch(req(`Bearer ${SECRET}`, { sink: "staging" }), withSecret, d);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, sink: "staging", target: "staging" });
    expect(posts).toHaveLength(1);
    const p = posts[0];
    expect(p.url).toBe("https://staging.test/v1/groups/ops-staging/messages");
    expect(p.body).toMatchObject({ mentions: ["oncall-staging"], card: { tone: "info", status: { label: "测试" } } });
    expect(p.body.body).toMatch(/^【测试】/);
    expect(p.body.body).toContain("不需要处理");
    expect(p.body.card.title).toBe("【测试】staging 告警测试");
    expect(p.body.clientMsgId).toMatch(/^test-/);
    expect(await s.pending()).toEqual(before);

    // A second request within a minute is refused; after it, the default sink is the first healthy target.
    expect((await handleFetch(req(`Bearer ${SECRET}`), withSecret, d)).status).toBe(429);
    clock += 2 * MIN;
    const again = await handleFetch(req(`Bearer ${SECRET}`), withSecret, d);
    expect(await again.json()).toMatchObject({ ok: true, sink: "production" });
    expect(posts[1].url).toBe("https://prod.test/v1/groups/ops-production/messages");
  });

  it("reports when the chosen env cannot take the alert", async () => {
    clock += 2 * MIN;
    const posts: any[] = [];
    const res = await handleFetch(req(`Bearer ${SECRET}`, { sink: "staging" }), withSecret, deps({ production: true, staging: false }, posts));
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ error: { code: "no_sink", message: "health: 503" } });
    expect(posts).toEqual([]);
  });
});
