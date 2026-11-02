// Hook deliveries run in the MemberGateway of the hook's bot member (one object per hook). A
// promise chain per hook serializes deliveries, so two GitHub events for one pull request never
// create two topics. The gateway SQLite keeps the topic keys, the cards and the delivery ids.
import { Card, type SendResult, type Topic, type TopicStatus } from "@chat/protocol";
import { type Env, roomStub } from "../core/env";
import { ApiError, type Result, toErrorBody, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import type { Logger } from "../core/logger";
import { LIMITS, TokenBucket } from "../core/ratelimit";
import { type Actor, registerGatewayMigration, registerGatewayRpc } from "../core/registry";
import {
  type GhEvent,
  type GhState,
  applyCi,
  applyEvent,
  branchKey,
  buildCard,
  desiredTopic,
  linkedTopicId,
  prKey,
  topicTitle,
} from "./github";

registerGatewayMigration({
  name: "integrations-1",
  up: `
    CREATE TABLE hook_keys (
      key TEXT PRIMARY KEY,
      topic_id TEXT NOT NULL,
      message_id TEXT,
      own_topic INTEGER NOT NULL DEFAULT 1,
      state TEXT,
      card TEXT,
      applied TEXT,
      updated_at INTEGER NOT NULL
    );
    CREATE TABLE hook_shas (sha TEXT PRIMARY KEY, key TEXT NOT NULL, at INTEGER NOT NULL);
    CREATE INDEX hook_shas_at ON hook_shas(at);
    CREATE TABLE hook_deliveries (id TEXT PRIMARY KEY, at INTEGER NOT NULL);
    CREATE INDEX hook_deliveries_at ON hook_deliveries(at);
  `,
});

/** Delivery ids are kept this long for dedupe; GitHub redeliveries come within days. */
const DELIVERY_TTL_MS = 14 * 24 * 60 * 60 * 1000;
const SHA_TTL_MS = 90 * 24 * 60 * 60 * 1000;

export interface HookRef {
  id: string;
  groupId: string;
  topicId: string | null;
  memberId: string;
}

export type DeliverArgs =
  | {
      op: "in";
      hook: HookRef;
      topicId?: string;
      topicKey?: string;
      title?: string;
      body: string;
      card?: Card;
      clientMsgId: string;
    }
  | {
      op: "github";
      hook: HookRef;
      deliveryId: string | null;
      event: GhEvent;
      /** GitHub login (lower case) -> member id. */
      logins: Record<string, string>;
    };

// In-memory, per gateway isolate: rate buckets and the per-hook chain.
const buckets = new Map<string, TokenBucket>();
const chains = new Map<string, Promise<unknown>>();

function serialize<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const prev = chains.get(key) ?? Promise.resolve();
  const next = prev.then(fn, fn);
  chains.set(key, next);
  const done = () => {
    if (chains.get(key) === next) chains.delete(key);
  };
  next.then(done, done);
  return next;
}

function take(hook: HookRef, op: DeliverArgs["op"]): boolean {
  let b = buckets.get(hook.id);
  if (!b) {
    // GitHub sends bursts (check suites plus workflow runs per push), so its burst is larger.
    b = op === "github" ? new TokenBucket(LIMITS.write.rate, 60) : new TokenBucket(LIMITS.write.rate, LIMITS.write.burst);
    buckets.set(hook.id, b);
  }
  return b.take();
}

registerGatewayRpc("hooks.deliver", async (gw, raw): Promise<Result<unknown>> => {
  const args = raw as DeliverArgs;
  try {
    if (!take(args.hook, args.op)) throw new ApiError("rate_limited", "too many posts");
    const value = await serialize<unknown>(args.hook.id, () =>
      args.op === "in" ? deliverIn(gw.env, gw.sql, args) : deliverGithub(gw.env, gw.sql, gw.log, args),
    );
    return { ok: true, value };
  } catch (err) {
    if (!(err instanceof ApiError)) gw.log.error("hook delivery failed", { hookId: args.hook.id, err });
    return { ok: false, error: toErrorBody(err) };
  }
});

const actorOf = (hook: HookRef): Actor => ({ id: hook.memberId, level: "member", kind: "bot", tokenId: hook.id });

type KeyRow = {
  key: string;
  topic_id: string;
  message_id: string | null;
  own_topic: number;
  state: string | null;
  card: string | null;
  applied: string | null;
};

function keyRow(sql: SqlStorage, key: string): KeyRow | undefined {
  return sql.exec<KeyRow>("SELECT * FROM hook_keys WHERE key = ?", key).toArray()[0] as KeyRow | undefined;
}

