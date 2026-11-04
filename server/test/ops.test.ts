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
      pageEvents: q("SELECT * FROM page_events ORDER BY event_id"),
      joined: q("SELECT member_id, joined_seq FROM room_members ORDER BY member_id"),
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

/** Adds a member through the HTTP route; returns the seq of its group.memberAdded event. */
async function addMember(gid: string, by: TestMember, memberId: string): Promise<number> {
  const r = await api(`/v1/groups/${gid}/members`, { method: "POST", token: by.token, body: { memberId } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const { events } = await roomTables(gid);
  const added = events.filter((e: any) => e.type === "group.memberAdded" && JSON.parse(e.data).memberId === memberId);
  return added.at(-1)!.seq as number;
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
    const [a, b, joiner] = await Promise.all([createMember(), createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 2);
    // A member added later: the history before its join counts as read, also after a restore.
    const joinedAt = await addMember(gid, a, joiner.id);
    await runArchive(env, { skipD1: true });
    await populate(gid, a, b, 1);
    // State outside the log: a mute preference and a read position.
    await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid)), (_o, state) => {
      state.storage.sql.exec("INSERT INTO member_prefs (member_id, muted, notify) VALUES (?, 1, 'none')", b.id);
      state.storage.sql.exec("UPDATE topic_states SET read_seq = 5 WHERE member_id = ?", b.id);
      // The 产出 page log (GAPS SF-2) is room state too.
      state.storage.sql.exec(
        "INSERT INTO page_events (event_id, artifact_id, version, actor_id, bot_id, kind, ack, at, name, payload) VALUES ('01PAGEEVENT0000000000000001', 'art', 1, ?, ?, 'emit', '{}', 1, 'pick', '{\"c\":1}')",
        b.id,
        a.id,
      );
    });
    await runArchive(env, { skipD1: true });
    const original = await roomTables(gid);
    expect(original.prefs).toHaveLength(1);
    expect(original.pageEvents).toHaveLength(1);
    expect(original.joined).toContainEqual({ member_id: joiner.id, joined_seq: joinedAt });
    const keys = await archivedKeys(`${GROUP_PREFIX}${gid}/`);
    expect(keys).toHaveLength(2);

    const target = await emptyGroupLike([a.id, b.id, joiner.id]);
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

async function sha256(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function uploadFile(m: TestMember, groupId: string, text: string, name: string, mime: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: await sha256(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const { SELF } = await import("cloudflare:test");
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, {
    method: "PUT",
    headers: { authorization: `Bearer ${m.token}` },
    body: bytes,
  });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

async function restoreInto(target: string, keys: string[]): Promise<void> {
  for (const key of keys) {
    const bytes = new Uint8Array(await (await env.FILES.get(key))!.arrayBuffer());
    const r = await importChunk(target, bytes);
    expect(r.status, JSON.stringify(r.body)).toBe(200);
  }
}

describe("deleted messages and restore", () => {
  it("scrubs a message deleted after it was archived, in R2 and in a restored room", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    const room = roomStub(env, gid);
    const root = unwrap(await room.send(gid, actor(a), { t: "send", rid: "r", groupId: gid, clientMsgId: clientMsgId(), body: "话题" }));
    const topicId = root.topic!.id;
    const secret = unwrap(
      await room.send(gid, actor(a), { t: "send", rid: "r", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "SECRET-PASSWORD 1" }),
    );
    unwrap(await room.edit(gid, actor(a), { t: "edit", rid: "r", groupId: gid, messageId: secret.message.id, body: "SECRET-PASSWORD 2" }));
    await runArchive(env, { skipD1: true });
    const [firstKey] = eventFiles(await archivedKeys(`${GROUP_PREFIX}${gid}/`), gid);
    // A copy of the file as it was before the delete (e.g. fetched to a folder earlier).
    const stale = new Uint8Array(await (await env.FILES.get(firstKey))!.arrayBuffer());
    expect(await gunzip(stale)).toContain("SECRET-PASSWORD");

    unwrap(await room.remove(gid, actor(a), secret.message.id));
    const run = await runArchive(env, { skipD1: true });
    expect(run.scrubbed).toEqual([firstKey]);
    const keys = eventFiles(await archivedKeys(`${GROUP_PREFIX}${gid}/`), gid);
    expect(keys).toHaveLength(2);
    for (const k of keys) expect(JSON.stringify(await readArchive(k))).not.toContain("SECRET-PASSWORD");
    // Nothing more to scrub next time.
    expect((await runArchive(env, { skipD1: true })).scrubbed).toEqual([]);

    // Restoring from the stale copy still drops the deleted text from the log and the search index.
    const target = await emptyGroupLike([a.id, b.id]);
    expect((await importChunk(target, stale)).status).toBe(200);
    await restoreInto(target, keys.slice(1));
    const restored = await roomTables(target);
    expect(JSON.stringify(restored.events)).not.toContain("SECRET-PASSWORD");
    expect(restored.events).toEqual((await roomTables(gid)).events);
    const { c } = await Client.connect(a.token);
    const found = await c.request({ t: "search", query: "SECRET", groupId: target });
    expect(found.t, JSON.stringify(found)).toBe("searchResults");
    expect(found.hits).toEqual([]);
    c.close();
  });
});

describe("产出 and files after restore", () => {
  it("rebuilds 产出, versions and ready-file records", async () => {
    const owner = await createMember({ level: "owner" });
    const b = await createMember();
    const gid = await createGroup(owner, [b.id]);
    const room = roomStub(env, gid);
    const root = unwrap(await room.send(gid, actor(owner), { t: "send", rid: "r", groupId: gid, clientMsgId: clientMsgId(), body: "页面" }));
    const topicId = root.topic!.id;
    const page = await uploadFile(owner, gid, "<h1>v1</h1>", "index.html", "text/html");
    const page2 = await uploadFile(owner, gid, "<h1>v2</h1>", "index.html", "text/html");
    const note = await uploadFile(owner, gid, "附件", "a.txt", "text/plain");
    const created = await api("/v1/artifacts", {
      token: owner.token,
      body: { groupId: gid, topicId, title: "页面", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: page }] },
    });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    const artifactId = created.body.artifact.id as string;
    const v2 = await api(`/v1/artifacts/${artifactId}/versions`, {
      token: owner.token,
      body: { entry: "index.html", files: [{ path: "index.html", fileId: page2 }], note: "第二版", title: "页面 2" },
    });
    expect(v2.status, JSON.stringify(v2.body)).toBe(200);
    await runArchive(env, { skipD1: true });

    const original = await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid)), (_o, state) => ({
      artifacts: state.storage.sql.exec("SELECT * FROM artifacts ORDER BY id").toArray(),
      versions: state.storage.sql.exec("SELECT * FROM artifact_versions ORDER BY artifact_id, version").toArray(),
      files: state.storage.sql.exec("SELECT * FROM file_records ORDER BY id").toArray(),
    }));
    expect(original.artifacts).toHaveLength(1);
    expect(original.versions).toHaveLength(2);
    expect(original.files).toHaveLength(3);

    // Restore into an empty room; D1 (restored first in a real restore) maps the files to it.
    const target = await emptyGroupLike([owner.id, b.id]);
    await env.DB.prepare("UPDATE files SET group_id = ? WHERE group_id = ?").bind(target, gid).run();
    await restoreInto(target, eventFiles(await archivedKeys(`${GROUP_PREFIX}${gid}/`), gid));
    const filesRoute = await api(`/v1/admin/import/groups/${target}/files`, { method: "POST", headers: ADMIN });
    expect(filesRoute.status, JSON.stringify(filesRoute.body)).toBe(200);
    expect(filesRoute.body.rows).toBe(3);
    expect((await api(`/v1/admin/import/groups/${target}/files`, { method: "POST" })).status).toBe(401);

    const restored = await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(target)), (_o, state) => ({
      artifacts: state.storage.sql.exec("SELECT * FROM artifacts ORDER BY id").toArray(),
      versions: state.storage.sql.exec("SELECT * FROM artifact_versions ORDER BY artifact_id, version").toArray(),
      files: state.storage.sql.exec("SELECT * FROM file_records ORDER BY id").toArray(),
    }));
    expect(restored).toEqual(original);

    // The restored room serves the 产出 and accepts the old files as attachments and versions.
    const listed = unwrap(await roomStub(env, target).rpc(target, "artifacts.list", actor(owner), {})) as any[];
    expect(listed.map((x) => [x.id, x.currentVersion, x.title])).toEqual([[artifactId, 2, "页面 2"]]);
    const sent = unwrap(
      await roomStub(env, target).send(target, actor(owner), {
        t: "send",
        rid: "r",
        groupId: target,
        topicId,
        clientMsgId: clientMsgId(),
        body: "旧附件",
        attachments: [{ fileId: note, name: "a.txt", size: 6, mime: "text/plain", sha256: "x" }],
        artifactRefs: [{ artifactId, version: 1 }],
      } as any),
    );
    expect(sent.message.createdSeq).toBeGreaterThan(0);
    const v3 = unwrap(
      await roomStub(env, target).rpc(target, "artifacts.addVersion", actor(owner), {
        artifactId,
        entry: "index.html",
        files: [{ path: "index.html", fileId: page }],
      }),
    ) as any;
    expect(v3.version.version).toBe(3);
  });

  // GAPS SF-6: restore --from-group gives the copy its own file and 产出 rows and R2 objects, so
  // its attachments and 产出 open for the target group's members.
  it("copies files and 产出 into a --from-group copy under new ids, idempotently (copy-files)", async () => {
    const owner = await createMember({ level: "owner" });
    const b = await createMember();
    const gid = await createGroup(owner, [b.id]);
    const room = roomStub(env, gid);
    const note = await uploadFile(owner, gid, "附件内容", "a.txt", "text/plain");
    const root = unwrap(
      await room.send(gid, actor(owner), {
        t: "send",
        rid: "r",
        groupId: gid,
        clientMsgId: clientMsgId(),
        body: "带附件",
        attachments: [{ fileId: note, name: "a.txt", size: 12, mime: "text/plain", sha256: "x" }],
      } as any),
    );
    const topicId = root.topic!.id;
    const page = await uploadFile(owner, gid, "<h1>v1</h1>", "index.html", "text/html");
    const page2 = await uploadFile(owner, gid, "<h1>v2</h1>", "index.html", "text/html");
    const created = await api("/v1/artifacts", {
      token: owner.token,
      body: { groupId: gid, topicId, title: "页面", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: page }] },
    });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    const artifactId = created.body.artifact.id as string;
    const v2 = await api(`/v1/artifacts/${artifactId}/versions`, {
      token: owner.token,
      body: { entry: "index.html", files: [{ path: "index.html", fileId: page2 }] },
    });
    expect(v2.status, JSON.stringify(v2.body)).toBe(200);
    await runArchive(env, { skipD1: true });

    // The copy's id map (restore.ts copyIdMap does the same with stable ids).
    const target = await emptyGroupLike([owner.id, b.id]);
    const map = new Map<string, string>([[gid, target]]);
    for (const id of [topicId, root.message.id, artifactId, note, page, page2]) map.set(id, ulid());
    const remap = (v: unknown): unknown =>
      typeof v === "string"
        ? (map.get(v) ?? v)
        : Array.isArray(v)
          ? v.map(remap)
          : v && typeof v === "object"
            ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, remap(x)]))
            : v;
    for (const key of eventFiles(await archivedKeys(`${GROUP_PREFIX}${gid}/`), gid)) {
      const text = await gunzip(await (await env.FILES.get(key))!.arrayBuffer());
      const lines = text.split("\n").map((l) => (l.trim() === "" ? l : JSON.stringify(remap(JSON.parse(l)))));
      const r = await importChunk(target, await gzip(lines.join("\n")));
      expect(r.status, JSON.stringify(r.body)).toBe(200);
    }
    const pair = (id: string): [string, string] => [id, map.get(id) as string];
    const lost = ulid();
    const body = {
      fromGroup: gid,
      files: [pair(note), pair(page), pair(page2), [lost, ulid()]],
      artifacts: [pair(artifactId)],
      topics: [pair(topicId)],
    };
    const path = `/v1/admin/import/groups/${target}/copy-files`;
    expect((await api(path, { body })).status).toBe(401);
    expect((await api(path, { headers: { "x-admin-secret": "wrong" }, body })).status).toBe(401);
    expect((await api(`/v1/admin/import/groups/${gid}/copy-files`, { headers: ADMIN, body })).status).toBe(400);
    expect((await api(`/v1/admin/import/groups/${ulid()}/copy-files`, { headers: ADMIN, body })).status).toBe(404);
    expect((await api(path, { headers: ADMIN, body: { ...body, files: Array.from({ length: 51 }, () => [ulid(), ulid()]) } })).status).toBe(400);
    const copied = await api(path, { headers: ADMIN, body });
    expect(copied.status, JSON.stringify(copied.body)).toBe(200);
    expect(copied.body).toEqual({ groupId: target, files: 3, artifacts: 1, bytes: 12 + 11 + 11, missing: 1 });
    // A re-run keeps the rows and copies no bytes again.
    const again = await api(path, { headers: ADMIN, body });
    expect(again.body).toEqual({ groupId: target, files: 3, artifacts: 1, bytes: 0, missing: 1 });
    const filesRoute = await api(`/v1/admin/import/groups/${target}/files`, { method: "POST", headers: ADMIN });
    expect(filesRoute.status, JSON.stringify(filesRoute.body)).toBe(200);
    expect(filesRoute.body.rows).toBe(3);

    // D1 rows of the copy: the target group, the copy's topic, the same name and hash.
    const row = await env.DB.prepare("SELECT group_id, topic_id, name, sha256, status FROM files WHERE id = ?").bind(map.get(note)).first<any>();
    const src = await env.DB.prepare("SELECT topic_id, sha256 FROM files WHERE id = ?").bind(note).first<any>();
    expect(row).toEqual({ group_id: target, topic_id: src.topic_id ? map.get(src.topic_id) : null, name: "a.txt", sha256: src.sha256, status: "ready" });
    const art = await env.DB.prepare("SELECT group_id, topic_id FROM artifacts WHERE id = ?").bind(map.get(artifactId)).first<any>();
    expect(art).toEqual({ group_id: target, topic_id: map.get(topicId) });

    // A member of the copy downloads the attachment and opens both versions of the 产出.
    const file = await api(`/v1/files/${map.get(note)}/content`, { token: b.token });
    expect(file.status).toBe(200);
    expect(file.body).toBe("附件内容");
    const got = await api(`/v1/artifacts/${map.get(artifactId)}`, { token: b.token });
    expect(got.status, JSON.stringify(got.body)).toBe(200);
    expect(got.body.artifact).toMatchObject({ id: map.get(artifactId), groupId: target, topicId: map.get(topicId), currentVersion: 2 });
    for (const [v, html] of [[1, "<h1>v1</h1>"], [2, "<h1>v2</h1>"]] as const) {
      const f = await api(`/v1/artifacts/${map.get(artifactId)}/versions/${v}/files/index.html`, { token: b.token });
      expect(f.status).toBe(200);
      expect(f.body).toBe(html);
    }
    // The source keeps its own objects; a member of neither group gets nothing.
    expect((await api(`/v1/files/${note}/content`, { token: b.token })).body).toBe("附件内容");
    const stranger = await createMember();
    expect([403, 404]).toContain((await api(`/v1/files/${map.get(note)}/content`, { token: stranger.token })).status);
    expect([403, 404]).toContain((await api(`/v1/artifacts/${map.get(artifactId)}`, { token: stranger.token })).status);
    const audited = await env.DB.prepare("SELECT details FROM audit WHERE action = 'admin.import.copyFiles' AND target = ?").bind(target).all<any>();
    expect(audited.results).toHaveLength(2);
  });
});

