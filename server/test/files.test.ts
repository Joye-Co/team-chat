import { SELF, env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { ArtifactResponse, ArtifactVersionResponse, FileMeta, UpdateInfo } from "@chat/protocol";
import { BASE, Client, api, clientMsgId, createGroup, createMember, protocolViolations, type TestMember } from "./helpers";

afterAll(() => {
  expect(protocolViolations).toEqual([]);
});

const enc = new TextEncoder();

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function raw(
  path: string,
  opts: { method?: string; token?: string; body?: BodyInit | null; headers?: Record<string, string> } = {},
): Promise<Response> {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  return SELF.fetch(`${BASE}${path}`, { method: opts.method ?? "GET", headers, body: opts.body ?? undefined });
}

async function createFile(
  m: TestMember,
  groupId: string,
  bytes: Uint8Array,
  extra: { name?: string; mime?: string; sha256?: string; size?: number } = {},
) {
  const r = await api("/v1/files", {
    token: m.token,
    body: {
      groupId,
      name: extra.name ?? "a.bin",
      size: extra.size ?? bytes.length,
      mime: extra.mime ?? "application/octet-stream",
      sha256: extra.sha256 ?? (await sha(bytes)),
    },
  });
  return r;
}

async function upload(m: TestMember, groupId: string, bytes: Uint8Array, extra: { name?: string; mime?: string } = {}) {
  const c = await createFile(m, groupId, bytes, extra);
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await raw(`/v1/files/${c.body.file.id}/content`, { method: "PUT", token: m.token, body: bytes });
  const body = (await put.json()) as any;
  expect(put.status, JSON.stringify(body)).toBe(200);
  return FileMeta.parse(body);
}

async function setup() {
  const owner = await createMember({ level: "owner" });
  const member = await createMember();
  const outsider = await createMember();
  const groupId = await createGroup(owner, [member.id]);
  const otherGroupId = await createGroup(outsider, []);
  return { owner, member, outsider, groupId, otherGroupId };
}

async function newTopic(c: Client, groupId: string): Promise<string> {
  const r = await c.ok({ t: "send", groupId, clientMsgId: clientMsgId(), body: "topic root" });
  return r.topic.id as string;
}

describe("files", () => {
  it("round-trips an upload and a download with the right headers", async () => {
    const { owner, groupId } = await setup();
    const bytes = enc.encode("hello, 文件 world");
    const created = await createFile(owner, groupId, bytes, { name: "报告 \"v1\".pdf", mime: "application/pdf" });
    expect(created.status).toBe(200);
    expect(created.body.file.status).toBe("pending");
    expect(created.body.uploadUrl).toBe(`${BASE}/v1/files/${created.body.file.id}/content`);

    const put = await raw(`/v1/files/${created.body.file.id}/content`, { method: "PUT", token: owner.token, body: bytes });
    expect(put.status).toBe(200);
    expect(((await put.json()) as any).status).toBe("ready");

    const meta = await api(`/v1/files/${created.body.file.id}`, { token: owner.token });
    expect(FileMeta.parse(meta.body).status).toBe("ready");

    const get = await raw(`/v1/files/${created.body.file.id}/content`, { token: owner.token });
    expect(get.status).toBe(200);
    expect(new Uint8Array(await get.arrayBuffer())).toEqual(bytes);
    expect(get.headers.get("content-type")).toBe("application/pdf");
    expect(get.headers.get("x-content-type-options")).toBe("nosniff");
    const cd = get.headers.get("content-disposition") ?? "";
    expect(cd.startsWith("attachment;")).toBe(true);
    expect(cd).toContain(`filename*=UTF-8''${encodeURIComponent("报告 \"v1\".pdf")}`);
    expect(cd).not.toMatch(/filename="[^"]*"[^;]*"/);

    // Images are inline; HTML is never served as HTML.
    const png = await upload(owner, groupId, enc.encode("not really a png"), { name: "a.png", mime: "image/png" });
    const img = await raw(`/v1/files/${png.id}/content`, { token: owner.token });
    expect(img.headers.get("content-disposition")?.startsWith("inline;")).toBe(true);
    await img.arrayBuffer();
    const html = await upload(owner, groupId, enc.encode("<script>alert(1)</script>"), { name: "x.html", mime: "text/html" });
    const h = await raw(`/v1/files/${html.id}/content`, { token: owner.token });
    expect(h.headers.get("content-type")).not.toContain("text/html");
    expect(h.headers.get("content-disposition")?.startsWith("attachment;")).toBe(true);
    await h.arrayBuffer();
  });

  it("serves byte ranges", async () => {
    const { owner, groupId } = await setup();
    const bytes = enc.encode("0123456789");
    const f = await upload(owner, groupId, bytes);
    const r1 = await raw(`/v1/files/${f.id}/content`, { token: owner.token, headers: { range: "bytes=2-5" } });
    expect(r1.status).toBe(206);
    expect(r1.headers.get("content-range")).toBe("bytes 2-5/10");
    expect(await r1.text()).toBe("2345");
    const r2 = await raw(`/v1/files/${f.id}/content`, { token: owner.token, headers: { range: "bytes=-3" } });
    expect(r2.status).toBe(206);
    expect(await r2.text()).toBe("789");
    const r3 = await raw(`/v1/files/${f.id}/content`, { token: owner.token, headers: { range: "bytes=7-" } });
    expect(await r3.text()).toBe("789");
    const r4 = await raw(`/v1/files/${f.id}/content`, { token: owner.token, headers: { range: "bytes=10-" } });
    expect(r4.status).toBe(416);
    expect(r4.headers.get("content-range")).toBe("bytes */10");
  });

  it("rejects a sha256 or length mismatch and keeps the file pending", async () => {
    const { owner, groupId } = await setup();
    const bytes = enc.encode("the right bytes");
    const c = await createFile(owner, groupId, bytes);
    const id = c.body.file.id;
    const wrong = enc.encode("the wrong bytes");
    expect(wrong.length).toBe(bytes.length);
    const bad = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: wrong });
    expect(bad.status).toBe(400);
    expect(((await bad.json()) as any).error.code).toBe("invalid");
    expect((await api(`/v1/files/${id}`, { token: owner.token })).body.status).toBe("pending");
    expect((await raw(`/v1/files/${id}/content`, { token: owner.token })).status).toBe(404);
    expect(await env.FILES.head(`files/${groupId}/${id}`)).toBeNull();

    const long = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: enc.encode("the right bytes!") });
    expect(long.status).toBe(400);
    await long.arrayBuffer();
    // A streamed body (no Content-Length) that is too long is rejected too.
    const stream = new ReadableStream({
      start(ctl) {
        ctl.enqueue(enc.encode("the right bytes"));
        ctl.enqueue(enc.encode(" and more"));
        ctl.close();
      },
    });
    const streamed = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: stream });
    expect(streamed.status).toBe(400);
    await streamed.arrayBuffer();

    const good = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: bytes });
    expect(good.status).toBe(200);
    await good.arrayBuffer();
    const again = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: bytes });
    expect(again.status).toBe(409);
  });

  it("enforces the size limit and the creator-only upload", async () => {
    const { owner, member, groupId } = await setup();
    const big = await createFile(owner, groupId, new Uint8Array(1), { size: 2_000_000_001 });
    expect(big.status).toBe(400);
    const ok = await createFile(owner, groupId, new Uint8Array(1), { size: 100_000_000 });
    expect(ok.status).toBe(200);
    const bytes = enc.encode("abc");
    const c = await createFile(owner, groupId, bytes);
    const put = await raw(`/v1/files/${c.body.file.id}/content`, { method: "PUT", token: member.token, body: bytes });
    expect(put.status).toBe(403);
  });

  it("denies access to files of other groups", async () => {
    const { owner, member, outsider, groupId, otherGroupId } = await setup();
    const f = await upload(owner, groupId, enc.encode("secret"), { name: "s.txt", mime: "text/plain" });
    for (const path of [`/v1/files/${f.id}`, `/v1/files/${f.id}/content`]) {
      expect((await raw(path, { token: outsider.token })).status).toBe(404);
      expect((await raw(path)).status).toBe(401);
    }
    const put = await raw(`/v1/files/${f.id}/content`, { method: "PUT", token: outsider.token, body: "x" });
    expect(put.status).toBe(404);
    // Cannot create a file in a group one is not in.
    expect((await createFile(outsider, groupId, enc.encode("x"))).status).toBe(404);

    // Attachments: a file of this group works and carries the server's record; another group's does not.
    const { c } = await Client.connect(member.token);
    const sent = await c.ok({
      t: "send",
      groupId,
      clientMsgId: clientMsgId(),
      body: "see file",
      attachments: [{ fileId: f.id, name: "forged.exe", size: 1, mime: "application/x-forged", sha256: "0".repeat(64) }],
    });
    expect(sent.message.attachments).toEqual([
      { fileId: f.id, name: "s.txt", size: 6, mime: "text/plain", sha256: f.sha256 },
    ]);
    const o = await Client.connect(outsider.token);
    const otherFile = await upload(outsider, otherGroupId, enc.encode("mine"));
    const cross = await o.c.request({
      t: "send",
      groupId: otherGroupId,
      clientMsgId: clientMsgId(),
      body: "steal",
      attachments: [{ fileId: f.id, name: "s.txt", size: 6, mime: "text/plain", sha256: f.sha256 }],
    });
    expect(cross).toMatchObject({ ok: false, error: { code: "invalid" } });
    const own = await c.request({
      t: "send",
      groupId,
      clientMsgId: clientMsgId(),
      body: "x",
      attachments: [{ fileId: otherFile.id, name: "a", size: 4, mime: "a/b", sha256: otherFile.sha256 }],
    });
    expect(own).toMatchObject({ ok: false, error: { code: "invalid" } });
    // A pending file cannot be attached.
    const pending = await createFile(member, groupId, enc.encode("later"));
    const p = await c.request({
      t: "send",
      groupId,
      clientMsgId: clientMsgId(),
      body: "x",
      attachments: [{ fileId: pending.body.file.id, name: "a", size: 5, mime: "a/b", sha256: pending.body.file.sha256 }],
    });
    expect(p).toMatchObject({ ok: false, error: { code: "invalid" } });

    // Edit replaces attachments, with the same group check.
    const f2 = await upload(member, groupId, enc.encode("second"), { name: "two.txt", mime: "text/plain" });
    const edited = await c.ok({
      t: "edit",
      groupId,
      messageId: sent.message.id,
      body: "see files",
      attachments: [{ fileId: f2.id, name: "x", size: 1, mime: "a/b", sha256: f2.sha256 }],
    });
    expect(edited.message.attachments.map((a: any) => a.fileId)).toEqual([f2.id]);
    const ev = await c.waitEvent((e) => e.type === "message.updated" && e.data.messageId === sent.message.id);
    expect(ev.data.attachments[0].name).toBe("two.txt");
    const badEdit = await c.request({
      t: "edit",
      groupId,
      messageId: sent.message.id,
      body: "x",
      attachments: [{ fileId: otherFile.id, name: "a", size: 4, mime: "a/b", sha256: otherFile.sha256 }],
    });
    expect(badEdit).toMatchObject({ ok: false, error: { code: "invalid" } });
    c.close();
    o.c.close();
  });
});

