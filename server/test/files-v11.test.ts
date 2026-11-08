// 1.1 server-files gaps (GAPS.md SF-*): media file limits, multipart uploads, stored page
// events, desk notes, queued runs, aside origins, PR merge state, group purpose, rename order
// and 产出 previews.
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function upload(m: TestMember, groupId: string, name: string, mime: string, bytes: Uint8Array, extra: { width?: number; height?: number } = {}): Promise<string> {
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: await sha(bytes), ...extra } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

describe("SF-7 media file limits", () => {
  it("refuse a poster over 200 KB or 1280 px, a frame over 320 px and a transcript over 2 MB", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const { c } = await Client.connect(owner.token);
    const topicId = (await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "视频" })).topic.id;
    const file = await upload(owner, gid, "talk.mp4", "video/mp4", new Uint8Array(20));
    const bigPoster = await upload(owner, gid, "p.jpg", "image/jpeg", new Uint8Array(200_001));
    const widePoster = await upload(owner, gid, "w.jpg", "image/jpeg", new Uint8Array(10), { width: 1920, height: 1080 });
    const poster = await upload(owner, gid, "ok.jpg", "image/jpeg", new Uint8Array(10), { width: 1280, height: 720 });
    const transcript = await upload(owner, gid, "t.json", "application/json", new Uint8Array(2_000_001));
    const base = { groupId: gid, topicId, title: "录像", kind: "file", entry: "talk.mp4", files: [{ path: "talk.mp4", fileId: file }] };
    const video = { kind: "video", mime: "video/mp4", bytes: 20, duration: 60, preview: "ready" };
    const make = (media: Record<string, unknown>) => api("/v1/artifacts", { token: owner.token, body: { ...base, media: { ...video, ...media } } });
    for (const media of [{ posterFileId: bigPoster }, { posterFileId: widePoster }, { frames: [{ t: 1, fileId: poster }] }, { transcriptFileId: transcript }]) {
      const r = await make(media);
      expect(r.status, JSON.stringify(media)).toBe(413);
      expect(r.body.error.message).toMatch(/^media: /);
    }
    expect([400, 413]).toContain((await make({ kind: "table", mime: "text/csv", bytes: 200_000_001, duration: undefined })).status);
    expect((await make({ posterFileId: poster })).status).toBe(200);
    c.close();
  });
});