async function send(
  env: Env,
  hook: HookRef,
  frame: { topicId?: string; title?: string; body: string; card?: Card; clientMsgId: string },
): Promise<Result<SendResult>> {
  return roomStub(env, hook.groupId).send(hook.groupId, actorOf(hook), {
    t: "send",
    rid: "hook",
    groupId: hook.groupId,
    ...frame,
  });
}

// ---------------------------------------------------------------------------
// Generic hook
// ---------------------------------------------------------------------------

async function deliverIn(env: Env, sql: SqlStorage, a: Extract<DeliverArgs, { op: "in" }>): Promise<SendResult> {
  const mapped = a.topicKey ? keyRow(sql, `in:${a.topicKey}`) : undefined;
  const topicId = a.topicId ?? mapped?.topic_id ?? (a.topicKey ? undefined : (a.hook.topicId ?? undefined));
  const frame: { topicId?: string; title?: string; body: string; card?: Card; clientMsgId: string } = {
    body: a.body,
    clientMsgId: a.clientMsgId,
  };
  if (topicId) frame.topicId = topicId;
  else if (a.title) frame.title = a.title;
  if (a.card) frame.card = a.card;
  const result = unwrap(await send(env, a.hook, frame));
  if (a.topicKey && !mapped) {
    sql.exec(
      "INSERT OR IGNORE INTO hook_keys (key, topic_id, own_topic, updated_at) VALUES (?, ?, 1, ?)",
      `in:${a.topicKey}`,
      result.message.topicId,
      Date.now(),
    );
  }
  return result;
}

// ---------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------

interface Applied {
  status?: TopicStatus;
  waitingOn?: string[];
  ownerId?: string | null;
  title?: string;
}

async function deliverGithub(
  env: Env,
  sql: SqlStorage,
  log: Logger,
  a: Extract<DeliverArgs, { op: "github" }>,
): Promise<{ ok: true; topicId?: string; duplicate?: boolean; ignored?: boolean }> {
  const now = Date.now();
  if (a.deliveryId && sql.exec("SELECT 1 FROM hook_deliveries WHERE id = ?", a.deliveryId).toArray().length > 0) {
    return { ok: true, duplicate: true };
  }
  const e = a.event;

  let key: string | null;
  if (e.type === "ci") {
    if (e.prNumbers.length > 0) key = prKey(e.repo, e.prNumbers[0] as number);
    else {
      const bySha = sql.exec<{ key: string }>("SELECT key FROM hook_shas WHERE sha = ?", e.sha).toArray()[0];
      key = bySha?.key ?? (e.branch ? branchKey(e.repo, e.branch) : null);
    }
  } else {
    key = prKey(e.repo, e.number);
  }
  if (!key) return finish(sql, a.deliveryId, now, { ok: true, ignored: true });

  const row = keyRow(sql, key);
  const prev = row?.state ? (JSON.parse(row.state) as GhState) : null;
  const state = e.type === "ci" ? applyCi(prev, e, key) : applyEvent(prev, e);
  if (!state) return finish(sql, a.deliveryId, now, { ok: true, ignored: true });

  const card = buildCard(state);
  const cardJson = JSON.stringify(card);
  const hash = await shortHash(`${a.hook.id}:${key}`);
  let topicId = row?.topic_id ?? null;
  let messageId = row?.message_id ?? null;
  let ownTopic = row ? row.own_topic === 1 : true;
  let applied: Applied = row?.applied ? (JSON.parse(row.applied) as Applied) : {};

  if (!row) {
    // First event for this key: a new topic, or the topic named by `Topic: <id>` in the body.
    const linked = e.type === "pr" ? linkedTopicId(e.pr.body) : e.type === "issue" ? linkedTopicId(e.issue.body) : null;
    let result: SendResult | null = null;
    if (linked) {
      const r = await send(env, a.hook, { topicId: linked, body: "", card, clientMsgId: `gh-${hash}` });
      if (r.ok) {
        result = r.value;
        ownTopic = false;
      } else if (r.error.code !== "not_found") throw new ApiError(r.error.code, r.error.message);
      else log.warn("linked topic not found", { hookId: a.hook.id });
    }
    if (!result) {
      result = unwrap(
        await send(env, a.hook, { title: topicTitle(state), body: "", card, clientMsgId: `gh-${hash}` }),
      );
      ownTopic = true;
      // A new topic starts open, waiting on nobody, with this title.
      applied = { status: "open", waitingOn: [], title: result.topic?.title ?? topicTitle(state) };
    }
    topicId = result.message.topicId;
    messageId = result.message.id;
  } else if (row.card !== cardJson) {
    const r = await roomStub(env, a.hook.groupId).edit(a.hook.groupId, actorOf(a.hook), {
      t: "edit",
      rid: "hook",
      groupId: a.hook.groupId,
      messageId: messageId as string,
      body: "",
      card,
    });
    if (!r.ok) {
      // Someone deleted the card (conflict) or it is gone: post a new card in the same topic.
      if (r.error.code !== "conflict" && r.error.code !== "not_found") throw new ApiError(r.error.code, r.error.message);
      const sent = unwrap(
        await send(env, a.hook, { topicId: topicId as string, body: "", card, clientMsgId: `gh-${hash}-${ulid(now)}` }),
      );
      messageId = sent.message.id;
    }
  }

  // Topic status, waitingOn, owner and title follow the PR or issue. Only changes are sent, so a
  // status a person sets by hand stays until GitHub changes the computed value.
  applied = await syncTopic(env, a, topicId as string, state, ownTopic, applied);

  sql.exec(
    `INSERT INTO hook_keys (key, topic_id, message_id, own_topic, state, card, applied, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(key) DO UPDATE SET message_id = excluded.message_id, state = excluded.state,
       card = excluded.card, applied = excluded.applied, updated_at = excluded.updated_at`,
    key,
    topicId,
    messageId,
    ownTopic ? 1 : 0,
    JSON.stringify(state),
    cardJson,
    JSON.stringify(applied),
    now,
  );
  if (state.kind === "pr" && state.headSha) {
    sql.exec("INSERT OR REPLACE INTO hook_shas (sha, key, at) VALUES (?, ?, ?)", state.headSha, key, now);
    sql.exec("DELETE FROM hook_shas WHERE at < ?", now - SHA_TTL_MS);
  }
  return finish(sql, a.deliveryId, now, { ok: true, topicId: topicId as string });
}

