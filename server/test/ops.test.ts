// Operations: daily archive to R2, restore into an empty room, D1 dump and import, secret-free logs.
import {
  createExecutionContext,
  createScheduledController,
  env,
  runInDurableObject,
  waitOnExecutionContext,
} from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { roomStub } from "../src/core/env";
import { unwrap } from "../src/core/errors";
import { ulid } from "../src/core/ids";
import type { Actor } from "../src/core/registry";
import worker from "../src/index";
import { ARCHIVE_CRON } from "../src/ops";
import {
  type D1Dump,
  D1_PREFIX,
  GROUP_PREFIX,
  groupFileKey,
  groupStateKey,
  parseGroupFileKey,
  runArchive,
  utcDate,
} from "../src/ops/archive";
import { gunzip, gzip } from "../src/ops/gzip";
import { BASE, Client, api, clientMsgId, createGroup, createMember, type TestMember } from "./helpers";

const ADMIN = { "x-admin-secret": "test-admin-secret" };

function actor(m: TestMember, level: "owner" | "member" = "member"): Actor {
  return { id: m.id, level, kind: "human" };
}

/** Writes a realistic log: topics, replies, mentions, an edit, a delete, a status change. */
async function populate(gid: string, a: TestMember, b: TestMember, topics = 2): Promise<void> {
  const room = roomStub(env, gid);
  for (let t = 0; t < topics; t++) {
    const root = unwrap(
      await room.send(gid, actor(a), { t: "send", rid: "r", groupId: gid, clientMsgId: clientMsgId(), body: `topic ${t}\nbody` }),
    );
    const topicId = root.topic!.id;
    const reply = unwrap(
      await room.send(gid, actor(b), {
        t: "send",
        rid: "r",
        groupId: gid,
        topicId,
        clientMsgId: clientMsgId(),
        body: `reply to ${t}`,
        mentions: [a.id],
      }),
    );
    unwrap(await room.edit(gid, actor(b), { t: "edit", rid: "r", groupId: gid, messageId: reply.message.id, body: "edited" }));
    const gone = unwrap(
      await room.send(gid, actor(a), { t: "send", rid: "r", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "oops" }),
    );
    unwrap(await room.remove(gid, actor(a), gone.message.id));
    unwrap(
      await room.topicUpdate(gid, actor(a), { t: "topic.update", rid: "r", groupId: gid, topicId, status: "in_progress" }),
    );
  }
}

async function roomTables(gid: string) {
  const stub = env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid));
  return runInDurableObject(stub, (_obj, state) => {
    const q = (s: string) => state.storage.sql.exec(s).toArray();
    return {
      events: q("SELECT seq, type, at, actor_id, topic_id, message_id, data FROM events ORDER BY seq"),
      topics: q("SELECT * FROM topics ORDER BY id"),
      messages: q("SELECT * FROM messages ORDER BY id"),
      mentions: q("SELECT * FROM mentions ORDER BY member_id, seq, message_id"),
      topicStates: q("SELECT * FROM topic_states ORDER BY member_id, topic_id"),
      prefs: q("SELECT * FROM member_prefs ORDER BY member_id"),
    };
  });
}

/** Event files (not state snapshots) under a prefix, in seq order. */
async function archivedKeys(prefix: string): Promise<string[]> {
  const list = await env.FILES.list({ prefix });
  return list.objects
    .map((o) => o.key)
    .filter((k) => k.startsWith(D1_PREFIX) || parseGroupFileKey(k))
    .sort();
}

const eventFiles = (keys: string[], gid: string) => keys.filter((k) => parseGroupFileKey(k)?.groupId === gid);

async function readArchive(key: string): Promise<any[]> {
  const obj = await env.FILES.get(key);
  const text = await gunzip(await obj!.arrayBuffer());
  return text
    .split("\n")
    .filter((l) => l !== "")
    .map((l) => JSON.parse(l));
}

/** A group that exists in D1 with the same members, whose room was never used: a fresh restore target. */
async function emptyGroupLike(memberIds: string[]): Promise<string> {
  const id = ulid();
  const now = Date.now();
  await env.DB.batch([
    env.DB.prepare("INSERT INTO groups (id, kind, name, created_by, created_at) VALUES (?, 'group', 'restored', ?, ?)").bind(
      id,
      memberIds[0],
      now,
    ),
    ...memberIds.map((m) =>
      env.DB.prepare("INSERT INTO group_members (group_id, member_id, added_at) VALUES (?, ?, ?)").bind(id, m, now),
    ),
  ]);
  return id;
}