describe("archive runs", () => {
  it("resumes a stopped pass after the last finished group and keeps going past a failing group", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    await runArchive(env, { skipD1: true }); // older groups first
    const ids: string[] = [];
    for (let i = 0; i < 3; i++) {
      const gid = await createGroup(a, [b.id]);
      await populate(gid, a, b, 1);
      ids.push(gid);
    }
    // Budget spent: every run finishes one group, and the next run goes on after it.
    const seen: string[] = [];
    let runs = 0;
    for (; runs < 500; runs++) {
      const r = await runArchive(env, { skipD1: true, budgetMs: -1 });
      expect(r.groups).toBe(1);
      seen.push(...r.files.map((k) => parseGroupFileKey(k)?.groupId).filter((x): x is string => !!x));
      if (r.complete) break;
    }
    expect(runs).toBeLessThan(500);
    for (const g of ids) expect(seen).toContain(g);
    const status = await api("/v1/ops/archive", { token: a.token });
    expect(status.status).toBe(200);
    expect(status.body.lastCompleteAt).toEqual(expect.any(Number));
    expect((await api("/v1/ops/archive")).status).toBe(401);

    // A group with no new events and today's state: no state rewrite.
    const quiet = await runArchive(env, { skipD1: true });
    expect(quiet.files.filter((k) => ids.some((g) => k.includes(g)))).toEqual([]);

    // One group fails (its state export throws): the later groups are still archived.
    for (const g of ids) await populate(g, a, b, 1);
    const failing = ids[0];
    const realPut = env.FILES.put.bind(env.FILES);
    const spy = vi.spyOn(env.FILES, "put");
    spy.mockImplementation(((key: string, ...rest: any[]) => {
      if (key.includes(failing)) return Promise.reject(new Error("r2 down"));
      return (realPut as any)(key, ...rest);
    }) as any);
    const before = (await api("/v1/ops/archive", { token: a.token })).body.lastCompleteAt;
    const r = await runArchive(env, { skipD1: true });
    spy.mockRestore();
    expect(r.failed).toEqual([failing]);
    expect(r.complete).toBe(true);
    for (const g of ids.slice(1)) expect(r.files.some((k) => k.includes(g))).toBe(true);
    const after = (await api("/v1/ops/archive", { token: a.token })).body;
    expect(after.lastCompleteAt).toBe(before);
    expect(after.lastRun).toMatchObject({ complete: true, failed: 1 });
    // The next run archives the failed group from its last seq.
    const retry = await runArchive(env, { skipD1: true });
    expect(retry.failed).toEqual([]);
    expect(retry.files.some((k) => parseGroupFileKey(k)?.groupId === failing)).toBe(true);
  });

  it("moves archive files above a point-in-time restore point aside and archives the new events", async () => {
    const [a, b, joiner] = await Promise.all([createMember(), createMember(), createMember()]);
    const gid = await createGroup(a, [b.id]);
    await populate(gid, a, b, 2);
    // A member added later: the history before its join counts as read, also after a restore.
    const joinedAt = await addMember(gid, a, joiner.id);
    await runArchive(env, { skipD1: true });
    const n = (await roomTables(gid)).events.length;
    const { supersedeAfter } = await import("../src/ops/archive");
    const keep = 3;
    const moved = await supersedeAfter(env, gid, keep, 1234);
    expect(moved).toEqual([groupFileKey(gid, 1, n)]);
    const keys = eventFiles(await archivedKeys(`${GROUP_PREFIX}${gid}/`), gid);
    expect(keys).toEqual([groupFileKey(gid, 1, keep)]);
    expect((await readArchive(keys[0])).map((e) => e.seq)).toEqual([1, 2, 3]);
    expect(await env.FILES.head(`${GROUP_PREFIX}${gid}/superseded/1234/${groupFileKey(gid, 1, n).split("/").pop()}`)).not.toBeNull();
    const row = await env.DB.prepare("SELECT last_seq FROM ops_archive WHERE group_id = ?").bind(gid).first<{ last_seq: number }>();
    expect(row?.last_seq).toBe(keep);
    // The next run archives from the restore point again.
    const r = await runArchive(env, { skipD1: true });
    expect(eventFiles(r.files, gid)).toEqual([groupFileKey(gid, keep + 1, n)]);
  });
});