describe("SF-1 multipart upload", () => {
  it("upload 150 MB in parts, download it with the right hash, and refuse 2.1 GB before upload", async () => {
    const owner = await createMember({ level: "owner" });
    const member = await createMember();
    const gid = await createGroup(owner, [member.id]);
    const size = 150_000_000;
    const bytes = new Uint8Array(size);
    for (let i = 0; i < size; i += 4096) bytes[i] = (i / 4096) & 0xff;
    const hash = await sha(bytes);
    const tooBig = await api("/v1/files", { token: owner.token, body: { groupId: gid, name: "big.mov", size: 2_100_000_000, mime: "video/quicktime", sha256: hash } });
    expect(tooBig.status).toBe(400);
    const c = await api("/v1/files", { token: owner.token, body: { groupId: gid, name: "big.mov", size, mime: "video/quicktime", sha256: hash } });
    expect(c.status, JSON.stringify(c.body)).toBe(200);
    const mp = c.body.multipart;
    expect(mp).toMatchObject({ partBytes: 52_428_800, parts: 3, partUrl: `${BASE}/v1/files/${c.body.file.id}/parts`, completeUrl: `${BASE}/v1/files/${c.body.file.id}/complete` });
    // The single PUT is for files up to 100 MB.
    const single = await SELF.fetch(c.body.uploadUrl, { method: "PUT", headers: { authorization: `Bearer ${owner.token}` }, body: new Uint8Array(1) });
    expect(single.status).toBe(413);
    await single.arrayBuffer();
    const put = (n: number, body: Uint8Array, token = owner.token) =>
      SELF.fetch(`${mp.partUrl}/${n}`, { method: "PUT", headers: { authorization: `Bearer ${token}` }, body });
    const part = (n: number) => bytes.subarray((n - 1) * mp.partBytes, Math.min(size, n * mp.partBytes));
    // Only the creator; a part of the wrong length is refused.
    const other = await put(1, part(1), member.token);
    expect(other.status).toBe(403);
    await other.arrayBuffer();
    const short = await put(1, part(1).subarray(1));
    expect(short.status).toBe(400);
    await short.arrayBuffer();
    // Out of order, and complete before the last part says what is missing.
    for (const n of [3, 1]) {
      const r = await put(n, part(n));
      expect(r.status).toBe(200);
      expect(await r.json()).toEqual({ part: n, size: part(n).length });
    }
    const early = await api(`/v1/files/${c.body.file.id}/complete`, { method: "POST", token: owner.token });
    expect(early.status).toBe(400);
    expect(early.body.error.message).toContain("parts missing");
    const r2 = await put(2, part(2));
    expect(r2.status).toBe(200);
    await r2.arrayBuffer();
    const done = await api(`/v1/files/${c.body.file.id}/complete`, { method: "POST", token: owner.token });
    expect(done.status, JSON.stringify(done.body)).toBe(200);
    expect(done.body).toMatchObject({ status: "ready", size, sha256: hash });
    const got = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { headers: { authorization: `Bearer ${member.token}` } });
    expect(got.status).toBe(200);
    expect(got.headers.get("content-length")).toBe(String(size));
    expect(await sha(new Uint8Array(await got.arrayBuffer()))).toBe(hash);
    // Once ready, no more parts.
    const late = await put(1, part(1));
    expect(late.status).toBe(409);
    await late.arrayBuffer();
  });

  it("refuse a completed upload whose bytes do not match the sha256, and take the parts again", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const size = 100_000_001;
    const bytes = new Uint8Array(size);
    const c = await api("/v1/files", { token: owner.token, body: { groupId: gid, name: "x.bin", size, mime: "application/octet-stream", sha256: "0".repeat(64) } });
    expect(c.status).toBe(200);
    const mp = c.body.multipart;
    const send = async () => {
      for (let n = 1; n <= mp.parts; n++) {
        const r = await SELF.fetch(`${mp.partUrl}/${n}`, { method: "PUT", headers: { authorization: `Bearer ${owner.token}` }, body: bytes.subarray((n - 1) * mp.partBytes, Math.min(size, n * mp.partBytes)) });
        expect(r.status).toBe(200);
        await r.arrayBuffer();
      }
    };
    await send();
    const bad = await api(`/v1/files/${c.body.file.id}/complete`, { method: "POST", token: owner.token });
    expect(bad.status).toBe(400);
    expect(bad.body.error.message).toContain("sha256");
    expect((await api(`/v1/files/${c.body.file.id}`, { token: owner.token })).body.status).toBe("pending");
    // The parts are gone; a new upload starts.
    const again = await api(`/v1/files/${c.body.file.id}/complete`, { method: "POST", token: owner.token });
    expect(again.body.error.message).toContain("parts missing");
    await send();
  });
});