async function importChunk(gid: string, body: Uint8Array | string, headers: Record<string, string> = ADMIN) {
  return api(`/v1/admin/import/groups/${gid}`, { method: "POST", headers, raw: body as unknown as string });
}

describe("daily archive", () => {
  it("writes every new event once as gzip NDJSON and records the last seq", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b);
    const before = await roomTables(gid);

    const first = await runArchive(env, { skipD1: true });
    expect(first.complete).toBe(true);
    const keys = eventFiles(first.files, gid);
    expect(first.files).toContain(groupStateKey(gid, utcDate(Date.now())));
    expect(keys).toEqual([groupFileKey(gid, 1, before.events.length)]);
    const lines = await readArchive(keys[0]);
    expect(lines.map((l) => l.seq)).toEqual(before.events.map((e: any) => e.seq));
    expect(lines[1]).toMatchObject({ type: "message.created", topicId: expect.any(String), messageId: expect.any(String) });
    const state = await env.DB.prepare("SELECT last_seq FROM ops_archive WHERE group_id = ?").bind(gid).first<{ last_seq: number }>();
    expect(state?.last_seq).toBe(before.events.length);

    // Idempotent: nothing new, nothing written.
    const second = await runArchive(env, { skipD1: true });
    expect(eventFiles(second.files, gid)).toEqual([]);

    // Only the new events go into the next file.
    await populate(gid, a, b, 1);
    const third = await runArchive(env, { skipD1: true });
    const next = eventFiles(third.files, gid);
    expect(next).toHaveLength(1);
    expect(parseGroupFileKey(next[0])).toEqual({ groupId: gid, from: before.events.length + 1, to: expect.any(Number) });
  });

  it("stops at its budget and resumes where it stopped", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const g1 = await createGroup(a, [b.id]);
    await populate(g1, a, b, 3);
    await runArchive(env, { skipD1: true }); // archive everything older first
    const g2 = await createGroup(a, [b.id]);
    const g3 = await createGroup(a, [b.id]);
    await populate(g2, a, b, 1);
    await populate(g3, a, b, 1);
    const partial = await runArchive(env, { skipD1: true, maxEvents: 1 });
    expect(partial.complete).toBe(false);
    const rest = await runArchive(env, { skipD1: true });
    expect(rest.complete).toBe(true);
    for (const g of [g2, g3]) {
      const keys = await archivedKeys(`${GROUP_PREFIX}${g}/`);
      expect(keys).toHaveLength(1);
      const seqs = (await roomTables(g)).events.map((e: any) => e.seq);
      expect((await readArchive(keys[0])).map((e) => e.seq)).toEqual(seqs);
    }
  });

  it("dumps D1 with hashes only and no archive state", async () => {
    const m = await createMember({ level: "owner" });
    const invite = await api("/v1/invites", { token: m.token, body: { name: "new", kind: "human", level: "member" } });
    expect(invite.status).toBe(200);
    const now = Date.UTC(2026, 10, 3, 3, 0, 0);
    const key = (await runArchive(env, { now })).d1Key;
    expect(key).toBe(`${D1_PREFIX}${utcDate(now)}.json.gz`);
    const text = await gunzip(await (await env.FILES.get(key!))!.arrayBuffer());
    expect(text).not.toContain(m.token);
    expect(text).not.toContain(invite.body.code);
    const dump = JSON.parse(text) as D1Dump;
    const names = dump.tables.map((t) => t.name);
    expect(names.slice(0, 3)).toEqual(["members", "tokens", "invites"]);
    expect(names).not.toContain("ops_archive");
    expect(names).not.toContain("d1_migrations");
    const tokens = dump.tables.find((t) => t.name === "tokens")!;
    expect(tokens.columns).toContain("token_hash");
    expect(tokens.rows.some((r) => r.includes(m.tokenId))).toBe(true);
  });

  it("runs from the cron trigger", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 1);
    const ctx = createExecutionContext();
    await worker.scheduled(createScheduledController({ cron: ARCHIVE_CRON, scheduledTime: Date.now() }), env, ctx);
    await waitOnExecutionContext(ctx);
    expect(await archivedKeys(`${GROUP_PREFIX}${gid}/`)).toHaveLength(1);
    expect(await archivedKeys(`${D1_PREFIX}${utcDate(Date.now())}`)).toHaveLength(1);
  });
});

