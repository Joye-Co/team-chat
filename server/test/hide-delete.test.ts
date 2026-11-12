// Hide and delete (docs/protocol.md, "Hide and delete"): hidden 群, 私聊 and 话题, topic.delete,
// DELETE /v1/groups/:id and DELETE /v1/members/:id.
import { SELF, env, runInDurableObject } from "cloudflare:test";
import { SERVER_FEATURES, mainTopicId } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { ulid } from "../src/core/ids";
import { GROUP_PREFIX, groupStateKey, parseGroupFileKey, runArchive, utcDate } from "../src/ops/archive";
import { gunzip } from "../src/ops/gzip";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const send = (c: Client, groupId: string, body: string, extra: Record<string, unknown> = {}) =>
  c.ok({ t: "send", groupId, clientMsgId: clientMsgId(), body, ...extra });

const prefs = (gid: string, m: TestMember, body: unknown) => api(`/v1/groups/${gid}/prefs`, { method: "PATCH", token: m.token, body });

async function summaryOf(m: TestMember, gid: string): Promise<any> {
  const r = await api("/v1/groups", { token: m.token });
  return r.body.find((s: any) => s.group.id === gid);
}

async function lastAudit(action: string, target: string): Promise<any> {
  const row = await env.DB.prepare("SELECT actor_id, details FROM audit WHERE action = ? AND target = ? ORDER BY id DESC LIMIT 1")
    .bind(action, target)
    .first<{ actor_id: string; details: string }>();
  return row ? { actorId: row.actor_id, details: JSON.parse(row.details) } : null;
}

describe("features", () => {
  it("lists hide-delete/1 in welcome.features", async () => {
    const a = await createMember();
    const { c, welcome } = await Client.connect(a.token);
    expect(SERVER_FEATURES).toContain("hide-delete/1");
    expect(welcome.features).toContain("hide-delete/1");
    c.close();
  });
});

describe("hide a 群 or 私聊", () => {
  it("is per member, audited, and cleared by someone else's message", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token);

    const r = await prefs(gid, b, { hidden: true });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body.hidden).toBe(true);
    expect((await summaryOf(b, gid)).hidden).toBe(true);
    expect((await summaryOf(a, gid)).hidden).toBeUndefined();
    // Every connection of mine gets the new summary.
    await cb.next((f) => f.t === "directory" && f.groups?.some((g: any) => g.group.id === gid && g.hidden === true));
    expect((await lastAudit("prefs.update", gid))?.details).toEqual({ groupId: gid, hidden: [false, true] });

    // My own message keeps it hidden.
    await send(cb, gid, "我自己说的", { topicId: mainTopicId(gid) });
    expect((await summaryOf(b, gid)).hidden).toBe(true);
    // Someone else's message shows it again.
    await send(ca, gid, "新消息", { topicId: mainTopicId(gid) });
    expect((await summaryOf(b, gid)).hidden).toBeUndefined();

    // Show again by hand.
    await prefs(gid, a, { hidden: true });
    const shown = await prefs(gid, a, { hidden: false });
    expect(shown.body.hidden).toBeUndefined();
    ca.close();
    cb.close();
  });

  it("POST /v1/dms shows a hidden DM again", async () => {
    const a = await createMember();
    const b = await createMember();
    const open = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    const dm = open.body.group.id as string;
    expect((await prefs(dm, a, { hidden: true })).body.hidden).toBe(true);
    const again = await api("/v1/dms", { token: a.token, body: { memberId: b.id } });
    expect(again.status).toBe(200);
    expect(again.body.group.id).toBe(dm);
    expect(again.body.hidden).toBeUndefined();
    expect((await summaryOf(a, dm)).hidden).toBeUndefined();
    expect((await lastAudit("prefs.update", dm))?.details).toEqual({ groupId: dm, hidden: [true, false] });
  });
});