describe("产出", () => {
  it("creates versions, serves files by path and blocks path traversal", async () => {
    const { owner, member, outsider, groupId, otherGroupId } = await setup();
    const { c } = await Client.connect(member.token);
    const topicId = await newTopic(c, groupId);
    const page = await upload(member, groupId, enc.encode("<h1>v1</h1>"), { name: "index.html", mime: "text/html" });
    const js = await upload(member, groupId, enc.encode("console.log(1)"), { name: "app.js" });
    const foreign = await upload(outsider, otherGroupId, enc.encode("x"), { name: "x.css", mime: "text/css" });

    const base = { groupId, topicId, title: "页面", kind: "html" };
    const badEntry = await api("/v1/artifacts", {
      token: member.token,
      body: { ...base, entry: "app.js", files: [{ path: "index.html", fileId: page.id }, { path: "app.js", fileId: js.id }] },
    });
    expect(badEntry.status).toBe(400);
    const foreignFile = await api("/v1/artifacts", {
      token: member.token,
      body: { ...base, entry: "index.html", files: [{ path: "index.html", fileId: page.id }, { path: "x.css", fileId: foreign.id }] },
    });
    expect(foreignFile.status).toBe(400);
    const traversal = await api("/v1/artifacts", {
      token: member.token,
      body: { ...base, entry: "index.html", files: [{ path: "../index.html", fileId: page.id }] },
    });
    expect(traversal.status).toBe(400);
    const outsiderCreate = await api("/v1/artifacts", {
      token: outsider.token,
      body: { ...base, entry: "index.html", files: [{ path: "index.html", fileId: page.id }] },
    });
    expect(outsiderCreate.status).toBe(404);

    const created = await api("/v1/artifacts", {
      token: member.token,
      body: {
        ...base,
        entry: "index.html",
        files: [
          { path: "index.html", fileId: page.id },
          { path: "assets/app.js", fileId: js.id },
        ],
        note: "first",
      },
    });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    const v1 = ArtifactVersionResponse.parse(created.body);
    expect(v1.version.version).toBe(1);
    expect(v1.version.files.find((f) => f.path === "assets/app.js")?.size).toBe(14);
    const ev1 = await c.waitEvent((e) => e.type === "artifact.created");
    expect(ev1.data.artifact.id).toBe(v1.artifact.id);
    const id = v1.artifact.id;

    const entry = await raw(`/v1/artifacts/${id}/versions/1/files/index.html`, { token: owner.token });
    expect(entry.status).toBe(200);
    expect(entry.headers.get("content-type")).toBe("text/html; charset=utf-8");
    expect(entry.headers.get("content-security-policy")).toContain("sandbox");
    expect(entry.headers.get("x-content-type-options")).toBe("nosniff");
    expect(await entry.text()).toBe("<h1>v1</h1>");
    const asset = await raw(`/v1/artifacts/${id}/versions/1/files/assets/app.js`, { token: owner.token });
    expect(asset.headers.get("content-type")).toBe("text/javascript; charset=utf-8");
    expect(await asset.text()).toBe("console.log(1)");

    for (const p of ["%2E%2E/index.html", "assets/%2E%2E/%2E%2E/x", "assets%2F..%2Findex.html", "a%5Cb", "assets//app.js", "%00"]) {
      const r = await raw(`/v1/artifacts/${id}/versions/1/files/${p}`, { token: owner.token });
      expect([400, 404], p).toContain(r.status);
    }
    expect((await raw(`/v1/artifacts/${id}/versions/1/files/missing.html`, { token: owner.token })).status).toBe(404);
    expect((await raw(`/v1/artifacts/${id}/versions/1/files/index.html`, { token: outsider.token })).status).toBe(404);
    expect((await raw(`/v1/artifacts/${id}`, { token: outsider.token })).status).toBe(404);

    const page2 = await upload(owner, groupId, enc.encode("<h1>v2</h1>"), { name: "index.html", mime: "text/html" });
    const added = await api(`/v1/artifacts/${id}/versions`, {
      token: owner.token,
      body: { entry: "index.html", files: [{ path: "index.html", fileId: page2.id }], title: "页面 2" },
    });
    expect(added.status, JSON.stringify(added.body)).toBe(200);
    expect(added.body.version.version).toBe(2);
    expect(added.body.artifact).toMatchObject({ currentVersion: 2, title: "页面 2" });
    await c.waitEvent((e) => e.type === "artifact.versionAdded" && e.data.version.version === 2);
    const outsiderAdd = await api(`/v1/artifacts/${id}/versions`, {
      token: outsider.token,
      body: { entry: "index.html", files: [{ path: "index.html", fileId: page2.id }] },
    });
    expect(outsiderAdd.status).toBe(404);

    const got = ArtifactResponse.parse((await api(`/v1/artifacts/${id}`, { token: member.token })).body);
    expect(got.versions.map((v) => v.version)).toEqual([2, 1]);
    expect(await (await raw(`/v1/artifacts/${id}/versions/1/files/index.html`, { token: member.token })).text()).toBe("<h1>v1</h1>");
    expect(await (await raw(`/v1/artifacts/${id}/versions/2/files/index.html`, { token: member.token })).text()).toBe("<h1>v2</h1>");
    expect((await raw(`/v1/artifacts/${id}/versions/3/files/index.html`, { token: member.token })).status).toBe(404);

    const list = await api(`/v1/artifacts?groupId=${groupId}&topicId=${topicId}`, { token: member.token });
    expect(list.body.map((a: any) => a.id)).toEqual([id]);
    expect((await api(`/v1/artifacts?groupId=${groupId}`, { token: outsider.token })).status).toBe(404);

    // Messages may reference existing versions of this group's 产出 only.
    const ref = await c.ok({ t: "send", groupId, topicId, clientMsgId: clientMsgId(), body: "done", artifactRefs: [{ artifactId: id, version: 2 }] });
    expect(ref.message.artifactRefs).toEqual([{ artifactId: id, version: 2 }]);
    const badRef = await c.request({ t: "send", groupId, topicId, clientMsgId: clientMsgId(), body: "x", artifactRefs: [{ artifactId: id, version: 9 }] });
    expect(badRef).toMatchObject({ ok: false, error: { code: "invalid" } });
    const o = await Client.connect(outsider.token);
    const crossRef = await o.c.request({ t: "send", groupId: otherGroupId, clientMsgId: clientMsgId(), body: "x", artifactRefs: [{ artifactId: id, version: 1 }] });
    expect(crossRef).toMatchObject({ ok: false, error: { code: "invalid" } });

    // A file 产出 has exactly one file; non-image files download as attachments.
    const fileArt = await api("/v1/artifacts", {
      token: member.token,
      body: { groupId, topicId, title: "数据", kind: "file", entry: "app.js", files: [{ path: "app.js", fileId: js.id }] },
    });
    expect(fileArt.status).toBe(200);
    const dl = await raw(`/v1/artifacts/${fileArt.body.artifact.id}/versions/1/files/app.js`, { token: member.token });
    expect(dl.headers.get("content-disposition")?.startsWith("attachment;")).toBe(true);
    await dl.arrayBuffer();
    c.close();
    o.c.close();
  });

  it("numbers concurrent versions from the current row while a bot's run check waits on D1", async () => {
    const owner = await createMember({ level: "owner" });
    const bot = await createMember({ kind: "bot" });
    const groupId = await createGroup(owner, [bot.id]);
    const { c } = await Client.connect(owner.token);
    const topicId = await newTopic(c, groupId);
    const page = await upload(owner, groupId, enc.encode("<h1>v</h1>"), { name: "index.html", mime: "text/html" });
    const files = [{ path: "index.html", fileId: page.id }];
    const created = await api("/v1/artifacts", {
      token: owner.token,
      body: { groupId, topicId, title: "页面", kind: "html", entry: "index.html", files },
    });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    const id = created.body.artifact.id as string;

    // Hold the room's bot_runs lookup until the owner's version is committed.
    let entered!: () => void;
    const inLookup = new Promise<void>((r) => (entered = r));
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    const genv = env as unknown as { GROUP_ROOM: DurableObjectNamespace };
    const room = genv.GROUP_ROOM.get(genv.GROUP_ROOM.idFromName(groupId));
    const inDo = runInDurableObject as unknown as (stub: unknown, fn: (inst: any) => void) => Promise<void>;
    await inDo(room, (inst: any) => {
      const orig = inst.env;
      const prepare = (q: string) => {
        const st = orig.DB.prepare(q);
        if (!q.includes("bot_runs")) return st;
        return {
          bind: (...args: unknown[]) => ({
            first: async () => {
              entered();
              await held;
              inst.env = orig;
              return st.bind(...args).first();
            },
          }),
        };
      };
      inst.env = { ...orig, DB: { ...orig.DB, prepare, batch: orig.DB.batch.bind(orig.DB) } };
    });
    const byBot = api(`/v1/artifacts/${id}/versions`, {
      token: bot.token,
      body: { entry: "index.html", files, runId: "no-such-run" },
    });
    await inLookup;
    const byOwner = await api(`/v1/artifacts/${id}/versions`, { token: owner.token, body: { entry: "index.html", files, title: "成员版" } });
    expect(byOwner.status, JSON.stringify(byOwner.body)).toBe(200);
    expect(byOwner.body.version.version).toBe(2);
    release();
    const late = await byBot;
    expect(late.status, JSON.stringify(late.body)).toBe(200);
    expect(late.body.version.version).toBe(3);
    // A version without a title keeps the title the owner set while the lookup waited.
    expect(late.body.artifact).toMatchObject({ currentVersion: 3, title: "成员版" });
    const got = ArtifactResponse.parse((await api(`/v1/artifacts/${id}`, { token: owner.token })).body);
    expect(got.versions.map((v) => v.version)).toEqual([3, 2, 1]);
    c.close();
  });
});

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 1, 2, 3]);