describe("SF-2 page log", () => {
  it("store page events with payloads and list them to every member, newest first", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const bot = await createMember({ kind: "bot", name: "ledger" });
    const outsider = await createMember();
    const gid = await createGroup(owner, [alice.id, bot.id]);
    const { c: ca } = await Client.connect(alice.token, { features: ["interact/1"] });
    const topicId = (await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "做个看板" })).topic.id;
    const html = await upload(bot, gid, "index.html", "text/html", new TextEncoder().encode("<p>hi</p>"));
    const made = await api("/v1/artifacts", {
      token: bot.token,
      body: { groupId: gid, topicId, title: "看板", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: html }], events: [{ name: "pick" }, { name: "ask.total", kind: "ask" }] },
    });
    expect(made.status, JSON.stringify(made.body)).toBe(200);
    const id = made.body.artifact.id as string;
    const first = await ca.ok({ t: "artifact.event", groupId: gid, artifactId: id, version: 1, name: "pick", payload: { channel: "bili" }, clientEventId: clientMsgId() });
    const second = await ca.ok({ t: "artifact.event", groupId: gid, artifactId: id, version: 1, name: "pick", payload: { channel: "weibo" } });
    ca.close();
    // Another member on another computer, after a restart: the log comes from the server.
    const list = await api(`/v1/artifacts/${id}/events`, { token: owner.token });
    expect(list.status, JSON.stringify(list.body)).toBe(200);
    expect(list.body.hasMore).toBe(false);
    expect(list.body.events.map((e: any) => [e.eventId, e.payload])).toEqual([
      [second.eventId, { channel: "weibo" }],
      [first.eventId, { channel: "bili" }],
    ]);
    expect(list.body.events[0]).toMatchObject({ artifactId: id, version: 1, name: "pick", kind: "emit", actorId: alice.id });
    const page = await api(`/v1/artifacts/${id}/events?limit=1`, { token: alice.token });
    expect(page.body.events.map((e: any) => e.eventId)).toEqual([second.eventId]);
    expect(page.body.hasMore).toBe(true);
    const older = await api(`/v1/artifacts/${id}/events?limit=1&before=${second.eventId}`, { token: alice.token });
    expect(older.body).toMatchObject({ events: [{ eventId: first.eventId }], hasMore: false });
    expect((await api(`/v1/artifacts/${id}/events`, { token: outsider.token })).status).toBe(404);
    expect((await api(`/v1/artifacts/${id}/events?limit=500`, { token: owner.token })).status).toBe(400);
  });
});

describe("SF-11 产出 preview", () => {
  it("store the author's preview card text and figures on a version and in the log", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const { c } = await Client.connect(owner.token);
    const topicId = (await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "报名" })).topic.id;
    const html = await upload(owner, gid, "index.html", "text/html", new TextEncoder().encode("<h1>x</h1>"));
    const preview = { title: "报名看板", summary: "这周报名 38 人，B 站最多。", metrics: [{ label: "报名", value: "38 人", delta: "+12" }, { label: "B 站", value: "9" }] };
    const made = await api("/v1/artifacts", { token: owner.token, body: { groupId: gid, topicId, title: "看板", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: html }], preview } });
    expect(made.status, JSON.stringify(made.body)).toBe(200);
    expect(made.body.version.preview).toEqual(preview);
    const ev = await c.waitEvent((e) => e.type === "artifact.created");
    expect(ev.data.version.preview).toEqual(preview);
    const v2 = await api(`/v1/artifacts/${made.body.artifact.id}/versions`, { token: owner.token, body: { entry: "index.html", files: [{ path: "index.html", fileId: html }] } });
    expect(v2.status).toBe(200);
    const got = await api(`/v1/artifacts/${made.body.artifact.id}`, { token: owner.token });
    expect(got.body.versions.map((v: any) => v.preview)).toEqual([undefined, preview]);
    const tooMany = { metrics: Array.from({ length: 7 }, (_, i) => ({ label: `m${i}`, value: "1" })) };
    expect((await api(`/v1/artifacts/${made.body.artifact.id}/versions`, { token: owner.token, body: { entry: "index.html", files: [{ path: "index.html", fileId: html }], preview: tooMany } })).status).toBe(400);
    c.close();
  });
});