describe("topic.hide", () => {
  it("hides a topic for me, lists it, refuses the 群聊, and a mention shows it again with reads", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token);
    const { c: cb2 } = await Client.connect(b.token);
    const topicId = (await send(ca, gid, "话题一")).topic.id as string;

    const st = await cb.ok({ t: "topic.hide", groupId: gid, topicId, hidden: true });
    expect(st).toMatchObject({ topicId, hidden: true });
    await cb2.next((f) => f.t === "reads" && f.groupId === gid && f.topics?.[0]?.topicId === topicId && f.topics[0].hidden === true);
    const listed = (await summaryOf(b, gid)).topics.find((t: any) => t.topicId === topicId);
    expect(listed).toMatchObject({ hidden: true });
    // Only for me.
    expect(((await summaryOf(a, gid)).topics ?? []).find((t: any) => t.topicId === topicId)?.hidden).toBeUndefined();

    expect(await cb.request({ t: "topic.hide", groupId: gid, topicId: mainTopicId(gid), hidden: true })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    expect(await cb.request({ t: "topic.hide", groupId: gid, topicId: "01NOPE", hidden: true })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });

    // A message without a mention keeps it hidden.
    await send(ca, gid, "没有提到你", { topicId });
    expect((await summaryOf(b, gid)).topics.find((t: any) => t.topicId === topicId).hidden).toBe(true);
    // A mention shows it again and sends reads to my connections.
    const before = cb2.frames.length;
    await send(ca, gid, "@b 看看", { topicId, mentions: [b.id] });
    await waitUntil(
      () => cb2.frames.slice(before).find((f) => f.t === "reads" && f.groupId === gid && f.topics?.[0]?.topicId === topicId && !f.topics[0].hidden),
      3000,
      "reads after a mention",
    );
    expect((await summaryOf(b, gid)).topics.find((t: any) => t.topicId === topicId).hidden).toBeUndefined();

    const shown = await cb.ok({ t: "topic.hide", groupId: gid, topicId, hidden: true });
    expect(shown.hidden).toBe(true);
    expect((await cb.ok({ t: "topic.hide", groupId: gid, topicId, hidden: false })).hidden).toBeUndefined();
    for (const c of [ca, cb, cb2]) c.close();
  });
});

