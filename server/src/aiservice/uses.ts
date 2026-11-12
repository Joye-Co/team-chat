// What the team's model does and how it is doing (usable 模 1.1, 1.3, 1.4, 1.9; 盯 4.6, 5.6):
// - per-use switches (前台 / titles and summaries / memory), each on unless an admin turned it off;
// - the config for one use, which never throws: a key that cannot be read (the Worker secret
//   missing or changed) counts as "not set up" for the caller and as a failed call here;
// - every call recorded under its use: the last result, failures in a row, and tokens per hour;
// - after FAIL_STREAK failures in a row one alert (AiServiceConfig.failing, 团队管理 › 要注意), one
//   活动记录 entry and one `ai.failing` notification to the admins; the next good call clears it
//   with another entry, and so does switching off the uses that were failing.
import { type AiFailingTransient, type AiServiceConfig, type AiServiceFailing, type AiServiceStatus, type AiServiceSwitches, SETTINGS_FEATURE, type ServiceUsage } from "@chat/protocol";
import { recordActivity } from "../activity/events";
import { featureTransient } from "../core/gateway";
import { type Env, gatewayStub } from "../core/env";
import type { Logger } from "../core/logger";
import { type PriceList, costOf, matchModel } from "../usage/prices";
import type { AiUse, CallUsage, ModelConfig, Tracker } from "./providers";
import { decryptKey, loadRow } from "./store";

/** Failed calls in a row (any use) that raise the alert. */
export const FAIL_STREAK = 3;
const ALERT = "*";
const HOUR_MS = 3_600_000;
/** How long the hourly token counts are kept. */
export const CALLS_KEEP_MS = 400 * 86_400_000;

export const ALL_ON: AiServiceSwitches = { frontDesk: true, titles: true, memory: true };
type Switch = keyof AiServiceSwitches;
const SWITCHES: Switch[] = ["frontDesk", "titles", "memory"];
export const SWITCH_OF: Record<AiUse, Switch> = { front_desk: "frontDesk", title: "titles", summary: "titles", retro: "memory", tidy: "memory" };

/** The error a use records when the saved key cannot be decrypted. */
export const KEY_UNREADABLE = "the saved API key cannot be read: the Worker secret HOOK_SECRET_KEY is missing or changed; enter the key again";

export async function loadSwitches(db: D1Database): Promise<AiServiceSwitches> {
  const rows = (await db.prepare("SELECT name, enabled FROM ai_service_switches").all<{ name: string; enabled: number }>()).results;
  const out = { ...ALL_ON };
  for (const r of rows) if ((SWITCHES as string[]).includes(r.name)) out[r.name as Switch] = r.enabled !== 0;
  return out;
}

export function switchStatements(db: D1Database, req: Partial<AiServiceSwitches>, by: string, now: number): D1PreparedStatement[] {
  return SWITCHES.filter((k) => req[k] !== undefined).map((k) =>
    db
      .prepare(
        `INSERT INTO ai_service_switches (name, enabled, updated_at, updated_by) VALUES (?, ?, ?, ?)
         ON CONFLICT (name) DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at, updated_by = excluded.updated_by`,
      )
      .bind(k, req[k] ? 1 : 0, now, by),
  );
}

const usesOf = (k: Switch): AiUse[] => (Object.keys(SWITCH_OF) as AiUse[]).filter((u) => SWITCH_OF[u] === k);

const DELETE_ALERT_IF_NONE_FAILING = `DELETE FROM ai_service_alert WHERE scope = ? AND NOT EXISTS (SELECT 1 FROM ai_service_health WHERE failures >= ?)`;

/**
 * For the switches `req` turns off: their uses' records go (they make no calls, so no good call
 * would ever clear them), and the alert when no use still at FAIL_STREAK is left. The last
 * statement is the alert's; pass its result to `afterSwitchedOff`. Empty when nothing is turned off.
 */
export function switchedOffStatements(db: D1Database, req: Partial<AiServiceSwitches>): D1PreparedStatement[] {
  const uses = SWITCHES.filter((k) => req[k] === false).flatMap(usesOf);
  if (uses.length === 0) return [];
  return [
    db.prepare(`DELETE FROM ai_service_health WHERE use IN (${uses.map(() => "?").join(", ")})`).bind(...uses),
    db.prepare(DELETE_ALERT_IF_NONE_FAILING).bind(ALERT, FAIL_STREAK),
  ];
}

/** Writes `ai.recovered` when switching uses off took the alert down. Never throws. */
export async function afterSwitchedOff(db: D1Database, alertResult: D1Result | undefined, req: Partial<AiServiceSwitches>, now: number, by: string): Promise<void> {
  if ((alertResult?.meta.changes ?? 0) === 0) return;
  const off = SWITCHES.filter((k) => req[k] === false);
  await recordActivity(db, { at: now, kind: "key", action: "ai.recovered", actorId: by, target: off.join(","), details: { switchedOff: off } }).catch(() => {});
}