describe("revocations", () => {
  it("lists revocations and applies them again after a D1 restore brings a token back", async () => {
    const owner = await createMember({ level: "owner" });
    const leaked = await createMember();
    const gone = await createMember();
    const rev = await api("/v1/tokens/revoke", { token: owner.token, body: { memberId: leaked.id, all: true } });
    expect(rev.status, JSON.stringify(rev.body)).toBe(200);
    expect((await api("/v1/me", { token: leaked.token })).status).toBe(401);
    await env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(gone.id).run();
    const saved = await api("/v1/admin/revocations", { headers: ADMIN });
    expect(saved.status).toBe(200);
    expect(saved.body.tokens.map((t: any) => t.id)).toContain(leaked.tokenId);
    expect(saved.body.members).toContain(gone.id);
    expect((await api("/v1/admin/revocations", {})).status).toBe(401);

    // A restore to a point before the revoke: the token works again.
    await env.DB.prepare("UPDATE tokens SET revoked_at = NULL WHERE id = ?").bind(leaked.tokenId).run();
    await env.DB.prepare("UPDATE members SET deactivated = 0 WHERE id = ?").bind(gone.id).run();
    expect((await api("/v1/me", { token: leaked.token })).status).toBe(200);
    const applied = await api("/v1/admin/revocations", { headers: ADMIN, body: saved.body });
    expect(applied.status, JSON.stringify(applied.body)).toBe(200);
    expect(applied.body.applied).toBe(2);
    expect((await api("/v1/me", { token: leaked.token })).status).toBe(401);
    expect((await api("/v1/me", { token: gone.token })).status).toBe(401);
    // Idempotent.
    expect((await api("/v1/admin/revocations", { headers: ADMIN, body: saved.body })).body.applied).toBe(0);
  });
});