describe("topic.delete", () => {
  it("lets the creator or an owner delete, scrubs messages, appends topic.deleted, and the topic is gone", async () => {
    const a = await createMember({ name: "creator" });
    const b = await createMember({ name: "other" });
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(a, [b.id, owner.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token);
    const { c: co } = await Client.connect(owner.token);
    const first = await send(ca, gid, "机密 规划 alpha");
    const topicId = first.topic.id as string;
    await send(cb, gid, "回复 规划 beta", { topicId });
    // A quote of a message of the topic, from the 群聊.
    const quoting = await send(cb, gid, "引用一下", { topicId: mainTopicId(gid), replyTo: { messageId: first.message.id } });

    expect(await cb.request({ t: "topic.delete", groupId: gid, topicId })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await ca.request({ t: "topic.delete", groupId: gid, topicId: mainTopicId(gid) })).toMatchObject({
      ok: false,
      error: { code: "invalid" },
    });
    expect(await ca.request({ t: "topic.delete", groupId: gid, topicId: "01NOPE" })).toMatchObject({ ok: false, error: { code: "not_found" } });

    const r = await ca.ok({ t: "topic.delete", groupId: gid, topicId });
    expect(r).toEqual({ topicId, deletedAt: expect.any(Number) });
    const ev = await cb.waitEvent((e) => e.groupId === gid && e.type === "topic.deleted");
    expect(ev).toMatchObject({ actorId: a.id, data: { topicId, deletedAt: r.deletedAt } });
    // Again: the first result, no second event.
    expect(await co.ok({ t: "topic.delete", groupId: gid, topicId })).toEqual(r);

    // No message.deleted events; the log keeps no content of the topic.
    const page = await cb.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.filter((e: any) => e.type === "message.deleted")).toEqual([]);
    expect(page.events.filter((e: any) => e.type === "topic.deleted")).toHaveLength(1);
    const created = page.events.filter((e: any) => e.type === "message.created" && e.data.message.topicId === topicId);
    expect(created).toHaveLength(2);
    for (const e of created) expect(e.data.message.body).toBe("");
    // The title (the first line of the first message) goes too.
    const made = page.events.find((e: any) => e.type === "topic.created" && e.data.topic.id === topicId);
    expect(made.data.topic.title).toBe("…");
    const quote = page.events.find((e: any) => e.type === "message.created" && e.data.message.id === quoting.message.id);
    expect(quote.data.message.replyTo.excerpt).toBe("");

    // Gone everywhere.
    expect(((await summaryOf(b, gid)).topics ?? []).some((t: any) => t.topicId === topicId)).toBe(false);
    expect(await cb.request({ t: "history", groupId: gid, topicId })).toMatchObject({ ok: false, error: { code: "not_found" } });
    expect(await cb.request({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "x" })).toMatchObject({
      ok: false,
      error: { code: "not_found" },
    });
    expect(await ca.request({ t: "topic.update", groupId: gid, topicId, status: "done" })).toMatchObject({ ok: false, error: { code: "not_found" } });
    for (const f of [
      { t: "topic.follow", follow: true },
      { t: "topic.mute", muted: true },
      { t: "topic.hide", hidden: true },
    ]) {
      expect(await cb.request({ ...f, groupId: gid, topicId }), f.t).toMatchObject({ ok: false, error: { code: "not_found" } });
    }
    const hits = await cb.request({ t: "search", query: "规划", groupId: gid });
    expect(hits.t).toBe("searchResults");
    expect(hits.hits).toEqual([]);

    expect((await lastAudit("topic.delete", gid))).toEqual({ actorId: a.id, details: { groupId: gid, topicId, messages: 2 } });
    for (const c of [ca, cb, co]) c.close();
  });

  it("lets an owner delete someone else's topic and leaves its 产出 out of GET /v1/artifacts", async () => {
    const a = await createMember();
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(a, [owner.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: co } = await Client.connect(owner.token);
    const keep = (await send(ca, gid, "保留")).topic.id as string;
    const gone = (await send(ca, gid, "删除")).topic.id as string;
    const bytes = new TextEncoder().encode("hello");
    const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    const sha = [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
    const art = async (topicId: string) => {
      const f = await api("/v1/files", { token: a.token, body: { groupId: gid, name: "a.txt", size: bytes.length, mime: "text/plain", sha256: sha } });
      expect(f.status, JSON.stringify(f.body)).toBe(200);
      await SELF.fetch(`${BASE}/v1/files/${f.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${a.token}` }, body: bytes });
      const r = await api("/v1/artifacts", {
        token: a.token,
        body: { groupId: gid, topicId, title: "产出", kind: "file", entry: "a.txt", files: [{ path: "a.txt", fileId: f.body.file.id }] },
      });
      expect(r.status, JSON.stringify(r.body)).toBe(200);
      return r.body.artifact?.id ?? r.body.id;
    };
    await art(keep);
    await art(gone);
    expect((await api(`/v1/artifacts?groupId=${gid}`, { token: a.token })).body).toHaveLength(2);
    await co.ok({ t: "topic.delete", groupId: gid, topicId: gone });
    const list = (await api(`/v1/artifacts?groupId=${gid}`, { token: a.token })).body as any[];
    expect(list.map((x) => x.topicId)).toEqual([keep]);
    expect((await lastAudit("topic.delete", gid))?.actorId).toBe(owner.id);
    ca.close();
    co.close();
  });
});

