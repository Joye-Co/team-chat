// Phone push (docs/protocol.md, "Push"): device registration, the 等你 badge, APNs pushes for
// the notifications the room decides, and answering a card from a notification over HTTP.
//
// The member's gateway sees every event frame on its way to the member (also while no socket is
// open). Its frame filter runs after the notification settings filter (settings/notify.ts), so a
// frame that still has `notify` is one the member wants to hear about. The filter keeps the
// waiting topics (the badge) and the open asks, and queues pushes in the gateway's SQLite; the
// gateway alarm sends them.
import {
  AnswerCardRequest,
  type BadgeResponse,
  type CardEventAck,
  type NotifyEvent,
  type NotifyReason,
  type PushData,
  type PushDevice,
  RegisterPushDeviceRequest,
} from "@chat/protocol";
import type { AuthContext } from "../core/auth";
import { getGroup, getMember, groupsOfMember, isGroupMember } from "../core/db";
import { gatewayStub, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import {
  type Actor,
  type GatewayContext,
  registerGatewayAlarm,
  registerGatewayFrameFilter,
  registerGatewayMigration,
  registerGatewayRpc,
  registerRoomRpc,
  registerRoute,
} from "../core/registry";
import { CARD_EVENT_RPC } from "../cards2/index";
import { type PushEnv, apnsConfigured, sendApns } from "./apns";
import { alertPayload, alertText, badgePayload, excerpt, quickActions } from "./payload";

/** A queued push is dropped after this many tries. */
const MAX_TRIES = 3;
const RETRY_MS = 30_000;
const DRAIN_BATCH = 50;
/** Alerts older than this are not worth showing. */
const ALERT_TTL_S = 24 * 3600;

// ---------------------------------------------------------------------------
// Gateway storage
// ---------------------------------------------------------------------------

registerGatewayMigration({
  name: "push-1",
  up: `
    CREATE TABLE push_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE push_waiting (topic_id TEXT PRIMARY KEY, group_id TEXT NOT NULL);
    CREATE TABLE push_asks (
      message_id TEXT PRIMARY KEY,
      group_id TEXT NOT NULL,
      topic_id TEXT NOT NULL,
      rev INTEGER NOT NULL,
      actions TEXT NOT NULL,
      at INTEGER NOT NULL
    );
    CREATE INDEX push_asks_topic ON push_asks(topic_id, at);
    CREATE TABLE push_outbox (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      kind TEXT NOT NULL,
      json TEXT NOT NULL,
      tries INTEGER NOT NULL DEFAULT 0,
      next_at INTEGER NOT NULL
    );
  `,
});

function stateGet(sql: SqlStorage, key: string): string | undefined {
  return sql.exec<{ value: string }>("SELECT value FROM push_state WHERE key = ?", key).toArray()[0]?.value;
}
function stateSet(sql: SqlStorage, key: string, value: string): void {
  sql.exec("INSERT OR REPLACE INTO push_state (key, value) VALUES (?, ?)", key, value);
}

/** The member this gateway pushes for; set once a device registers. */
function pushMember(gw: GatewayContext): string | undefined {
  if (stateGet(gw.sql, "enabled") !== "1") return undefined;
  return gw.memberId ?? stateGet(gw.sql, "member_id");
}

function waitingCount(sql: SqlStorage): number {
  return sql.exec<{ n: number }>("SELECT count(*) AS n FROM push_waiting").one().n;
}

/** What an alert push needs besides `PushData`, read from the frame. */
interface AlertQueued {
  data: PushData;
  actorId: string;
  text?: string;
  topicTitle?: string;
  artifactTitle?: string;
}

function enqueue(gw: GatewayContext, kind: "alert" | "badge", body: unknown): void {
  const now = Date.now();
  if (kind === "badge") gw.sql.exec("DELETE FROM push_outbox WHERE kind = 'badge' AND tries = 0");
  gw.sql.exec("INSERT INTO push_outbox (kind, json, next_at) VALUES (?, ?, ?)", kind, JSON.stringify(body), now);
  gw.wakeAt(now);
}

// ---------------------------------------------------------------------------
// The frame filter: track waiting topics and asks, queue pushes
// ---------------------------------------------------------------------------

type Ev = { type?: string; groupId?: string; seq?: number; actorId?: string; data?: Record<string, any> };

function trackAsk(sql: SqlStorage, memberId: string, groupId: string, message: Record<string, any>, at: number): void {
  const card = message.cardV2 as Record<string, any> | undefined;
  const ask = message.cardState?.ask as { status?: string; from?: string[] } | undefined;
  if (!card || !ask) return;
  if (ask.status !== "open" || !ask.from?.includes(memberId)) {
    sql.exec("DELETE FROM push_asks WHERE message_id = ?", message.id);
    return;
  }
  sql.exec(
    "INSERT OR REPLACE INTO push_asks (message_id, group_id, topic_id, rev, actions, at) VALUES (?, ?, ?, ?, ?, ?)",
    message.id,
    groupId,
    message.topicId,
    card.rev ?? 1,
    JSON.stringify(quickActions(card)),
    at,
  );
}

/** Keeps `push_waiting` and `push_asks` in step with one event. */
function track(sql: SqlStorage, memberId: string, ev: Ev): void {
  const d = ev.data ?? {};
  const groupId = ev.groupId ?? "";
  switch (ev.type) {
    case "topic.created": {
      const t = d.topic as { id: string; waitingOn?: string[] } | undefined;
      if (t?.waitingOn?.includes(memberId)) sql.exec("INSERT OR REPLACE INTO push_waiting (topic_id, group_id) VALUES (?, ?)", t.id, groupId);
      break;
    }
    case "topic.updated":
      if (Array.isArray(d.waitingOn)) {
        if (d.waitingOn.includes(memberId)) sql.exec("INSERT OR REPLACE INTO push_waiting (topic_id, group_id) VALUES (?, ?)", d.topicId, groupId);
        else sql.exec("DELETE FROM push_waiting WHERE topic_id = ?", d.topicId);
      }
      break;
    case "message.created":
      if (d.message) trackAsk(sql, memberId, groupId, d.message, Date.now());
      break;
    case "message.updated": {
      const ask = d.cardState?.ask as { status?: string; from?: string[] } | undefined;
      if (ask && (ask.status !== "open" || !ask.from?.includes(memberId))) sql.exec("DELETE FROM push_asks WHERE message_id = ?", d.messageId);
      else if (d.cardV2) {
        sql.exec("UPDATE push_asks SET rev = ?, actions = ? WHERE message_id = ?", d.cardV2.rev ?? 1, JSON.stringify(quickActions(d.cardV2)), d.messageId);
      }
      break;
    }
    case "message.deleted":
      sql.exec("DELETE FROM push_asks WHERE message_id = ?", d.messageId);
      break;
    case "group.memberRemoved":
      if (d.memberId === memberId) {
        sql.exec("DELETE FROM push_waiting WHERE group_id = ?", groupId);
        sql.exec("DELETE FROM push_asks WHERE group_id = ?", groupId);
      }
      break;
  }
}

function topicOf(ev: Ev): string | undefined {
  const d = ev.data ?? {};
  return d.message?.topicId ?? d.topicId ?? d.topic?.id ?? d.artifact?.topicId ?? undefined;
}

/** The ask a push for this event offers, if any. */
function askFor(sql: SqlStorage, ev: Ev, reason: string, topicId: string | undefined): PushData["ask"] {
  let row: { message_id: string; rev: number; actions: string } | undefined;
  if (ev.type === "message.created" && ev.data?.message?.id) {
    row = sql.exec<{ message_id: string; rev: number; actions: string }>("SELECT message_id, rev, actions FROM push_asks WHERE message_id = ?", ev.data.message.id).toArray()[0];
  } else if (reason === "waiting" && topicId) {
    row = sql
      .exec<{ message_id: string; rev: number; actions: string }>("SELECT message_id, rev, actions FROM push_asks WHERE topic_id = ? ORDER BY at DESC LIMIT 1", topicId)
      .toArray()[0];
  }
  if (!row) return undefined;
  const actions = JSON.parse(row.actions) as { id: string; label: string }[];
  return actions.length > 0 ? { messageId: row.message_id, rev: row.rev, actions } : undefined;
}

export function pushFilter(gw: GatewayContext, frame: Record<string, unknown>): Record<string, unknown> {
  if (frame.t !== "event") return frame;
  const memberId = pushMember(gw);
  if (!memberId) return frame;
  const ev = frame.event as Ev;
  track(gw.sql, memberId, ev);
  const badge = waitingCount(gw.sql);

  const reason = frame.notify as NotifyReason | undefined;
  if (reason && ev.groupId && ev.actorId) {
    const topicId = topicOf(ev);
    const d = ev.data ?? {};
    const message = d.message as { id: string; body?: string; cardV2?: { title?: string } } | undefined;
    const data: PushData = {
      v: 1,
      reason,
      ...(typeof frame.notifyEvent === "string" ? { notifyEvent: frame.notifyEvent as NotifyEvent } : {}),
      groupId: ev.groupId,
      ...(topicId ? { topicId } : {}),
      ...(message ? { messageId: message.id } : d.messageId ? { messageId: d.messageId } : {}),
      seq: ev.seq ?? 0,
    };
    const ask = askFor(gw.sql, ev, reason, topicId);
    if (ask) data.ask = ask;
    const queued: AlertQueued = {
      data,
      actorId: ev.actorId,
      ...(message ? { text: excerpt(message.body || message.cardV2?.title || "") } : {}),
      ...(typeof d.title === "string" ? { topicTitle: d.title } : d.topic?.title ? { topicTitle: d.topic.title } : {}),
      ...(d.artifact?.title ? { artifactTitle: d.artifact.title } : {}),
    };
    enqueue(gw, "alert", queued);
    stateSet(gw.sql, "badge", String(badge));
  } else if (String(badge) !== (stateGet(gw.sql, "badge") ?? String(badge))) {
    enqueue(gw, "badge", { badge });
    stateSet(gw.sql, "badge", String(badge));
  }
  return frame;
}

registerGatewayFrameFilter(pushFilter);

// ---------------------------------------------------------------------------
// Sending (gateway alarm)
// ---------------------------------------------------------------------------

type DeviceRow = { id: string; token_id: string; push_token: string; app_id: string; environment: string };

async function liveDevices(env: PushEnv, memberId: string): Promise<DeviceRow[]> {
  // Devices of revoked sign-ins go away here (sign out, 退出这台设备, deactivation).
  await env.DB.prepare(
    "DELETE FROM push_devices WHERE member_id = ? AND token_id IN (SELECT id FROM tokens WHERE member_id = ? AND revoked_at IS NOT NULL)",
  )
    .bind(memberId, memberId)
    .run();
  const r = await env.DB.prepare(
    `SELECT d.id, d.token_id, d.push_token, d.app_id, d.environment FROM push_devices d
     JOIN tokens t ON t.id = d.token_id AND t.revoked_at IS NULL WHERE d.member_id = ?`,
  )
    .bind(memberId)
    .all<DeviceRow>();
  return r.results;
}

async function buildAlert(env: PushEnv, q: AlertQueued, badge: number): Promise<Record<string, unknown>> {
  const [group, actor] = await Promise.all([getGroup(env.DB, q.data.groupId), getMember(env.DB, q.actorId)]);
  const isDm = group?.kind === "dm";
  const actorName = actor?.name ?? "";
  let topicTitle = q.topicTitle;
  if (!topicTitle && q.data.topicId && q.data.reason !== "mention" && q.data.reason !== "message") {
    try {
      topicTitle = (unwrap(await roomStub(env, q.data.groupId).rpc(q.data.groupId, "push.topic", null, { topicId: q.data.topicId })) as { title?: string }).title;
    } catch {
      // the alert still goes out without the title
    }
  }
  const groupName = isDm ? actorName || group?.name || "" : (group?.name ?? "");
  return alertPayload(q.data, alertText({ data: q.data, groupName, isDm, actorName, text: q.text, topicTitle, artifactTitle: q.artifactTitle }), badge);
}

export async function drainPush(gw: GatewayContext): Promise<void> {
  const env = gw.env as PushEnv;
  const now = Date.now();
  const rows = gw.sql
    .exec<{ id: number; kind: string; json: string; tries: number; next_at: number }>("SELECT * FROM push_outbox ORDER BY id LIMIT ?", DRAIN_BATCH)
    .toArray();
  if (rows.length === 0) return;
  const memberId = pushMember(gw);
  if (!memberId || !apnsConfigured(env)) {
    gw.sql.exec("DELETE FROM push_outbox");
    return;
  }
  const due = rows.filter((r) => r.next_at <= now);
  if (due.length === 0) {
    gw.wakeAt(Math.min(...rows.map((r) => r.next_at)));
    return;
  }
  const open = new Set(gw.openTokenIds());
  const devices = (await liveDevices(env, memberId)).filter((d) => !open.has(d.token_id));
  const badge = waitingCount(gw.sql);
  for (const row of due) {
    let retry = false;
    if (devices.length > 0) {
      const body = JSON.parse(row.json) as AlertQueued | { badge: number };
      const alert = row.kind === "alert";
      const payload = alert ? await buildAlert(env, body as AlertQueued, badge) : badgePayload((body as { badge: number }).badge);
      const data = alert ? (body as AlertQueued).data : undefined;
      for (const d of devices) {
        const r = await sendApns(env, {
          token: d.push_token,
          appId: d.app_id,
          environment: d.environment,
          payload,
          priority: alert ? 10 : 5,
          collapseId: data?.topicId ?? data?.groupId,
          expiration: alert ? Math.floor(now / 1000) + ALERT_TTL_S : 0,
        });
        if (r.result === "gone") {
          await env.DB.prepare("DELETE FROM push_devices WHERE id = ?").bind(d.id).run();
          gw.log.info("push device removed", { deviceId: d.id, status: r.status, reason: r.reason });
        } else if (r.result === "retry") {
          retry = true;
        } else if (r.result === "failed") {
          gw.log.warn("push failed", { deviceId: d.id, status: r.status, reason: r.reason });
        }
      }
    }
    if (retry && row.tries + 1 < MAX_TRIES) {
      gw.sql.exec("UPDATE push_outbox SET tries = tries + 1, next_at = ? WHERE id = ?", now + RETRY_MS * (row.tries + 1), row.id);
    } else {
      gw.sql.exec("DELETE FROM push_outbox WHERE id = ?", row.id);
    }
  }
  const next = gw.sql.exec<{ at: number | null }>("SELECT min(next_at) AS at FROM push_outbox").one().at;
  if (next !== null) gw.wakeAt(next);
}

registerGatewayAlarm(drainPush);

// ---------------------------------------------------------------------------
// Room RPCs: waiting topics (badge sync) and a topic's title
// ---------------------------------------------------------------------------

registerRoomRpc("push.waiting", (room, _actor, args) => {
  const { memberId } = args as { memberId: string };
  return room.sql
    .exec<{ id: string }>("SELECT id FROM topics WHERE EXISTS (SELECT 1 FROM json_each(topics.waiting_on) WHERE value = ?)", memberId)
    .toArray()
    .map((r) => r.id);
});

registerRoomRpc("push.topic", (room, _actor, args) => {
  const { topicId } = args as { topicId: string };
  return { title: room.sql.exec<{ title: string }>("SELECT title FROM topics WHERE id = ?", topicId).toArray()[0]?.title };
});

// ---------------------------------------------------------------------------
// Gateway RPCs
// ---------------------------------------------------------------------------

/** Turns pushing on for this member and recounts the waiting topics from the rooms. */
registerGatewayRpc("push.enable", async (gw, args) => {
  const { memberId } = args as { memberId: string };
  stateSet(gw.sql, "member_id", memberId);
  stateSet(gw.sql, "enabled", "1");
  const groups = await groupsOfMember(gw.env.DB, memberId);
  const waiting = await Promise.all(
    groups.map(async ({ row }) => {
      try {
        return { groupId: row.id, topics: unwrap(await roomStub(gw.env, row.id).rpc(row.id, "push.waiting", null, { memberId })) as string[] };
      } catch {
        return { groupId: row.id, topics: [] as string[] };
      }
    }),
  );
  gw.sql.exec("DELETE FROM push_waiting");
  for (const w of waiting) for (const t of w.topics) gw.sql.exec("INSERT OR REPLACE INTO push_waiting (topic_id, group_id) VALUES (?, ?)", t, w.groupId);
  const badge = waitingCount(gw.sql);
  stateSet(gw.sql, "badge", String(badge));
  return { waiting: badge };
});

registerGatewayRpc("push.badge", (gw) => {
  return { waiting: stateGet(gw.sql, "enabled") === "1" ? waitingCount(gw.sql) : null };
});

// ---------------------------------------------------------------------------
// HTTP routes
// ---------------------------------------------------------------------------

type DeviceDbRow = DeviceRow & { platform: string; created_at: number; updated_at: number };

function viewDevice(r: DeviceDbRow, tokenId: string): PushDevice {
  return {
    id: r.id,
    platform: r.platform as PushDevice["platform"],
    appId: r.app_id,
    environment: r.environment as PushDevice["environment"],
    createdAt: r.created_at,
    updatedAt: r.updated_at,
    current: r.token_id === tokenId,
  };
}

registerRoute({
  method: "POST",
  path: "/v1/me/push-devices",
  auth: "member",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const req = await body(RegisterPushDeviceRequest);
    const now = Date.now();
    const existing = await env.DB.prepare("SELECT id, created_at FROM push_devices WHERE push_token = ?").bind(req.token).first<{ id: string; created_at: number }>();
    const id = existing?.id ?? ulid();
    await env.DB.prepare(
      `INSERT INTO push_devices (id, member_id, token_id, platform, push_token, app_id, environment, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(push_token) DO UPDATE SET member_id = excluded.member_id, token_id = excluded.token_id,
         platform = excluded.platform, app_id = excluded.app_id, environment = excluded.environment, updated_at = excluded.updated_at`,
    )
      .bind(id, a.memberId, a.tokenId, req.platform, req.token, req.appId, req.environment, existing?.created_at ?? now, now)
      .run();
    await gatewayStub(env, a.memberId).rpc("push.enable", { memberId: a.memberId });
    const row = await env.DB.prepare("SELECT * FROM push_devices WHERE id = ?").bind(id).first<DeviceDbRow>();
    return viewDevice(row as DeviceDbRow, a.tokenId);
  },
});