function finish<T>(sql: SqlStorage, deliveryId: string | null, now: number, value: T): T {
  if (deliveryId) {
    sql.exec("INSERT OR IGNORE INTO hook_deliveries (id, at) VALUES (?, ?)", deliveryId, now);
    sql.exec("DELETE FROM hook_deliveries WHERE at < ?", now - DELIVERY_TTL_MS);
  }
  return value;
}

async function syncTopic(
  env: Env,
  a: Extract<DeliverArgs, { op: "github" }>,
  topicId: string,
  state: GhState,
  ownTopic: boolean,
  applied: Applied,
): Promise<Applied> {
  const want = desiredTopic(state);
  const title = ownTopic && state.kind !== "branch" ? topicTitle(state) : undefined;
  if (!want && title === undefined) return applied;

  const members = new Set(unwrap(await roomStub(env, a.hook.groupId).memberIds(a.hook.groupId)));
  const toMember = (login: string): string | null => {
    const id = a.logins[login.toLowerCase()];
    return id && members.has(id) ? id : null;
  };
  const update: { status?: TopicStatus; waitingOn?: string[]; ownerId?: string | null; title?: string } = {};
  if (want) {
    const waitingOn = [...new Set(want.waitingOn.map(toMember).filter((x): x is string => !!x))].sort();
    if (want.status !== applied.status) update.status = want.status;
    if (JSON.stringify(waitingOn) !== JSON.stringify(applied.waitingOn ?? [])) update.waitingOn = waitingOn;
    // The owner is the mapped author, set once (people may hand the topic to someone else).
    if (applied.ownerId === undefined && ownTopic && "author" in state && state.author) {
      const owner = toMember(state.author);
      if (owner) update.ownerId = owner;
    }
  }
  if (title !== undefined && title !== applied.title) update.title = title;
  if (Object.keys(update).length === 0) return applied;

  const r = await roomStub(env, a.hook.groupId).topicUpdate(a.hook.groupId, actorOf(a.hook), {
    t: "topic.update",
    rid: "hook",
    groupId: a.hook.groupId,
    topicId,
    ...update,
  });
  const topic: Topic = unwrap(r).topic;
  return {
    status: update.status ?? applied.status,
    waitingOn: update.waitingOn ?? applied.waitingOn,
    ownerId: update.ownerId !== undefined ? update.ownerId : applied.ownerId,
    title: update.title ?? applied.title ?? topic.title,
  };
}

async function shortHash(text: string): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  return [...d.slice(0, 16)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