describe("runs of a deleted topic or group", () => {
  it("stops the topic's active runs when it is deleted, and a group's when it is deleted", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot", name: "ext" });
    const gid = await createGroup(owner, [bot.id]);
    const { c } = await Client.connect(owner.token);
    const t1 = (await send(c, gid, "第一件事")).topic.id as string;
    const t2 = (await send(c, gid, "第二件事")).topic.id as string;
    const r1 = await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId: t1 } });
    const r2 = await api("/v1/runs", { token: bot.token, body: { groupId: gid, topicId: t2 } });
    expect(r1.body.status).toBe("running");
    await c.ok({ t: "topic.delete", groupId: gid, topicId: t1 });
    expect((await api(`/v1/runs/${r1.body.id}`, { token: owner.token })).body.status).toBe("cancelled");
    expect((await api(`/v1/runs/${r2.body.id}`, { token: owner.token })).body.status).toBe("running");
    // Deleting the group stops the rest.
    expect((await api(`/v1/groups/${gid}`, { method: "DELETE", token: owner.token })).status).toBe(200);
    const ns = (env as any).BOT_DESK as DurableObjectNamespace;
    const ended = await runInDurableObject(ns.get(ns.idFromName(bot.id)), (_o, state) =>
      state.storage.sql.exec("SELECT status, ended_at FROM runs WHERE id = ?", r2.body.id).toArray()[0],
    );
    expect(ended).toMatchObject({ status: "cancelled", ended_at: expect.any(Number) });
    c.close();
  });
});

describe("DELETE /v1/groups/:id", () => {
  it("lets an owner delete a group: members get removedGroupIds and the group answers not_found", async () => {
    const owner = await createMember({ level: "owner" });
    const b = await createMember();
    const gid = await createGroup(owner, [b.id], "要删的群");
    const { c: cb } = await Client.connect(b.token);
    await send(cb, gid, "一些内容");

    const refused = await api(`/v1/groups/${gid}`, { method: "DELETE", token: b.token });
    expect(refused.status).toBe(403);
    expect(refused.body.error.code).toBe("forbidden");

    const r = await api(`/v1/groups/${gid}`, { method: "DELETE", token: owner.token });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body).toEqual({ ok: true });
    await cb.next((f) => f.t === "directory" && f.removedGroupIds?.includes(gid));
    expect(await summaryOf(b, gid)).toBeUndefined();
    expect(await summaryOf(owner, gid)).toBeUndefined();
    expect(await cb.request({ t: "send", groupId: gid, topicId: mainTopicId(gid), clientMsgId: clientMsgId(), body: "x" })).toMatchObject({ ok: false });
    expect((await api(`/v1/groups/${gid}`, { method: "PATCH", token: owner.token, body: { name: "x" } })).status).toBe(404);
    expect((await prefs(gid, b, { muted: true })).status).toBe(404);
    expect((await api(`/v1/groups/${gid}`, { method: "DELETE", token: owner.token })).status).toBe(404);
    const members = await env.DB.prepare("SELECT count(*) AS n FROM group_members WHERE group_id = ?").bind(gid).first<{ n: number }>();
    expect(members?.n).toBe(0);
    expect(await lastAudit("group.delete", gid)).toEqual({ actorId: owner.id, details: { name: "要删的群", members: [owner.id, b.id] } });
    cb.close();
  });

  it("refuses DMs and the default group", async () => {
    const owner = await createMember({ level: "owner" });
    const b = await createMember();
    const dm = (await api("/v1/dms", { token: owner.token, body: { memberId: b.id } })).body.group.id;
    const r1 = await api(`/v1/groups/${dm}`, { method: "DELETE", token: owner.token });
    expect(r1.status).toBe(403);
    expect(r1.body.error.message).toBe("a DM cannot be deleted");
    const gid = await createGroup(owner, [b.id]);
    await env.DB.prepare("UPDATE groups SET is_default = 1 WHERE id = ?").bind(gid).run();
    const r2 = await api(`/v1/groups/${gid}`, { method: "DELETE", token: owner.token });
    expect(r2.status).toBe(403);
    expect(r2.body.error.message).toBe("the default group cannot be deleted");
    await env.DB.prepare("UPDATE groups SET is_default = 0 WHERE id = ?").bind(gid).run();
    expect((await api(`/v1/groups/01NOPE`, { method: "DELETE", token: owner.token })).status).toBe(404);
  });
});