describe("restore", () => {
  it("rebuilds an empty room from the archive, identical to the original", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 2);
    await runArchive(env, { skipD1: true });
    await populate(gid, a, b, 1);
    // State outside the log: a mute preference and a read position.
    await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid)), (_o, state) => {
      state.storage.sql.exec("INSERT INTO member_prefs (member_id, muted, notify) VALUES (?, 1, 'none')", b.id);
      state.storage.sql.exec("UPDATE topic_states SET read_seq = 5 WHERE member_id = ?", b.id);
    });
    await runArchive(env, { skipD1: true });
    const original = await roomTables(gid);
    expect(original.prefs).toHaveLength(1);
    const keys = await archivedKeys(`${GROUP_PREFIX}${gid}/`);
    expect(keys).toHaveLength(2);

    const target = await emptyGroupLike([a.id, b.id]);
    let total = 0;
    for (const key of keys) {
      const bytes = new Uint8Array(await (await env.FILES.get(key))!.arrayBuffer());
      const r = await importChunk(target, bytes);
      expect(r.status, JSON.stringify(r.body)).toBe(200);
      total += r.body.imported;
    }
    expect(total).toBe(original.events.length);
    const stateBytes = new Uint8Array(await (await env.FILES.get(groupStateKey(gid, utcDate(Date.now()))))!.arrayBuffer());
    const st = await api(`/v1/admin/import/groups/${target}/state`, { method: "POST", headers: ADMIN, raw: stateBytes as unknown as string });
    expect(st.status, JSON.stringify(st.body)).toBe(200);
    const restored = await roomTables(target);
    expect(restored).toEqual(original);

    // Re-running a chunk is a no-op.
    const again = await importChunk(target, new Uint8Array(await (await env.FILES.get(keys[0]))!.arrayBuffer()));
    expect(again.body).toMatchObject({ imported: 0, skipped: expect.any(Number) });

    // Members can read the restored group and keep writing after the last seq.
    const { c, welcome } = await Client.connect(a.token);
    const summary = welcome.groups.find((g: any) => g.group.id === target);
    expect(summary.lastSeq).toBe(original.events.length);
    const topicId = original.topics[0].id as string;
    const history = await c.request({ t: "history", groupId: target, topicId });
    expect(history.messages.length).toBe(3);
    const sent = await c.ok({ t: "send", groupId: target, topicId, clientMsgId: clientMsgId(), body: "after restore" });
    expect(sent.message.createdSeq).toBe(original.events.length + 1);
    c.close();

    // Once the room has live events, import is refused.
    const late = await importChunk(target, await gzip('{"seq":999,"type":"x","at":1,"actorId":"a","data":{}}\n'));
    expect(late.status).toBe(409);
  });

  it("refuses gaps, bad input and missing admin secret", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const target = await emptyGroupLike([a.id, b.id]);
    const gap = await importChunk(target, '{"seq":2,"type":"group.updated","at":1,"actorId":"a","data":{"name":"x"}}\n');
    expect(gap.status).toBe(400);
    expect(gap.body.error.message).toContain("gap");
    const bad = await importChunk(target, "not json\n");
    expect(bad.status).toBe(400);
    const noSecret = await importChunk(target, "", { "x-admin-secret": "wrong" });
    expect(noSecret.status).toBe(401);
    const missing = await importChunk(ulid(), '{"seq":1,"type":"group.updated","at":1,"actorId":"a","data":{"name":"x"}}\n');
    expect(missing.status).toBe(404);
    // A failed chunk leaves the room empty.
    expect((await roomTables(target)).events).toEqual([]);
  });

  it("serves archive listings and objects to the admin only", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 1);
    await runArchive(env, { skipD1: true });
    const list = await api(`/v1/admin/archive?prefix=${encodeURIComponent(`${GROUP_PREFIX}${gid}/`)}`, { headers: ADMIN });
    expect(list.status).toBe(200);
    const keys = list.body.objects.map((o: any) => o.key);
    const n = (await roomTables(gid)).events.length;
    expect(keys).toEqual([groupFileKey(gid, 1, n), groupStateKey(gid, utcDate(Date.now()))]);
    const res = await fetchObject(keys[0]);
    expect(res.status).toBe(200);
    expect((await gunzip(await res.arrayBuffer())).split("\n").filter(Boolean)).toHaveLength(n);
    expect((await api("/v1/admin/archive", {})).status).toBe(401);
    expect((await api("/v1/admin/archive?prefix=uploads/", { headers: ADMIN })).status).toBe(400);
  });

  it("imports D1 rows idempotently and only into known columns", async () => {
    const status = await api("/v1/admin/import/d1", { headers: ADMIN });
    expect(status.status).toBe(200);
    expect(status.body.tables).toHaveProperty("members");
    const id = ulid();
    const chunk = { table: "members", columns: ["id", "kind", "name", "level", "created_at"], rows: [[id, "human", "R", "member", 1]] };
    const one = await api("/v1/admin/import/d1", { headers: ADMIN, body: chunk });
    expect(one.body).toMatchObject({ rows: 1, inserted: 1 });
    const two = await api("/v1/admin/import/d1", { headers: ADMIN, body: chunk });
    expect(two.body).toMatchObject({ rows: 1, inserted: 0 });
    const badCol = await api("/v1/admin/import/d1", { headers: ADMIN, body: { ...chunk, columns: ["id", "evil\" TEXT"] } });
    expect(badCol.status).toBe(400);
    const badTable = await api("/v1/admin/import/d1", { headers: ADMIN, body: { ...chunk, table: "ops_archive" } });
    expect(badTable.status).toBe(400);
    expect((await api("/v1/admin/import/d1", { body: chunk })).status).toBe(401);
  });
});

