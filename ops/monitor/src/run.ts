// One monitor run (every 5 minutes): check every target, update alert state, post pending
// alerts through the first healthy target, and post the daily summary once a day.
import type { Alert, CheckResult } from "./alerts";
import { type Config, type Env, type Target, fetcherFor, readConfig, tokenFor } from "./config";
import { type ProbeResult, probe } from "./probe";
import { type Post, alertPost, sendPost, summaryPost } from "./post";
import type { MonitorState } from "./state";

const DAY = 24 * 60 * 60 * 1000;

export interface Deps {
  fetcher: typeof fetch;
  now: () => number;
  probe: typeof probe;
  log: (msg: string, fields?: Record<string, unknown>) => void;
}

export interface RunReport {
  results: CheckResult[];
  raised: number;
  posted: number;
  pending: number;
  summary: boolean;
}

export function stateStub(env: Env): DurableObjectStub<MonitorState> {
  return env.MONITOR_STATE.get(env.MONITOR_STATE.idFromName("monitor"));
}

const defaultDeps = (): Deps => ({
  fetcher: (input, init) => fetch(input, init),
  now: Date.now,
  probe,
  log: (msg, fields) => console.log(JSON.stringify({ level: "info", svc: "chat-monitor", msg, ...fields })),
});

interface Sink {
  target: Target;
  token: string;
  groupId: string;
  fetcher: typeof fetch;
  /** Members to mention on alerts posted through this sink. */
  oncall: string[];
}

async function postVia(state: DurableObjectStub<MonitorState>, deps: Deps, sink: Sink, id: string, post: Post): Promise<boolean> {
  const key = `alertTopic:${sink.target.name}`;
  let topicId = await state.get(key);
  let out = await sendPost(sink.fetcher, sink.target.baseUrl, sink.token, sink.groupId, topicId, id, post);
  if (!out.ok && out.topicMissing) {
    await state.set(key, null);
    topicId = null;
    out = await sendPost(sink.fetcher, sink.target.baseUrl, sink.token, sink.groupId, null, id, post);
  }
  if (!out.ok && out.status === 400 && post.mentions) {
    // An on-call member is not in this env's ops group: post without mentions rather than not at all.
    deps.log("post refused with mentions; posting without", { sink: sink.target.name });
    out = await sendPost(sink.fetcher, sink.target.baseUrl, sink.token, sink.groupId, topicId, id, { ...post, mentions: undefined });
  }
  if (out.ok && out.topicId && out.topicId !== topicId) await state.set(key, out.topicId);
  if (!out.ok) deps.log("post failed", { sink: sink.target.name, status: out.status });
  return out.ok;
}

interface Collected {
  probes: ProbeResult[];
  /** Healthy targets in config order: the alert sinks. */
  sinks: Sink[];
}

/** Probes every target and returns the healthy ones as alert sinks, in config order. */
async function collect(env: Env, deps: Deps, cfg: Config, state: DurableObjectStub<MonitorState>): Promise<Collected> {
  const fetchers = new Map<string, typeof fetch | null>();
  for (const t of cfg.targets) {
    try {
      fetchers.set(t.name, fetcherFor(env, t, deps.fetcher));
    } catch {
      fetchers.set(t.name, null);
    }
  }
  const probes: ProbeResult[] = await Promise.all(
    cfg.targets.map(async (t) => {
      const key = `rtTopic:${t.name}`;
      const topicId = await state.get(key);
      const fetcher = fetchers.get(t.name);
      if (!fetcher) {
        const at = deps.now();
        const error = `config: service binding ${t.service} is missing`;
        return { target: t.name, at, ok: false, healthMs: null, rttMs: null, error, opsGroupId: null, topicId };
      }
      const r = await deps.probe(t, tokenFor(env, t), topicId, { fetcher, now: deps.now });
      if (r.topicId !== topicId) await state.set(key, r.topicId);
      return r;
    }),
  );
  const sinks: Sink[] = [];
  for (const p of probes) {
    const target = cfg.targets.find((t) => t.name === p.target) as Target;
    const token = tokenFor(env, target);
    const fetcher = fetchers.get(target.name);
    if (p.ok && p.opsGroupId && token && fetcher) {
      sinks.push({ target, token, groupId: p.opsGroupId, fetcher, oncall: target.oncall ?? p.oncall ?? [] });
    }
  }
  return { probes, sinks };
}