describe("DELETE /v1/members/:id", () => {
  it("deletes a deactivated AI member: out of its groups, runner and placements gone, kept as deleted", async () => {
    const owner = await createMember({ level: "owner" });
    const person = await createMember();
    const bot = await createMember({ kind: "bot", name: "旧助手" });
    const gid = await createGroup(owner, [person.id, bot.id]);
    const dm = (await api("/v1/dms", { token: owner.token, body: { memberId: bot.id } })).body.group.id;
    const { c: co } = await Client.connect(owner.token);
    const now = Date.now();
    const machineId = `m-${crypto.randomUUID()}`;
    await env.DB.batch([
      env.DB.prepare("INSERT INTO bot_runners (member_id, runtime, updated_at) VALUES (?, 'codex', ?)").bind(bot.id, now),
      env.DB.prepare("INSERT INTO machines (id, name, token_hash, created_by, created_at, owner_id) VALUES (?, 'mac', ?, ?, ?, ?)").bind(
        machineId,
        crypto.randomUUID(),
        owner.id,
        now,
        owner.id,
      ),
      env.DB.prepare("INSERT INTO machine_bots (machine_id, bot_id, created_at) VALUES (?, ?, ?)").bind(machineId, bot.id, now),
    ]);

    const del = (m: TestMember, id = bot.id) => api(`/v1/members/${id}`, { method: "DELETE", token: m.token });
    expect((await del(person)).status).toBe(403);
    const human = await del(owner, person.id);
    expect(human.status).toBe(403);
    const active = await del(owner);
    expect(active.status).toBe(409);
    expect(active.body.error.message).toBe("deactivate the member first");
    expect((await del(owner, "01NOPE")).status).toBe(404);

    expect((await api(`/v1/members/${bot.id}`, { method: "PATCH", token: owner.token, body: { deactivated: true } })).status).toBe(200);
    const r = await del(owner);
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body).toMatchObject({ id: bot.id, deleted: true, deactivated: true });

    await co.waitEvent((e) => e.groupId === gid && e.type === "group.memberRemoved" && e.data.memberId === bot.id);
    await co.next((f) => f.t === "directory" && f.members?.some((m: any) => m.id === bot.id && m.deleted === true));
    expect((await summaryOf(owner, gid)).group.memberIds).not.toContain(bot.id);
    // Its DM stays, so old messages keep their author.
    expect((await summaryOf(owner, dm)).group.memberIds).toContain(bot.id);
    const list = (await api("/v1/members", { token: owner.token })).body as any[];
    expect(list.find((m) => m.id === bot.id)).toMatchObject({ deleted: true, deactivated: true });
    const roster = (await api("/v1/team/roster", { token: owner.token })).body;
    expect(roster.members.some((m: any) => m.memberId === bot.id)).toBe(false);
    const runner = await env.DB.prepare("SELECT 1 FROM bot_runners WHERE member_id = ?").bind(bot.id).first();
    const placed = await env.DB.prepare("SELECT 1 FROM machine_bots WHERE bot_id = ?").bind(bot.id).first();
    expect(runner).toBeNull();
    expect(placed).toBeNull();

    expect((await api(`/v1/members/${bot.id}`, { method: "PATCH", token: owner.token, body: { deactivated: false } })).status).toBe(404);
    expect((await del(owner)).status).toBe(404);
    expect((await api("/v1/invites", { token: owner.token, body: { name: "x", kind: "bot", level: "member", memberId: bot.id } })).status).toBe(404);
    expect(await lastAudit("member.delete", bot.id)).toEqual({ actorId: owner.id, details: { name: "旧助手" } });
    co.close();
  });
});

const ADMIN = { "x-admin-secret": "test-admin-secret" };