/** The admins (human, active) learn of a raised alert on their connections with `settings/1`. Never throws. */
export async function notifyAdmins(env: Env, failing: AiServiceFailing, log?: Logger): Promise<void> {
  try {
    const admins = (
      await env.DB.prepare("SELECT id FROM members WHERE level = 'owner' AND kind = 'human' AND deactivated = 0 ORDER BY id LIMIT 50").all<{ id: string }>()
    ).results;
    const event: AiFailingTransient = { type: "ai.failing", failing };
    await Promise.all(
      admins.map((m) =>
        gatewayStub(env, m.id)
          .deliver([featureTransient(SETTINGS_FEATURE, event)])
          .catch((err) => log?.warn("ai failing notice not delivered", { memberId: m.id, err })),
      ),
    );
  } catch (err) {
    log?.warn("ai failing notice failed", { err });
  }
}

/** A new or removed config starts with a clean record: the old failures were about the old key. */
export function resetHealthStatements(db: D1Database): D1PreparedStatement[] {
  return [db.prepare("DELETE FROM ai_service_health"), db.prepare("DELETE FROM ai_service_alert")];
}

/**
 * The config for one use, or null when the team has none, the use is switched off, or the key
 * cannot be read (recorded as a failed call of the use, so the admins learn of it). Never throws.
 */
export async function useConfig(env: Env, use: AiUse, log?: Logger): Promise<ModelConfig | null> {
  let row: Awaited<ReturnType<typeof loadRow>>;
  let switches: AiServiceSwitches;
  try {
    [row, switches] = await Promise.all([loadRow(env.DB), loadSwitches(env.DB)]);
  } catch (err) {
    log?.warn("ai service unreadable", { use, err });
    return null;
  }
  if (!row || !switches[SWITCH_OF[use]]) return null;
  try {
    return { provider: row.provider, model: row.model, baseUrl: row.base_url, apiKey: await decryptKey(env, row.key_cipher) };
  } catch (err) {
    log?.warn("ai service key unreadable", { use, err });
    await recordCall(env, use, row.model, { ok: false, error: KEY_UNREADABLE }, Date.now(), log);
    return null;
  }
}

/** `CallOptions.track` for one use of `cfg`. */
export function tracker(env: Env, use: AiUse, cfg: Pick<ModelConfig, "model">, log?: Logger): Tracker {
  return (r) => recordCall(env, use, cfg.model, r, Date.now(), log);
}

type CallOutcome = { ok: true; usage?: CallUsage } | { ok: false; error: string };

/** Records one finished call; never throws. */
export async function recordCall(env: Env, use: AiUse, model: string, r: CallOutcome, now = Date.now(), log?: Logger): Promise<void> {
  const db = env.DB;
  const u = r.ok ? r.usage : undefined;
  const calls = db
    .prepare(
      `INSERT INTO ai_service_calls (hour, use, model, calls, failures, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens)
       VALUES (?, ?, ?, 1, ?, ?, ?, ?, ?)
       ON CONFLICT (hour, use, model) DO UPDATE SET calls = calls + 1, failures = failures + excluded.failures,
         input_tokens = input_tokens + excluded.input_tokens, output_tokens = output_tokens + excluded.output_tokens,
         cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens, cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens`,
    )
    .bind(Math.floor(now / HOUR_MS) * HOUR_MS, use, model.slice(0, 200), r.ok ? 0 : 1, u?.inputTokens ?? 0, u?.outputTokens ?? 0, u?.cacheReadTokens ?? 0, u?.cacheWriteTokens ?? 0);
  try {
    if (r.ok) {
      const res = await db.batch([
        calls,
        db
          .prepare(
            `INSERT INTO ai_service_health (use, at, ok, error, failures, ok_at) VALUES (?, ?, 1, NULL, 0, ?)
             ON CONFLICT (use) DO UPDATE SET at = excluded.at, ok = 1, error = NULL, failures = 0, ok_at = excluded.ok_at`,
          )
          .bind(use, now, now),
        db.prepare(DELETE_ALERT_IF_NONE_FAILING).bind(ALERT, FAIL_STREAK),
      ]);
      if ((res[2].meta.changes ?? 0) > 0) {
        log?.info("ai service works again", { use });
        await recordActivity(db, { at: now, kind: "key", action: "ai.recovered", target: use, details: { use, model } });
      }
      return;
    }
    const error = r.error.slice(0, 500);
    const res = await db.batch([
      calls,
      db
        .prepare(
          `INSERT INTO ai_service_health (use, at, ok, error, failures) VALUES (?, ?, 0, ?, 1)
           ON CONFLICT (use) DO UPDATE SET at = excluded.at, ok = 0, error = excluded.error, failures = failures + 1`,
        )
        .bind(use, now, error),
      db
        .prepare(`INSERT OR IGNORE INTO ai_service_alert (scope, since, use, error) SELECT ?, ?, use, error FROM ai_service_health WHERE use = ? AND failures >= ?`)
        .bind(ALERT, now, use, FAIL_STREAK),
    ]);
    if ((res[2].meta.changes ?? 0) > 0) {
      log?.warn("ai service keeps failing", { use, error });
      await recordActivity(db, { at: now, kind: "key", action: "ai.failing", target: use, details: { use, model, error, failures: FAIL_STREAK } });
      await notifyAdmins(env, { since: now, use, error }, log);
    }
  } catch (err) {
    log?.warn("ai service call not recorded", { use, err });
  }
}