/** Posts one alert through the first sink that takes it. The only alert delivery path. */
async function deliver(state: DurableObjectStub<MonitorState>, deps: Deps, sinks: Sink[], id: string, alert: Alert): Promise<Sink | null> {
  for (const sink of sinks) {
    // Member ids differ per env: mention the people on call in the sink env's ops group.
    if (await postVia(state, deps, sink, id, alertPost(alert, sink.oncall))) return sink;
  }
  return null;
}

export async function runMonitor(env: Env, partial: Partial<Deps> = {}): Promise<RunReport> {
  const deps = { ...defaultDeps(), ...partial };
  const cfg: Config = readConfig(env);
  const state = stateStub(env);

  const { probes, sinks } = await collect(env, deps, cfg, state);
  const results: CheckResult[] = probes.map(({ target, at, ok, healthMs, rttMs, error }) => ({
    target,
    at,
    ok,
    healthMs,
    rttMs,
    error,
  }));
  // Archive freshness has its own alert state; it is checked only while the service is up.
  for (const p of probes) if (p.backup) results.push(p.backup);
  const raised = await state.record(results, cfg.failThreshold);
  for (const r of results) {
    deps.log("check", { target: r.target, check: r.check ?? "service", ok: r.ok, healthMs: r.healthMs, rttMs: r.rttMs, error: r.error });
  }

  let posted = 0;
  const pending = await state.pending();
  for (const p of pending) {
    if (await deliver(state, deps, sinks, p.id, p.alert)) {
      await state.markSent(p.id);
      posted++;
    } else {
      await state.markFailed(p.id);
    }
  }

  let summary = false;
  const now = deps.now();
  const date = new Date(now).toISOString().slice(0, 10);
  if (new Date(now).getUTCHours() >= cfg.summaryHourUtc && (await state.get("summaryDate")) !== date && sinks.length > 0) {
    const rows = await state.summary(
      cfg.targets.map((t) => t.name),
      now - DAY,
      now + 1,
    );
    for (const sink of sinks) {
      if (await postVia(state, deps, sink, `summary-${date}`, summaryPost(date, rows))) {
        await state.set("summaryDate", date);
        summary = true;
        break;
      }
    }
  }
  const left = pending.length - posted;
  deps.log("monitor run", { targets: results.length, raised: raised.length, posted, pending: left, summary });
  return { results, raised: raised.length, posted, pending: left, summary };
}

/** Shortest gap between two manual test alerts. */
export const TEST_ALERT_MIN_GAP_MS = 60_000;

export type TestAlertOutcome =
  | { ok: true; sink: string; target: string }
  | { ok: false; reason: "unknown_target" | "too_soon" | "no_sink" | "post_failed"; detail?: string };

/**
 * A manual test alert: probes the targets like a cron run (without recording the results, so
 * alert state does not change), builds a 不可用 alert marked as a test and delivers it through
 * the same sinks, card builder and post path as a real alert. `sink` picks the env whose ops
 * group gets it (default: the first healthy target, as for a real alert).
 */
export async function sendTestAlert(env: Env, opts: { sink?: string } = {}, partial: Partial<Deps> = {}): Promise<TestAlertOutcome> {
  const deps = { ...defaultDeps(), ...partial };
  const cfg: Config = readConfig(env);
  if (opts.sink && !cfg.targets.some((t) => t.name === opts.sink)) return { ok: false, reason: "unknown_target" };
  const state = stateStub(env);
  const now = deps.now();
  if (!(await state.claim("testAlertAt", now, TEST_ALERT_MIN_GAP_MS))) return { ok: false, reason: "too_soon" };

  const { probes, sinks } = await collect(env, deps, cfg, state);
  const chosen = opts.sink ? sinks.filter((s) => s.target.name === opts.sink) : sinks;
  if (chosen.length === 0) {
    const p = probes.find((x) => x.target === opts.sink);
    return { ok: false, reason: "no_sink", detail: p?.error ?? undefined };
  }
  const about = chosen[0].target.name;
  const alert: Alert = { kind: "down", test: true, target: about, at: now, since: now, fails: cfg.failThreshold, error: "手动测试" };
  const sink = await deliver(state, deps, chosen, `test-${crypto.randomUUID()}`, alert);
  deps.log("test alert", { sink: sink?.target.name ?? null, ok: !!sink });
  return sink ? { ok: true, sink: sink.target.name, target: about } : { ok: false, reason: "post_failed" };
}
