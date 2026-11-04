// 1.1 server-files gaps (GAPS.md SF-*): media file limits, multipart uploads, stored page
// events, desk notes, queued runs, aside origins, PR merge state, group purpose, rename order
// and 产出 previews.
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(() => {
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
