// One monitor run (every 5 minutes): check every target, update alert state, post pending
// alerts through the first healthy target, and post the daily summary once a day.
import type { CheckResult } from "./alerts";
import { type Config, type Env, type Target, readConfig, tokenFor } from "./config";
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
}

async function postVia(state: DurableObjectStub<MonitorState>, deps: Deps, sink: Sink, id: string, post: Post): Promise<boolean> {
  const key = `alertTopic:${sink.target.name}`;
  let topicId = await state.get(key);
  let out = await sendPost(deps.fetcher, sink.target.baseUrl, sink.token, sink.groupId, topicId, id, post);
  if (!out.ok && out.topicMissing) {
    await state.set(key, null);
    topicId = null;
    out = await sendPost(deps.fetcher, sink.target.baseUrl, sink.token, sink.groupId, null, id, post);
  }
  if (out.ok && out.topicId && out.topicId !== topicId) await state.set(key, out.topicId);
  if (!out.ok) deps.log("post failed", { sink: sink.target.name, status: out.status });
  return out.ok;
}

export async function runMonitor(env: Env, partial: Partial<Deps> = {}): Promise<RunReport> {
  const deps = { ...defaultDeps(), ...partial };
  const cfg: Config = readConfig(env);
  const state = stateStub(env);

  const probes: ProbeResult[] = await Promise.all(
    cfg.targets.map(async (t) => {
      const key = `rtTopic:${t.name}`;
      const topicId = await state.get(key);
      const r = await deps.probe(t, tokenFor(env, t), topicId, { fetcher: deps.fetcher, now: deps.now });
      if (r.topicId !== topicId) await state.set(key, r.topicId);
      return r;
    }),
  );
  const results: CheckResult[] = probes.map(({ target, at, ok, healthMs, rttMs, error }) => ({
    target,
    at,
    ok,
    healthMs,
    rttMs,
    error,
  }));
  const raised = await state.record(results, cfg.failThreshold);
  for (const r of results) {
    deps.log("check", { target: r.target, ok: r.ok, healthMs: r.healthMs, rttMs: r.rttMs, error: r.error });
  }

  // Healthy targets in config order are the alert sinks.
  const sinks: Sink[] = [];
  for (const p of probes) {
    const target = cfg.targets.find((t) => t.name === p.target) as Target;
    const token = tokenFor(env, target);
    if (p.ok && p.opsGroupId && token) sinks.push({ target, token, groupId: p.opsGroupId });
  }

  let posted = 0;
  const pending = await state.pending();
  for (const p of pending) {
    let sent = false;
    for (const sink of sinks) {
      if (await postVia(state, deps, sink, p.id, alertPost(p.alert))) {
        sent = true;
        break;
      }
    }
    if (sent) {
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