describe("member gateway state", () => {
  it("archives read cursors and the hook topic map once a day and merges them back", async () => {
    const [a, b] = await Promise.all([createMember(), createMember()]);
    const gw = (id: string) => env.MEMBER_GATEWAY.get(env.MEMBER_GATEWAY.idFromName(id));
    await runInDurableObject(gw(a.id), (_o, state) => {
      state.storage.sql.exec("INSERT INTO cursors (group_id, seq, updated_at) VALUES ('g1', 42, 1), ('g2', 7, 1)");
      state.storage.sql.exec(
        "INSERT INTO hook_keys (key, topic_id, message_id, own_topic, updated_at) VALUES ('pr:o/r#1', 't1', 'm1', 1, 1)",
      );
      state.storage.sql.exec("INSERT INTO hook_deliveries (id, at) VALUES ('d1', 1)");
    });
    const day = Date.UTC(2030, 0, 1, 3);
    const run = await runArchive(env, { skipD1: true, now: day });
    const { memberStateKey } = await import("../src/ops/archive");
    const key = memberStateKey(a.id, "2030-01-01");
    expect(run.files).toContain(key);
    // Once a day: a second run that day writes no member state.
    expect((await runArchive(env, { skipD1: true, now: day })).files.some((k) => k.startsWith("archive/members/"))).toBe(false);

    // Restore into b's gateway, which already has a newer cursor for g2.
    await runInDurableObject(gw(b.id), (_o, state) => {
      state.storage.sql.exec("INSERT INTO cursors (group_id, seq, updated_at) VALUES ('g2', 9, 2)");
    });
    const bytes = new Uint8Array(await (await env.FILES.get(key))!.arrayBuffer());
    const r = await api(`/v1/admin/import/members/${b.id}/state`, { method: "POST", headers: ADMIN, raw: bytes as unknown as string });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const restored = await runInDurableObject(gw(b.id), (_o, state) => ({
      cursors: state.storage.sql.exec("SELECT group_id, seq FROM cursors ORDER BY group_id").toArray(),
      keys: state.storage.sql.exec("SELECT key, topic_id FROM hook_keys").toArray(),
      deliveries: state.storage.sql.exec("SELECT id FROM hook_deliveries").toArray(),
    }));
    expect(restored).toEqual({
      cursors: [
        { group_id: "g1", seq: 42 },
        { group_id: "g2", seq: 9 },
      ],
      keys: [{ key: "pr:o/r#1", topic_id: "t1" }],
      deliveries: [{ id: "d1" }],
    });
    expect((await api(`/v1/admin/import/members/${ulid()}/state`, { method: "POST", headers: ADMIN, raw: bytes as unknown as string })).status).toBe(404);
    expect((await api(`/v1/admin/import/members/${b.id}/state`, { method: "POST", raw: bytes as unknown as string })).status).toBe(401);
  });
});