registerRoute({
  method: "GET",
  path: "/v1/me/push-devices",
  auth: "member",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    const r = await env.DB.prepare(
      `SELECT d.* FROM push_devices d JOIN tokens t ON t.id = d.token_id AND t.revoked_at IS NULL
       WHERE d.member_id = ? ORDER BY d.updated_at DESC LIMIT 100`,
    )
      .bind(a.memberId)
      .all<DeviceDbRow>();
    return { devices: r.results.map((x) => viewDevice(x, a.tokenId)) };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/me/push-devices",
  auth: "member",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    const r = await env.DB.prepare("DELETE FROM push_devices WHERE member_id = ? AND token_id = ?").bind(a.memberId, a.tokenId).run();
    return { removed: r.meta.changes ?? 0 };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/me/push-devices/:id",
  auth: "member",
  async handler({ env, auth, params }) {
    const a = auth as AuthContext;
    const r = await env.DB.prepare("DELETE FROM push_devices WHERE id = ? AND member_id = ?").bind(params.id, a.memberId).run();
    if (!r.meta.changes) throw new ApiError("not_found", "device not found");
    return { removed: r.meta.changes };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/me/badge",
  auth: "member",
  async handler({ env, auth }): Promise<BadgeResponse> {
    const a = auth as AuthContext;
    const gw = gatewayStub(env, a.memberId);
    let { waiting } = (await gw.rpc("push.badge", {})) as { waiting: number | null };
    if (waiting === null) waiting = ((await gw.rpc("push.enable", { memberId: a.memberId })) as { waiting: number }).waiting;
    return { waiting };
  },
});

registerRoute({
  method: "POST",
  path: "/v1/groups/:id/messages/:messageId/answer",
  auth: "member",
  async handler({ env, auth, params, body }): Promise<CardEventAck> {
    const a = auth as AuthContext;
    if (!(await gatewayStub(env, a.memberId).rpc("http.post.take", { tokenId: a.tokenId }))) {
      throw new ApiError("rate_limited", "too many posts");
    }
    const req = await body(AnswerCardRequest);
    const group = await getGroup(env.DB, params.id);
    if (!group) throw new ApiError("not_found", "group not found");
    if (!(await isGroupMember(env.DB, group.id, a.memberId))) throw new ApiError("forbidden", "not a member of this group");
    const actor: Actor = { id: a.memberId, level: a.member.level, kind: a.member.kind, tokenId: a.tokenId };
    return unwrap(
      await roomStub(env, group.id).rpc(group.id, CARD_EVENT_RPC, actor, {
        messageId: params.messageId,
        rev: req.rev,
        actionId: req.actionId,
        clientEventId: req.clientEventId,
        event: "action.press",
      }),
    ) as CardEventAck;
  },
});