describe("SF-8 / SF-9 group purpose and rename order", () => {
  const patch = (id: string, token: string, body: unknown) => api(`/v1/groups/${id}`, { method: "PATCH", token, body });

  it("set, read and clear a purpose line, alone or with a rename", async () => {
    const a = await createMember({ level: "owner" });
    const b = await createMember();
    const gid = await createGroup(a, [b.id], "开发");
    const { c: cb } = await Client.connect(b.token);
    const r = await patch(gid, a.token, { purpose: "  orderbot、拾光和　TeamChat 的开发 " });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body.group).toMatchObject({ name: "开发", purpose: "orderbot、拾光和 TeamChat 的开发" });
    const ev = await cb.waitEvent((e) => e.groupId === gid && e.type === "group.updated");
    expect(ev.data).toEqual({ name: "开发", purpose: "orderbot、拾光和 TeamChat 的开发" });
    const list = await api("/v1/groups", { token: b.token });
    expect(list.body.find((s: any) => s.group.id === gid).group.purpose).toBe("orderbot、拾光和 TeamChat 的开发");
    // A rename alone keeps the purpose and sends only the name.
    expect((await patch(gid, a.token, { name: "研发" })).body.group).toMatchObject({ name: "研发", purpose: "orderbot、拾光和 TeamChat 的开发" });
    const renamed = await cb.waitEvent((e) => e.groupId === gid && e.type === "group.updated" && e.data.name === "研发");
    expect(renamed.data).toEqual({ name: "研发" });
    // Clear it.
    const cleared = await patch(gid, a.token, { purpose: "" });
    expect(cleared.body.group.purpose).toBeUndefined();
    await cb.waitEvent((e) => e.groupId === gid && e.type === "group.updated" && e.data.purpose === "");
    expect((await patch(gid, a.token, { purpose: "x".repeat(201) })).status).toBe(400);
    expect((await patch(gid, a.token, {})).status).toBe(400);
    const audit = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action = 'group.purpose' AND target = ?").bind(gid).first<{ n: number }>();
    expect(audit?.n).toBe(2);
    cb.close();
  });

  it("end concurrent renames with D1 equal to the newest event of the room log", async () => {
    const a = await createMember({ level: "owner" });
    const gid = await createGroup(a, [], "起点");
    const names = Array.from({ length: 12 }, (_, i) => `名字 ${i}`);
    const results = await Promise.all(names.map((name, i) => patch(gid, a.token, i % 3 === 0 ? { name, purpose: `目的 ${i}` } : { name })));
    expect(results.every((r) => r.status === 200)).toBe(true);
    const { c } = await Client.connect(a.token);
    const page = await c.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    const updates = page.events.filter((e: any) => e.type === "group.updated");
    const lastName = updates.at(-1).data.name;
    const lastPurpose = updates.filter((e: any) => e.data.purpose !== undefined).at(-1)?.data.purpose ?? null;
    const row = await env.DB.prepare("SELECT name, purpose, update_seq FROM groups WHERE id = ?").bind(gid).first<any>();
    expect(row).toEqual({ name: lastName, purpose: lastPurpose, update_seq: updates.at(-1).seq });
    // A stale write (older seq) does not overwrite D1.
    await env.DB.prepare("UPDATE groups SET name = 'stale', update_seq = 0 WHERE id = ?").bind(gid).run();
    expect((await patch(gid, a.token, { name: lastName })).body.group.name).toBe(lastName);
    c.close();
  });
});

