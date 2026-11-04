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