describe("avatars", () => {
  it("stores image avatars and serves them to members", async () => {
    const { owner, member, outsider } = await setup();
    const put = await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: member.token, body: PNG });
    const m = (await put.json()) as any;
    expect(put.status, JSON.stringify(m)).toBe(200);
    expect(m.avatarUrl).toMatch(new RegExp(`^${BASE}/v1/members/${member.id}/avatar\\?v=\\d+$`));
    const get = await raw(`/v1/members/${member.id}/avatar`, { token: outsider.token });
    expect(get.status).toBe(200);
    expect(get.headers.get("content-type")).toBe("image/png");
    expect(new Uint8Array(await get.arrayBuffer())).toEqual(PNG);
    expect((await raw(`/v1/members/${member.id}/avatar`)).status).toBe(401);
    expect((await api("/v1/me", { token: member.token })).body.avatarUrl).toBe(m.avatarUrl);

    const other = await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: outsider.token, body: PNG });
    expect(other.status).toBe(403);
    const byOwner = await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: owner.token, body: PNG });
    expect(byOwner.status).toBe(200);
    const notImage = await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: member.token, body: "<svg/>" });
    expect(notImage.status).toBe(400);
    const big = new Uint8Array(1_048_577);
    big.set(PNG);
    const tooBig = await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: member.token, body: big });
    expect(tooBig.status).toBe(413);
  });

  it("removes an avatar for the member or an owner", async () => {
    const { owner, member, outsider } = await setup();
    await raw(`/v1/members/${member.id}/avatar`, { method: "PUT", token: owner.token, body: PNG });
    expect((await raw(`/v1/members/${member.id}/avatar`, { method: "DELETE", token: outsider.token })).status).toBe(403);
    const del = await raw(`/v1/members/${member.id}/avatar`, { method: "DELETE", token: owner.token });
    expect(del.status).toBe(200);
    expect(((await del.json()) as any).avatarUrl).toBeNull();
    expect((await raw(`/v1/members/${member.id}/avatar`, { token: member.token })).status).toBe(404);
    expect((await api("/v1/me", { token: member.token })).body.avatarUrl).toBeNull();
    const again = await raw(`/v1/members/${member.id}/avatar`, { method: "DELETE", token: member.token });
    expect(again.status).toBe(200);
  });
});