describe("SF-4 post a side chat answer to its topic", () => {
  it("store fromAside on the message, return it in history, and post an answer once", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot", name: "forge" });
    const gid = await createGroup(owner, [bot.id]);
    const { c } = await Client.connect(owner.token);
    const { c: cb } = await Client.connect(bot.token);
    const t = await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "重连之后补发" });
    const topicId = t.topic.id as string;
    const dm = (await api("/v1/dms", { token: owner.token, body: { memberId: bot.id } })).body.group.id as string;
    const side = await c.ok({ t: "send", groupId: dm, clientMsgId: clientMsgId(), body: "为什么会丢消息？", context: { groupId: gid, topicId, uptoSeq: t.message.createdSeq } });
    const answer = await cb.ok({ t: "send", groupId: dm, topicId: side.topic.id, clientMsgId: clientMsgId(), body: "建议：重连后按 seq 补发。" });
    const fromAside = { groupId: dm, topicId: side.topic.id, messageId: answer.message.id };
    const posted = await c.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "建议：重连后按 seq 补发。", fromAside });
    expect(posted.message.fromAside).toEqual(fromAside);
    // After a restart the history says it is posted.
    const page = await c.request({ t: "history", groupId: gid, topicId });
    expect(page.messages.find((m: any) => m.id === posted.message.id).fromAside).toEqual(fromAside);
    // Posting the same answer again is refused, also with a new clientMsgId.
    const again = await c.request({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "建议：重连后按 seq 补发。", fromAside });
    expect(again).toMatchObject({ ok: false, error: { code: "conflict", reason: "aside.already_posted" } });
    // The origin must be a side chat about this topic.
    const other = (await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "另一个话题" })).topic.id;
    const wrong = await c.request({ t: "send", groupId: gid, topicId: other, clientMsgId: clientMsgId(), body: "x", fromAside });
    expect(wrong).toMatchObject({ ok: false, error: { code: "invalid", reason: "aside.bad_origin" } });
    const missing = await c.request({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "x", fromAside: { ...fromAside, messageId: side.message.id.replace(/.$/, "Z") } });
    expect(missing).toMatchObject({ ok: false, error: { code: "not_found" } });
    // Someone outside the DM cannot name it.
    const stranger = await createMember();
    await api(`/v1/groups/${gid}/members`, { token: owner.token, body: { memberId: stranger.id } });
    const { c: cs } = await Client.connect(stranger.token);
    const theirs = await cs.request({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "x", fromAside });
    expect(theirs.ok).toBe(false);
    for (const x of [c, cb, cs]) x.close();
  });
});

describe("SF-3 queued runs (排队)", () => {
  it("queue a run when the only machine is full, give its place, and start it when the slot frees", async () => {
    const { Daemon, mention, world } = await import("./daemon");
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [], maxParallelRuns: 1 });
    const { c } = await Client.connect(w.human.token);
    const runs = (topicId: string) => c.events(w.gid).filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run);
    const t1 = await mention(c, w, "@dev 第一件事");
    const first = await d.next((f) => f.t === "run.assign" && f.topicId === t1);
    d.send({ t: "run.accept", runId: first.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === first.runId && e.data.run.status === "running");
    const t2 = await mention(c, w, "@dev 第二件事");
    const q2 = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === t2 && e.data.run.queuePosition !== undefined)).data.run;
    expect(q2).toMatchObject({ status: "created", queuePosition: 1 });
    expect(q2.machineId).toBeUndefined();
    expect(q2.queuedAt).toBeGreaterThan(0);
    const t3 = await mention(c, w, "@dev 第三件事");
    const q3 = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === t3 && e.data.run.queuePosition !== undefined)).data.run;
    expect(q3.queuePosition).toBe(2);
    // Nothing went to the full machine.
    expect(d.frames.filter((f) => f.t === "run.assign").length).toBe(1);
    // The first run ends: the oldest queued run gets the machine, the next one moves up.
    const seq = d.done(first.runId, { status: "succeeded", exitCode: 0, resultText: "好了", summary: "好了" });
    await d.acked(first.runId, seq);
    const second = await d.next((f) => f.t === "run.assign" && f.topicId === t2, 20_000);
    expect(second.runId).toBe(q2.id);
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === q3.id && e.data.run.queuePosition === 1, 20_000);
    const placed = runs(t2).at(-1);
    expect(placed.queuePosition).toBeUndefined();
    expect(placed.queuedAt).toBeUndefined();
    expect(placed.machineId).toBe(w.machineId);
    c.close();
    d.close();
  }, 40_000);

  it("still fail at once when no machine could ever take the run", async () => {
    const { mention, world } = await import("./daemon");
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const t = await mention(c, w);
    const ended = (await c.waitEvent((e) => e.type === "run.updated" && e.data.run.topicId === t && e.data.run.status === "failed")).data.run;
    expect(ended.queuePosition).toBeUndefined();
    c.close();
  });
});