async function roomRows(gid: string) {
  return runInDurableObject(env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid)), (_o, state) => {
    const q = (s: string) => state.storage.sql.exec(s).toArray();
    return {
      topics: q("SELECT id FROM topics ORDER BY id"),
      messages: q("SELECT id, topic_id, body, deleted_at FROM messages ORDER BY id"),
      states: q("SELECT * FROM topic_states ORDER BY member_id, topic_id"),
      deleted: q("SELECT topic_id, deleted_at, messages FROM deleted_topics"),
      tables: q("SELECT name FROM sqlite_master WHERE type = 'table'").length,
      events: q("SELECT count(*) AS n FROM events")[0].n as number,
    };
  });
}

describe("archive and restore", () => {
  it("scrubs a deleted topic from older archive files and restores it as deleted", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const keep = (await send(ca, gid, "保留的话题")).topic.id as string;
    const gone = await send(ca, gid, "秘密内容 zeta");
    const topicId = gone.topic.id as string;
    await send(ca, gid, "更多秘密 zeta", { topicId, mentions: [b.id] });
    await runArchive(env, { skipD1: true });
    await ca.ok({ t: "topic.delete", groupId: gid, topicId });
    await runArchive(env, { skipD1: true });

    const keys = (await env.FILES.list({ prefix: `${GROUP_PREFIX}${gid}/` })).objects
      .map((o) => o.key)
      .filter((k) => parseGroupFileKey(k))
      .sort();
    expect(keys.length).toBe(2);
    for (const key of keys) {
      const text = await gunzip(await (await env.FILES.get(key))!.arrayBuffer());
      expect(text).not.toContain("zeta");
    }
    const original = await roomRows(gid);
    expect(original.topics.map((t: any) => t.id)).toEqual([keep]);
    expect(original.deleted).toEqual([{ topic_id: topicId, deleted_at: expect.any(Number), messages: 2 }]);

    // Restore into an empty room of a group with the same members.
    const target = ulid();
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare("INSERT INTO groups (id, kind, name, created_by, created_at) VALUES (?, 'group', 'restored', ?, ?)").bind(target, a.id, now),
      ...[a.id, b.id].map((m) => env.DB.prepare("INSERT INTO group_members (group_id, member_id, added_at) VALUES (?, ?, ?)").bind(target, m, now)),
    ]);
    for (const key of keys) {
      const bytes = new Uint8Array(await (await env.FILES.get(key))!.arrayBuffer());
      const r = await api(`/v1/admin/import/groups/${target}`, { method: "POST", headers: ADMIN, raw: bytes as unknown as string });
      expect(r.status, JSON.stringify(r.body)).toBe(200);
    }
    const stateBytes = new Uint8Array(await (await env.FILES.get(groupStateKey(gid, utcDate(Date.now()))))!.arrayBuffer());
    expect((await api(`/v1/admin/import/groups/${target}/state`, { method: "POST", headers: ADMIN, raw: stateBytes as unknown as string })).status).toBe(200);
    const restored = await roomRows(target);
    expect(restored.topics).toEqual(original.topics);
    expect(restored.messages).toEqual(original.messages);
    expect(restored.deleted.map((d: any) => d.topic_id)).toEqual([topicId]);
    expect(restored.states).toEqual(original.states);
    ca.close();
  });

  it("wipes a deleted group's room and leaves it out of the daily archive", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const { c } = await Client.connect(owner.token);
    await send(c, gid, "内容");
    expect((await roomRows(gid)).events).toBeGreaterThan(0);
    expect((await api(`/v1/groups/${gid}`, { method: "DELETE", token: owner.token })).status).toBe(200);
    const after = await roomRows(gid);
    expect(after.events).toBe(0);
    expect(after.topics).toEqual([]);
    const report = await runArchive(env, { skipD1: true });
    expect(report.files.some((k: string) => k.includes(gid))).toBe(false);
    c.close();
  });
});