async function gzip(text: string): Promise<Uint8Array> {
  const s = new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(s).arrayBuffer());
}

describe("diagnostics", () => {
  it("stores gzip uploads that owners can list and download", async () => {
    const { owner, member } = await setup();
    const body = await gzip("log line\n");
    const headers = { "content-type": "application/gzip" };
    const up = await raw("/v1/diagnostics", { method: "POST", token: member.token, body, headers });
    const { id } = (await up.json()) as any;
    expect(up.status).toBe(200);
    const list = await api("/v1/diagnostics", { token: owner.token });
    expect(list.body.entries.find((e: any) => e.id === id)).toMatchObject({ memberId: member.id, size: body.length });
    expect((await api("/v1/diagnostics", { token: member.token })).status).toBe(403);
    const dl = await raw(`/v1/diagnostics/${id}`, { token: owner.token });
    expect(new Uint8Array(await dl.arrayBuffer())).toEqual(body);
    expect((await raw(`/v1/diagnostics/${id}`, { token: member.token })).status).toBe(403);
    expect(await env.FILES.head(`diagnostics/${member.id}/${id}.gz`)).not.toBeNull();

    const notGzip = await raw("/v1/diagnostics", { method: "POST", token: member.token, body: "plain text here......", headers });
    expect(notGzip.status).toBe(400);
    const big = new Uint8Array(5_242_881);
    big.set(body);
    const tooBig = await raw("/v1/diagnostics", { method: "POST", token: member.token, body: big, headers });
    expect(tooBig.status).toBe(413);
    for (let i = 1; i < 10; i++) {
      const r = await raw("/v1/diagnostics", { method: "POST", token: member.token, body, headers });
      expect(r.status).toBe(200);
      await r.arrayBuffer();
    }
    const limited = await raw("/v1/diagnostics", { method: "POST", token: member.token, body, headers });
    expect(limited.status).toBe(429);
  });
});