describe("SF-3 desk notes", () => {
  it("let a bot set, read and clear its desk note, synced through the group log", async () => {
    const owner = await createMember({ level: "owner" });
    const jojo = await createMember({ kind: "bot", name: "jojo" });
    const gid = await createGroup(owner, [jojo.id]);
    const { c } = await Client.connect(owner.token);
    const put = (token: string, body: unknown) => api("/v1/desk-note", { method: "PUT", token, body });
    const r = await put(jojo.token, { groupId: gid, text: "  今天先把 1.1 发出去\n" });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    expect(r.body.seq).toBeGreaterThan(0);
    const ev = await c.waitEvent((e) => e.type === "desk.noted");
    expect(ev).toMatchObject({ groupId: gid, actorId: jojo.id, seq: r.body.seq, data: { text: "今天先把 1.1 发出去" } });
    // Same text again stores nothing.
    expect((await put(jojo.token, { groupId: gid, text: "今天先把 1.1 发出去" })).body).toEqual({ seq: null });
    const list = await api(`/v1/groups/${gid}/desk-notes`, { token: owner.token });
    expect(list.body).toEqual({ notes: [{ memberId: jojo.id, text: "今天先把 1.1 发出去", at: ev.at, seq: ev.seq }] });
    // A catching-up client gets it from the log.
    const page = await c.request({ t: "fetch", groupId: gid, afterSeq: 0 });
    expect(page.events.filter((e: any) => e.type === "desk.noted")).toHaveLength(1);
    // People do not set desk notes; notes are short; outsiders get nothing.
    expect((await put(owner.token, { groupId: gid, text: "x" })).status).toBe(403);
    expect((await put(jojo.token, { groupId: gid, text: "长".repeat(141) })).status).toBe(400);
    const other = await createMember({ kind: "bot" });
    expect((await put(other.token, { groupId: gid, text: "x" })).status).toBe(404);
    // Clear.
    expect((await put(jojo.token, { groupId: gid, text: "" })).status).toBe(200);
    await c.waitEvent((e) => e.type === "desk.noted" && e.data.text === "");
    expect((await api(`/v1/groups/${gid}/desk-notes`, { token: owner.token })).body).toEqual({ notes: [] });
    c.close();
  });
});

describe("SF-10 rate limit answers", () => {
  it("say how long to wait, and a rate-limited send leaves no gap in the write order", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const { c } = await Client.connect(owner.token);
    const topicId = (await c.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "burst" })).topic.id;
    const rids = Array.from({ length: 30 }, () => c.rid());
    for (const [i, rid] of rids.entries()) c.send({ t: "send", rid, groupId: gid, topicId, clientMsgId: clientMsgId(), body: `m${i}` });
    const acks = await Promise.all(rids.map((rid) => c.next((f) => f.rid === rid)));
    const limited = acks.filter((a) => !a.ok);
    expect(limited.length).toBeGreaterThan(0);
    for (const a of limited) {
      expect(a.error.code).toBe("rate_limited");
      expect(a.error.retryAfterMs).toBeGreaterThan(0);
      expect(a.error.retryAfterMs).toBeLessThanOrEqual(1000);
    }
    // After the wait, the next send goes through at once (no 2 s turn wait for the refused ones).
    await new Promise((r) => setTimeout(r, Math.max(...limited.map((a) => a.error.retryAfterMs))));
    const t0 = Date.now();
    await c.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "after" });
    expect(Date.now() - t0).toBeLessThan(1500);
    c.close();
  });
});