describe("card state updates after restore", () => {
  it("keep a message that was never edited without editedAt", async () => {
    const [a, b] = await Promise.all([createMember({ level: "owner" }), createMember()]);
    const bot = await createMember({ kind: "bot", name: "Ledger" });
    const gid = await createGroup(a, [b.id, bot.id]);
    const { c: cbot } = await Client.connect(bot.token, { features: ["interact/1"] });
    const { c: cb } = await Client.connect(b.token, { features: ["interact/1"] });
    const card = {
      v: 2,
      catalog: "tc.card/2.1",
      rev: 1,
      intent: "ask",
      title: "发版吗？",
      fallback: "发版吗？",
      ask: { from: [b.id] },
      blocks: [{ id: "pick", type: "decision", deliver: "immediate", options: [{ id: "yes", label: "发" }, { id: "no", label: "不发" }] }],
    };
    const m = (await cbot.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: card })).message;
    const r = await cb.request({ t: "card.event", groupId: gid, messageId: m.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "pick", event: "choice.submit", value: { value: "yes" } });
    expect(r).toMatchObject({ ok: true });
    const upd = await cb.waitEvent((e) => e.type === "message.updated" && e.data.messageId === m.id);
    // A state update: editedAt is the creation time, and the message was not edited.
    expect(upd.data.editedAt).toBe(m.createdAt);
    const hist = await cb.request({ t: "history", groupId: gid, topicId: m.topicId });
    expect(hist.messages.find((x: any) => x.id === m.id).editedAt).toBeNull();
    const rows = unwrap(await roomStub(env, gid).exportRows(gid, 0)).rows;
    const target = await emptyGroupLike([a.id, b.id, bot.id]);
    const imp = await importChunk(target, await gzip(rows.map((x) => JSON.stringify(x)).join("\n") + "\n"));
    expect(imp.status, JSON.stringify(imp.body)).toBe(200);
    const edited = await runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(target)), async (_o, state) =>
      state.storage.sql.exec<{ edited_at: number | null }>("SELECT edited_at FROM messages WHERE id = ?", m.id).one().edited_at,
    );
    expect(edited).toBeNull();
    // Let the room and the gateways finish their work (bots outbox, presence, close) before the test ends.
    await new Promise((r) => setTimeout(r, 300));
    for (const x of [cbot, cb]) x.close();
    await new Promise((r) => setTimeout(r, 300));
  });
});