async function publish(version: string, bytes: Uint8Array, opts: { secret?: string; sha256?: string; notes?: string } = {}) {
  const headers: Record<string, string> = {
    "x-admin-secret": opts.secret ?? "test-admin-secret",
    "x-sha256": opts.sha256 ?? (await sha(bytes)),
  };
  if (opts.notes !== undefined) {
    headers["x-release-notes"] = btoa(String.fromCharCode(...enc.encode(opts.notes)));
  }
  return raw(`/v1/admin/releases/${version}`, { method: "PUT", body: bytes, headers });
}

describe("releases and updates", () => {
  it("publishes releases, picks the highest version and serves downloads", async () => {
    const { owner, member } = await setup();
    const dmg9 = enc.encode("dmg 0.9.0 bytes");
    const dmg10 = enc.encode("dmg 0.10.0 bytes!");
    expect((await publish("0.9.0", dmg9, { secret: "wrong" })).status).toBe(401);
    expect((await publish("0.9", dmg9)).status).toBe(400);
    const badSha = await publish("0.9.0", dmg9, { sha256: "a".repeat(64) });
    expect(badSha.status).toBe(400);
    expect((await api("/v1/updates/latest", { token: member.token })).status).toBe(404);

    const p9 = await publish("0.9.0", dmg9, { notes: "修复 **bugs**" });
    const info9 = UpdateInfo.parse(await p9.json());
    expect(p9.status).toBe(200);
    expect(info9).toMatchObject({ version: "0.9.0", notes: "修复 **bugs**", size: dmg9.length });
    expect(info9.downloadUrl).toBe(`${BASE}/v1/updates/0.9.0/download`);
    // The same file again is done already (a release script repeats its uploads after one of
    // them failed): the answer of the first upload, nothing stored twice. Another file for the
    // version is a conflict.
    const again9 = await publish("0.9.0", dmg9);
    expect(again9.status).toBe(200);
    expect(UpdateInfo.parse(await again9.json())).toEqual(info9);
    expect((await publish("0.9.0", enc.encode("another dmg"))).status).toBe(409);
    expect((await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action = 'release.publish' AND target = '0.9.0'").first<{ n: number }>())?.n).toBe(1);
    expect((await publish("0.10.0", dmg10)).status).toBe(200);

    const latest = await api("/v1/updates/latest", { token: member.token });
    expect(latest.body.version).toBe("0.10.0");
    expect((await api("/v1/updates/latest")).status).toBe(401);

    const dl = await raw("/v1/updates/0.9.0/download", { token: member.token });
    expect(new Uint8Array(await dl.arrayBuffer())).toEqual(dmg9);
    expect(dl.headers.get("content-disposition")).toContain('filename="team-chat-0.9.0.dmg"');
    const part = await raw("/v1/updates/0.10.0/download", { token: member.token, headers: { range: "bytes=0-2" } });
    expect(part.status).toBe(206);
    expect(await part.text()).toBe("dmg");
    expect((await raw("/v1/updates/1.0.0/download", { token: member.token })).status).toBe(404);
    expect(await env.FILES.head("releases/0.10.0/team-chat-0.10.0.dmg")).not.toBeNull();

    const audit = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action = 'release.publish'").first<{ n: number }>();
    expect(audit?.n).toBe(2);
    void owner;
  });

  it("serves the latest DMG to holders of a valid invite code", async () => {
    const { owner } = await setup();
    await publish("1.2.3", enc.encode("the dmg"));
    const inv = await api("/v1/invites", { token: owner.token, body: { name: "实习生", kind: "human", level: "member" } });
    expect(inv.status).toBe(200);
    const code = inv.body.code as string;
    const ip = { "cf-connecting-ip": "203.0.113.7" };

    const ok = await raw(`/download?code=${encodeURIComponent(code)}`, { headers: ip });
    expect(ok.status).toBe(200);
    expect(ok.headers.get("content-disposition")).toContain('filename="team-chat-1.2.3.dmg"');
    expect(await ok.text()).toBe("the dmg");
    // Downloading does not use up the invite.
    const row = await env.DB.prepare("SELECT used_at FROM invites WHERE created_by = ?").bind(owner.id).first<{ used_at: number | null }>();
    expect(row?.used_at).toBeNull();
    expect((await raw(`/download?code=${code.toLowerCase().replace(/-/g, "")}`, { headers: ip })).status).toBe(200);

    const bad = await raw("/download?code=AAAA-BBBB-CCCC", { headers: ip });
    expect(bad.status).toBe(404);
    expect((await raw("/download?code=AAAA-BBBB", { headers: ip })).status).toBe(404);
    expect(bad.headers.get("content-type")).toContain("text/plain");

    // Links carry the code in the fragment: GET without a code is a page that posts it.
    const page = await raw("/download", { headers: ip });
    expect(page.status).toBe(200);
    expect(page.headers.get("content-type")).toContain("text/html");
    const csp = page.headers.get("content-security-policy") ?? "";
    expect(csp).toMatch(/script-src 'sha256-[A-Za-z0-9+/=]+'/);
    const html = await page.text();
    expect(html).toContain('location.hash');
    const script = html.slice(html.indexOf("<script>") + 8, html.indexOf("</script>"));
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", enc.encode(script)));
    expect(csp).toContain(`'sha256-${btoa(String.fromCharCode(...digest))}'`);
    const post = (code: string) =>
      raw("/download", {
        method: "POST",
        headers: { ...ip, "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({ code }).toString(),
      });
    const posted = await post(code);
    expect(posted.status).toBe(200);
    expect(await posted.text()).toBe("the dmg");
    const wrong = await post("AAAA-BBBB");
    expect(wrong.status).toBe(404);
    await wrong.arrayBuffer();

    await env.DB.prepare("UPDATE invites SET expires_at = ? WHERE created_by = ?").bind(Date.now() - 1, owner.id).run();
    expect((await raw(`/download?code=${code}`, { headers: ip })).status).toBe(404);

    // Rate limit per IP.
    const ip2 = { "cf-connecting-ip": "203.0.113.99" };
    let limited = 0;
    for (let i = 0; i < 25; i++) {
      const r = await raw("/download?code=AAAA-BBBB-CCCC", { headers: ip2 });
      if (r.status === 429) limited++;
      await r.arrayBuffer();
    }
    expect(limited).toBe(5);
  });

  it("withdraws a bad release so the previous one is offered again", async () => {
    const { member } = await setup();
    expect((await publish("7.0.0", enc.encode("good"))).status).toBe(200);
    expect((await publish("7.0.1", enc.encode("crashes"))).status).toBe(200);
    expect((await api("/v1/updates/latest", { token: member.token })).body.version).toBe("7.0.1");
    const withdraw = (body: unknown, secret = "test-admin-secret") =>
      api("/v1/admin/releases/7.0.1/withdraw", { headers: { "x-admin-secret": secret }, body });
    expect((await withdraw({}, "wrong")).status).toBe(401);
    const w = await withdraw({});
    expect(w.status, JSON.stringify(w.body)).toBe(200);
    expect(w.body).toMatchObject({ version: "7.0.1", withdrawn: true, latest: { version: "7.0.0" } });
    expect((await api("/v1/updates/latest", { token: member.token })).body.version).toBe("7.0.0");
    expect((await raw("/v1/updates/7.0.1/download", { token: member.token })).status).toBe(404);
    expect((await publish("7.0.1", enc.encode("again"))).status).toBe(409);
    const back = await withdraw({ restore: true });
    expect(back.body).toMatchObject({ withdrawn: false, latest: { version: "7.0.1" } });
    expect((await api("/v1/admin/releases/9.9.9/withdraw", { headers: { "x-admin-secret": "test-admin-secret" }, body: {} })).status).toBe(404);
    const audit = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action IN ('release.withdraw', 'release.restore')").first<{ n: number }>();
    expect(audit?.n).toBe(2);
  });

  it("keeps the Windows installer apart from the DMG", async () => {
    const { owner, member } = await setup();
    const publishWindows = async (version: string, bytes: Uint8Array, secret = "test-admin-secret") =>
      raw(`/v1/admin/releases/${version}/assets/windows`, {
        method: "PUT",
        body: bytes,
        headers: { "x-admin-secret": secret, "x-sha256": await sha(bytes), "x-release-notes": btoa("notes") },
      });
    const dmg = enc.encode("the dmg of 30.0.0");
    const exe = enc.encode("MZ the installer of 30.0.0");
    const latest = (query = "") => api(`/v1/updates/latest${query}`, { token: member.token });

    // Only a DMG: a Mac gets it, a Windows client gets nothing (never the DMG).
    expect((await publish("30.0.0", dmg)).status).toBe(200);
    expect((await latest()).body).toMatchObject({ version: "30.0.0", platform: "macos", downloadUrl: `${BASE}/v1/updates/30.0.0/download` });
    expect((await latest("?platform=macos")).body.sha256).toBe(await sha(dmg));
    expect((await latest("?platform=windows")).status).toBe(404);
    expect((await raw("/v1/updates/30.0.0/download?platform=windows", { token: member.token })).status).toBe(404);
    expect((await latest("?platform=linux")).status).toBe(400);

    expect((await publishWindows("30.0.0", exe, "wrong")).status).toBe(401);
    expect((await raw("/v1/admin/releases/30.0.0/assets/linux", { method: "PUT", body: exe, headers: { "x-admin-secret": "test-admin-secret", "x-sha256": await sha(exe) } })).status).toBe(400);
    const published = await publishWindows("30.0.0", exe);
    expect(published.status).toBe(200);
    const info = UpdateInfo.parse(await published.json());
    expect(info).toMatchObject({ version: "30.0.0", platform: "windows", size: exe.length, sha256: await sha(exe), notes: "notes" });
    expect(info.downloadUrl).toBe(`${BASE}/v1/updates/30.0.0/download?platform=windows`);
    const sameExe = await publishWindows("30.0.0", exe);
    expect(sameExe.status).toBe(200);
    expect(UpdateInfo.parse(await sameExe.json())).toEqual(info);
    expect((await publishWindows("30.0.0", enc.encode("another installer"))).status).toBe(409);
    // The secret is asked before anything else, for the same file too.
    expect((await publishWindows("30.0.0", exe, "wrong")).status).toBe(401);

    // Each platform has its own latest: a newer installer does not move the Mac, and the other way round.
    const exe31 = enc.encode("MZ the installer of 30.1.0");
    expect((await publishWindows("30.1.0", exe31)).status).toBe(200);
    expect((await latest("?platform=windows")).body).toMatchObject({ version: "30.1.0", platform: "windows" });
    expect((await latest()).body.version).toBe("30.0.0");
    expect((await publish("30.2.0", enc.encode("the dmg of 30.2.0"))).status).toBe(200);
    expect((await latest()).body.version).toBe("30.2.0");
    expect((await latest("?platform=windows")).body.version).toBe("30.1.0");

    const dl = await raw("/v1/updates/30.0.0/download?platform=windows", { token: member.token });
    expect(dl.status).toBe(200);
    expect(dl.headers.get("content-disposition")).toContain('filename="team-chat-30.0.0-setup.exe"');
    expect(dl.headers.get("content-type")).toBe("application/vnd.microsoft.portable-executable");
    expect(new Uint8Array(await dl.arrayBuffer())).toEqual(exe);
    const mac = await raw("/v1/updates/30.0.0/download", { token: member.token });
    expect(mac.headers.get("content-type")).toBe("application/x-apple-diskimage");
    expect(new Uint8Array(await mac.arrayBuffer())).toEqual(dmg);
    expect((await raw("/v1/updates/30.2.0/download?platform=windows", { token: member.token })).status).toBe(404);
    expect(await env.FILES.head("releases/30.0.0/team-chat-30.0.0-setup.exe")).not.toBeNull();

    // The invite download serves the installer to a browser on Windows.
    const inv = await api("/v1/invites", { token: owner.token, body: { name: "实习生", kind: "human", level: "member" } });
    const code = inv.body.code as string;
    const ip = { "cf-connecting-ip": "203.0.113.21" };
    const post = (fields: Record<string, string>) =>
      raw("/download", { method: "POST", headers: { ...ip, "content-type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(fields).toString() });
    const win = await post({ code, platform: "windows" });
    expect(win.headers.get("content-disposition")).toContain('filename="team-chat-30.1.0-setup.exe"');
    expect(new Uint8Array(await win.arrayBuffer())).toEqual(exe31);
    expect(await (await post({ code })).text()).toBe("the dmg of 30.2.0");
    expect(await (await post({ code, platform: "amiga" })).text()).toBe("the dmg of 30.2.0");
    expect(await (await raw("/download", { headers: ip })).text()).toContain('"windows"');
    // A link from before the fragment form, opened in a browser on Windows: the request names no
    // platform, so the browser's own words decide. Edge and Chrome on Windows 10 and 11, x64 and
    // ARM, all say "Windows NT 10.0".
    const edge = { ...ip, "user-agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0" };
    const safari = { ...ip, "user-agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15" };
    const old = await raw(`/download?code=${code}`, { headers: edge });
    expect(old.headers.get("content-disposition")).toContain('filename="team-chat-30.1.0-setup.exe"');
    expect(new Uint8Array(await old.arrayBuffer())).toEqual(exe31);
    expect(await (await raw(`/download?code=${code}`, { headers: safari })).text()).toBe("the dmg of 30.2.0");
    // A named platform wins over the browser: a Mac can fetch the installer and Windows the DMG.
    expect(await (await raw(`/download?code=${code}&platform=macos`, { headers: edge })).text()).toBe("the dmg of 30.2.0");
    expect((await raw(`/download?code=${code}&platform=windows`, { headers: safari })).headers.get("content-disposition")).toContain("-setup.exe");
    const posted = await raw("/download", { method: "POST", headers: { ...edge, "content-type": "application/x-www-form-urlencoded" }, body: new URLSearchParams({ code }).toString() });
    expect(posted.headers.get("content-disposition")).toContain('filename="team-chat-30.1.0-setup.exe"');
    await posted.arrayBuffer();

    // Withdrawing a version takes every platform's file of it.
    const withdraw = (version: string, body: unknown) =>
      api(`/v1/admin/releases/${version}/withdraw`, { headers: { "x-admin-secret": "test-admin-secret" }, body });
    expect((await withdraw("30.1.0", {})).body).toMatchObject({ version: "30.1.0", withdrawn: true, latest: { version: "30.2.0" } });
    expect((await latest("?platform=windows")).body.version).toBe("30.0.0");
    expect((await raw("/v1/updates/30.1.0/download?platform=windows", { token: member.token })).status).toBe(404);
    expect((await withdraw("30.0.0", {})).status).toBe(200);
    expect((await latest("?platform=windows")).status).toBe(404);
    expect((await withdraw("30.0.0", { restore: true })).status).toBe(200);
    expect((await latest("?platform=windows")).body.version).toBe("30.0.0");
  });
});

describe("files: review fixes", () => {
  it("keep the good object when an older upload attempt fails after a retry finished", async () => {
    const { owner, groupId } = await setup();
    const bytes = enc.encode("0123456789abcdef");
    const c = await createFile(owner, groupId, bytes);
    const id = c.body.file.id;
    // Attempt A: the network stalls after a few bytes.
    let ctl!: ReadableStreamDefaultController<Uint8Array>;
    const slow = new ReadableStream<Uint8Array>({ start: (x) => void (ctl = x) });
    ctl.enqueue(enc.encode("0123"));
    const a = raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: slow, headers: { "content-length": String(bytes.length) } });
    await new Promise((r) => setTimeout(r, 100));
    // Attempt B (the client's retry) finishes with the right bytes.
    const b = await raw(`/v1/files/${id}/content`, { method: "PUT", token: owner.token, body: bytes });
    expect(b.status).toBe(200);
    await b.arrayBuffer();
    // Then A ends with wrong bytes.
    ctl.enqueue(enc.encode("XXXXXXXXXXXX"));
    ctl.close();
    const ra = await a;
    expect(ra.status).toBe(400);
    await ra.arrayBuffer();
    expect((await api(`/v1/files/${id}`, { token: owner.token })).body.status).toBe("ready");
    const got = await raw(`/v1/files/${id}/content`, { token: owner.token });
    expect(got.status).toBe(200);
    expect(await got.text()).toBe("0123456789abcdef");
  });

  it("drop the attachments of a deleted message and stop serving files only it used", async () => {
    const { owner, member, groupId } = await setup();
    const { c } = await Client.connect(member.token);
    const { c: co } = await Client.connect(owner.token);
    const secret = await upload(member, groupId, enc.encode("API_KEY=secret"), { name: ".env", mime: "text/plain" });
    const shared = await upload(member, groupId, enc.encode("shared"), { name: "s.txt", mime: "text/plain" });
    const sent = await c.ok({
      t: "send",
      groupId,
      clientMsgId: clientMsgId(),
      body: "oops",
      attachments: [ref(secret), ref(shared)],
    });
    const topicId = sent.message.topicId;
    // Another message keeps using `shared`.
    await c.ok({ t: "send", groupId, topicId, clientMsgId: clientMsgId(), body: "again", attachments: [ref(shared)] });
    await c.ok({ t: "delete", groupId, messageId: sent.message.id });

    const h = await co.request({ t: "history", groupId, topicId });
    const gone = h.messages.find((m: any) => m.id === sent.message.id);
    expect(gone.deletedAt).toBeTypeOf("number");
    expect(gone.attachments).toBeUndefined();
    const page = await co.request({ t: "fetch", groupId, afterSeq: 0 });
    const created = page.events.find((e: any) => e.type === "message.created" && e.data.message.id === sent.message.id);
    expect(created.data.message.attachments).toBeUndefined();
    expect(created.data.message.body).toBe("");

    await waitFor(async () => (await raw(`/v1/files/${secret.id}/content`, { token: owner.token })).status === 404);
    expect((await api(`/v1/files/${secret.id}`, { token: owner.token })).status).toBe(404);
    expect(await env.FILES.head(`files/${groupId}/${secret.id}`)).toBeNull();
    const still = await raw(`/v1/files/${shared.id}/content`, { token: owner.token });
    expect(still.status).toBe(200);
    expect(await still.text()).toBe("shared");
    // A deleted file cannot be attached again.
    const re = await c.request({ t: "send", groupId, clientMsgId: clientMsgId(), body: "x", attachments: [ref(secret)] });
    expect(re).toMatchObject({ ok: false, error: { code: "invalid" } });
    c.close();
    co.close();
  });

  it("limit the bytes a member stores", async () => {
    const { owner, groupId } = await setup();
    await env.DB.prepare(
      `INSERT INTO files (id, group_id, name, size, mime, sha256, status, created_by, created_at)
       VALUES (?, ?, 'big', ?, 'a/b', ?, 'ready', ?, ?)`,
    )
      .bind(`big-${owner.id}`, groupId, 100 * 1024 ** 3, "0".repeat(64), owner.id, Date.now())
      .run();
    const r = await createFile(owner, groupId, enc.encode("x"));
    expect(r.status).toBe(413);
    expect(r.body.error.code).toBe("too_large");
  });

  it("rate limit file creation per member", async () => {
    const { owner, groupId } = await setup();
    const window = Math.floor(Date.now() / 3600_000) * 3600_000;
    await env.DB.prepare("INSERT INTO files_rate_hits (key, window, count) VALUES (?, ?, 500)").bind(`files:n:${owner.id}`, window).run();
    const r = await createFile(owner, groupId, enc.encode("x"));
    expect(r.status).toBe(429);
  });
});

function ref(f: FileMeta) {
  return { fileId: f.id, name: f.name, size: f.size, mime: f.mime, sha256: f.sha256 };
}

async function waitFor(fn: () => Promise<boolean>, ms = 5000): Promise<void> {
  const start = Date.now();
  while (!(await fn())) {
    if (Date.now() - start > ms) throw new Error("timed out");
    await new Promise((r) => setTimeout(r, 50));
  }
}
