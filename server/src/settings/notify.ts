// Notification settings per member (docs/protocol.md, "Settings"). They live in the member's
// gateway SQLite, because the gateway delivers every event frame to that member: it filters
// `notify` there, logs what a digest counts, and sends the daily digest from its alarm.
import {
  type DigestTransient,
  type NotifyEvent,
  type NotifySettings,
  SETTINGS_FEATURE,
  UpdateNotifySettingsRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { gatewayStub } from "../core/env";
import { ApiError } from "../core/errors";
import { featureTransient } from "../core/gateway";
import {
  type GatewayContext,
  registerGatewayAlarm,
  registerGatewayFrameFilter,
  registerGatewayMigration,
  registerGatewayRpc,
  registerRoute,
} from "../core/registry";
import { SWITCHABLE, applyNotifyUpdate, isKnownZone, keepNotify, nextLocalTime, withDefaults } from "./rules";

/** Log rows older than this are dropped (a digest covers at most a week). */
const LOG_KEEP_MS = 7 * 86_400_000;
const DIGEST_ITEMS = 20;

registerGatewayMigration({
  name: "settings-1",
  up: `
    CREATE TABLE notify_settings (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
    CREATE TABLE notify_log (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      at INTEGER NOT NULL,
      event TEXT NOT NULL,
      group_id TEXT NOT NULL,
      topic_id TEXT NOT NULL
    );
    CREATE INDEX notify_log_at ON notify_log(at);
    CREATE TABLE digest_state (id INTEGER PRIMARY KEY CHECK (id = 1), last_at INTEGER NOT NULL, next_at INTEGER);
  `,
});

export function loadNotifySettings(sql: SqlStorage): NotifySettings {
  const row = sql.exec<{ json: string }>("SELECT json FROM notify_settings WHERE id = 1").toArray()[0];
  if (!row) return withDefaults(null);
  try {
    return withDefaults(JSON.parse(row.json) as Partial<NotifySettings>);
  } catch {
    return withDefaults(null);
  }
}

function digestState(sql: SqlStorage): { last_at: number; next_at: number | null } | undefined {
  return sql.exec<{ last_at: number; next_at: number | null }>("SELECT last_at, next_at FROM digest_state WHERE id = 1").toArray()[0];
}

/** Plans the next digest (or clears it) after a settings change or a digest. */
function planDigest(gw: GatewayContext, s: NotifySettings, now: number): void {
  const next = s.digestAt ? nextLocalTime(now, s.digestAt, s.timeZone) : null;
  const state = digestState(gw.sql);
  if (state) gw.sql.exec("UPDATE digest_state SET next_at = ? WHERE id = 1", next);
  else gw.sql.exec("INSERT INTO digest_state (id, last_at, next_at) VALUES (1, ?, ?)", now, next);
  if (next !== null) gw.wakeAt(next);
}

// ---------------------------------------------------------------------------
// Filter: the `notify` field of event frames under the member's settings
// ---------------------------------------------------------------------------

registerGatewayFrameFilter((gw, frame) => {
  if (frame.t !== "event" || frame.notify === undefined) return frame;
  const s = loadNotifySettings(gw.sql);
  const now = Date.now();
  const notifyEvent = typeof frame.notifyEvent === "string" ? frame.notifyEvent : undefined;
  // The digest counts what the member wants to hear about, also what quiet hours held back.
  if (s.digestAt && notifyEvent && SWITCHABLE.has(notifyEvent) && s.events[notifyEvent as NotifyEvent]) {
    const event = frame.event as { groupId?: string; data?: Record<string, unknown> } | undefined;
    const topicId = eventTopic(event);
    if (event?.groupId && topicId) {
      gw.sql.exec("INSERT INTO notify_log (at, event, group_id, topic_id) VALUES (?, ?, ?, ?)", now, notifyEvent, event.groupId, topicId);
    }
  }
  if (keepNotify(s, notifyEvent, now)) return frame;
  const { notify: _n, notifyEvent: _e, ...rest } = frame;
  return rest;
});

function eventTopic(event: { data?: Record<string, unknown> } | undefined): string | undefined {
  const d = event?.data;
  if (!d) return undefined;
  if (typeof d.topicId === "string") return d.topicId;
  const nested = (d.message ?? d.topic ?? d.artifact) as { topicId?: unknown; id?: unknown } | undefined;
  if (typeof nested?.topicId === "string") return nested.topicId;
  if (d.topic && typeof nested?.id === "string") return nested.id;
  return undefined;
}

// ---------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------

/** The digest since `since`, from the log. */
export function buildDigest(sql: SqlStorage, since: number, at: number): DigestTransient {
  const counts = { waiting: 0, mention: 0, result: 0, blocked: 0 };
  for (const r of sql.exec<{ event: string; n: number }>("SELECT event, count(*) AS n FROM notify_log WHERE at > ? AND at <= ? GROUP BY event", since, at)) {
    if (r.event in counts) counts[r.event as keyof typeof counts] = r.n;
  }
  const items = sql
    .exec<{ event: string; group_id: string; topic_id: string }>(
      `SELECT event, group_id, topic_id, max(at) AS last FROM notify_log WHERE at > ? AND at <= ?
       GROUP BY event, group_id, topic_id ORDER BY last DESC LIMIT ?`,
      since,
      at,
      DIGEST_ITEMS,
    )
    .toArray()
    .map((r) => ({ groupId: r.group_id, topicId: r.topic_id, event: r.event as NotifyEvent }));
  return { type: "digest", at, since, counts, items };
}

registerGatewayAlarm((gw) => {
  const state = digestState(gw.sql);
  const s = loadNotifySettings(gw.sql);
  if (!state || state.next_at === null || !s.digestAt) return;
  const now = Date.now();
  if (now < state.next_at) {
    gw.wakeAt(state.next_at);
    return;
  }
  // Offline at digest time: nothing is sent and the next digest covers both days.
  if (gw.online()) {
    const digest = buildDigest(gw.sql, state.last_at, now);
    gw.broadcast(featureTransient(SETTINGS_FEATURE, digest));
    gw.sql.exec("UPDATE digest_state SET last_at = ? WHERE id = 1", now);
  }
  gw.sql.exec("DELETE FROM notify_log WHERE at < ?", now - LOG_KEEP_MS);
  planDigest(gw, s, now);
});

// ---------------------------------------------------------------------------
// GET and PATCH /v1/me/notify-settings
// ---------------------------------------------------------------------------

registerGatewayRpc("settings.notify.get", (gw) => loadNotifySettings(gw.sql));

registerGatewayRpc("settings.notify.set", (gw, args) => {
  const { update, now } = args as { update: UpdateNotifySettingsRequest; now: number };
  const before = loadNotifySettings(gw.sql);
  const after = applyNotifyUpdate(before, update, now);
  gw.sql.exec("INSERT OR REPLACE INTO notify_settings (id, json) VALUES (1, ?)", JSON.stringify(after));
  if (after.digestAt !== before.digestAt || after.timeZone !== before.timeZone || !digestState(gw.sql)) planDigest(gw, after, now);
  return { before, after };
});

registerRoute({
  method: "GET",
  path: "/v1/me/notify-settings",
  auth: "member",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    return (await gatewayStub(env, a.memberId).rpc("settings.notify.get", {})) as NotifySettings;
  },
});

registerRoute({
  method: "PATCH",
  path: "/v1/me/notify-settings",
  auth: "member",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const update = await body(UpdateNotifySettingsRequest);
    if (update.timeZone !== undefined && !isKnownZone(update.timeZone)) throw new ApiError("invalid", "unknown time zone");
    const r = (await gatewayStub(env, a.memberId).rpc("settings.notify.set", { update, now: Date.now() })) as {
      before: NotifySettings;
      after: NotifySettings;
    };
    const changed = (Object.keys(update) as (keyof NotifySettings)[]).filter(
      (k) => JSON.stringify(r.before[k]) !== JSON.stringify(r.after[k]),
    );
    if (changed.length > 0) {
      await auditStatement(env.DB, a, { action: "settings.notify", target: a.memberId, details: { changed } }).run();
    }
    return r.after;
  },
});