async function fetchObject(key: string): Promise<Response> {
  const { SELF } = await import("cloudflare:test");
  return SELF.fetch(`${BASE}/v1/admin/archive/object?key=${encodeURIComponent(key)}`, { headers: ADMIN });
}

describe("logs carry no secrets", () => {
  const lines: string[] = [];
  beforeEach(() => {
    lines.length = 0;
    for (const level of ["log", "info", "warn", "error", "debug"] as const) {
      vi.spyOn(console, level).mockImplementation((...args: unknown[]) => {
        lines.push(args.map((x) => (typeof x === "string" ? x : JSON.stringify(x))).join(" "));
      });
    }
  });
  afterEach(() => vi.restoreAllMocks());

  it("never prints tokens, invite codes, the admin secret or message bodies", async () => {
    const owner = await createMember({ level: "owner" });
    const invite = await api("/v1/invites", { token: owner.token, body: { name: "intern", kind: "human", level: "member" } });
    const redeem = await api("/v1/auth/redeem", { body: { inviteCode: invite.body.code, deviceName: "mac" } });
    expect(redeem.status).toBe(200);
    const bot = await api("/v1/bots", { token: owner.token, body: { name: "bot" } });
    // Failures that log: bad tokens, bad admin secret, malformed bodies.
    await api("/v1/me", { token: `${owner.token}x` });
    await api("/v1/me", { headers: { authorization: `Bearer ${owner.token}` } });
    await api("/v1/admin/archive/run", { method: "POST", headers: { "x-admin-secret": "guess-the-secret" } });
    await api("/v1/auth/redeem", { body: { inviteCode: invite.body.code, deviceName: "again" } });
    const gid = await createGroup(owner, [redeem.body.member.id]);
    const { c } = await Client.connect(redeem.body.token);
    await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: `secret plan ${owner.token}` });
    c.send("{not json");
    c.close();
    await runArchive(env, { log: (await import("../src/core/logger")).logger });
    await new Promise((r) => setTimeout(r, 200));

    const all = lines.join("\n");
    expect(lines.length).toBeGreaterThan(0);
    for (const secret of [owner.token, redeem.body.token, bot.body.token, invite.body.code, "test-admin-secret", "guess-the-secret"]) {
      expect(all).not.toContain(secret);
    }
    expect(all).not.toMatch(/tc_[A-Za-z0-9_-]{20,}/);
    expect(all).not.toMatch(/\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b/);
    expect(all).not.toMatch(/Bearer\s+(?!\[redacted\])\S+/);
    expect(all).not.toContain("secret plan");
  });
});

describe("point-in-time recovery", () => {
  it("returns a bookmark without applying, and refuses times outside 30 days", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 1);
    const dry = await api(`/v1/admin/pitr/groups/${gid}`, { headers: ADMIN, body: { at: Date.now() - 1000 } });
    if (dry.status === 200) {
      expect(dry.body).toMatchObject({ applied: false, bookmark: expect.any(String), lastSeq: expect.any(Number) });
    } else {
      // The local runtime has no point-in-time recovery; Cloudflare does (checked in the runbook drill).
      expect(dry.body.error.message).toContain("not available");
    }
    const old = await api(`/v1/admin/pitr/groups/${gid}`, { headers: ADMIN, body: { at: Date.now() - 31 * 24 * 3600 * 1000 } });
    expect(old.status).toBe(400);
    expect((await api(`/v1/admin/pitr/groups/${gid}`, { body: { at: Date.now() } })).status).toBe(401);
  });
});
