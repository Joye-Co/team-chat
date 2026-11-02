import { env } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { type CheckResult, INITIAL, step } from "../src/alerts";
import type { Target } from "../src/config";
import type { ProbeResult } from "../src/probe";
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