/** The switches, the last call of each use and the alert, for `AiServiceConfig`. */
export async function healthFields(db: D1Database): Promise<Pick<AiServiceConfig, "switches" | "uses" | "failing">> {
  const [switches, health, alert] = await Promise.all([
    loadSwitches(db),
    db
      .prepare("SELECT use, at, ok, error, failures, ok_at FROM ai_service_health ORDER BY use LIMIT 16")
      .all<{ use: string; at: number; ok: number; error: string | null; failures: number; ok_at: number | null }>(),
    db.prepare("SELECT since, use, error FROM ai_service_alert WHERE scope = ?").bind(ALERT).first<{ since: number; use: string; error: string }>(),
  ]);
  return {
    switches,
    ...(health.results.length > 0
      ? {
          uses: health.results.map((h) => ({
            use: h.use,
            at: h.at,
            ok: h.ok !== 0,
            ...(h.error ? { error: h.error } : {}),
            failures: h.failures,
            ...(h.ok_at ? { okAt: h.ok_at } : {}),
          })),
        }
      : {}),
    ...(alert ? { failing: { since: alert.since, use: alert.use, error: alert.error } } : {}),
  };
}

/** `GET /v1/settings/ai/status`: set up or not, and which uses are on. No details. */
export async function aiStatus(db: D1Database): Promise<AiServiceStatus> {
  const [row, switches] = await Promise.all([db.prepare("SELECT 1 AS x FROM ai_service LIMIT 1").first(), loadSwitches(db)]);
  const configured = row !== null;
  return { configured, frontDesk: configured && switches.frontDesk, titles: configured && switches.titles, memory: configured && switches.memory };
}

/** 用量's 前台和整理 line: the calls that ended in [from, to), counted by the hour they ended in. */
export async function serviceUsage(db: D1Database, from: number, to: number, prices: PriceList): Promise<ServiceUsage | undefined> {
  const rows = (
    await db
      .prepare(
        `SELECT use, model, sum(calls) AS calls, sum(failures) AS failures, sum(input_tokens) AS input, sum(output_tokens) AS output,
                sum(cache_read_tokens) AS cache_read, sum(cache_write_tokens) AS cache_write
         FROM ai_service_calls WHERE hour >= ? AND hour < ? GROUP BY use, model`,
      )
      .bind(Math.floor(from / HOUR_MS) * HOUR_MS, to)
      .all<{ use: string; model: string; calls: number; failures: number; input: number; output: number; cache_read: number; cache_write: number }>()
  ).results;
  if (rows.length === 0) return undefined;
  const out: ServiceUsage = { calls: 0, failures: 0, costUsd: 0, inputTokens: 0, outputTokens: 0, uses: [] };
  let cacheRead = 0;
  let unpriced = 0;
  const byUse = new Map<string, ServiceUsage["uses"][number]>();
  for (const r of rows) {
    out.calls += r.calls;
    out.failures += r.failures;
    out.inputTokens += r.input;
    out.outputTokens += r.output;
    cacheRead += r.cache_read;
    const tokens = { inputTokens: r.input, outputTokens: r.output, cacheReadTokens: r.cache_read, cacheWriteTokens: r.cache_write };
    const price = matchModel(prices, r.model);
    if (price) out.costUsd += costOf(price.price, tokens);
    else unpriced += r.input + r.output + r.cache_read + r.cache_write;
    const u = byUse.get(r.use) ?? { use: r.use, calls: 0, inputTokens: 0, outputTokens: 0 };
    u.calls += r.calls;
    u.inputTokens += r.input;
    u.outputTokens += r.output;
    byUse.set(r.use, u);
  }
  out.costUsd = Math.round(out.costUsd * 10_000) / 10_000;
  if (cacheRead > 0) out.cacheReadTokens = cacheRead;
  if (unpriced > 0) out.unpricedTokens = unpriced;
  out.uses = [...byUse.values()].sort((a, b) => b.inputTokens + b.outputTokens - (a.inputTokens + a.outputTokens)).slice(0, 16);
  return out;
}
