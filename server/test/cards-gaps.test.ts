// 1.1.0 gap fixes of the cards area: files and 产出 a card names, draft content hashes,
// reserved 产出 paths, anonymous counts, refs from an aside topic, recorded decisions, member
// card status, shape reasons, catalogs per socket and frame sources.
import { SELF, env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const FEATURES = ["interact/1", "refs/1"];

export const base = (extra: Partial<CardV2>): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "info",
  title: "卡片",
  fallback: "卡片",
  blocks: [],
  ...extra,
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function upload(m: TestMember, groupId: string, name = "a.png", mime = "image/png", text = "png"): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: await sha(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

async function world() {
  const owner = await createMember({ level: "owner", name: "Joye" });
  const alice = await createMember({ name: "Alice" });
  const bob = await createMember({ name: "Bob" });
  const bot = await createMember({ kind: "bot", name: "Ledger" });
  const gid = await createGroup(owner, [alice.id, bob.id, bot.id]);
  const connect = async (m: TestMember, features: string[] = FEATURES) => (await Client.connect(m.token, { features })).c;
  return { owner, alice, bob, bot, gid, connect };
}

const send = (c: Client, gid: string, cardV2: CardV2, extra: Record<string, unknown> = {}) =>
  c.request({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2, ...extra });

describe("files and 产出 a card names (PC-2)", () => {
  it("refuses a file or 产出 of another group and takes one of this group", async () => {
    const w = await world();
    const other = await createGroup(w.owner, [w.bot.id]);
    const mine = await upload(w.bot, w.gid);
    const theirs = await upload(w.bot, other);
    const cbot = await w.connect(w.bot);
    const image = (fileId: string) => base({ blocks: [{ id: "img", type: "image", fileId }] });
    const bad = await send(cbot, w.gid, image(theirs));
    expect(bad, JSON.stringify(bad)).toMatchObject({ ok: false, error: { code: "invalid", reason: "card.limits" } });
    expect(bad.error.message).toContain("file not in group");
    const choice = base({
      intent: "ask",
      ask: { from: [w.alice.id] },
      blocks: [
        {
          id: "pick",
          type: "imageChoice",
          label: "哪个？",
          fallbackText: "选图",
          options: [
            { id: "a", label: "A", fileId: mine },
            { id: "b", label: "B", fileId: theirs },
          ],
        },
      ],
    });
    expect(await send(cbot, w.gid, choice)).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    const evidence = base({ blocks: [{ id: "e", type: "evidence", label: "截图", fileId: theirs }] });
    expect(await send(cbot, w.gid, evidence)).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    const ref = base({ blocks: [{ id: "r", type: "ref", label: "页面", target: { kind: "artifact", artifactId: "01JBX0ART0000000000000000Z" } }] });
    const r = await send(cbot, w.gid, ref);
    expect(r).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    expect(r.error.message).toContain("产出 not in group");
    const ok = await send(cbot, w.gid, image(mine));
    expect(ok, JSON.stringify(ok)).toMatchObject({ ok: true });
    // A patch that names a file of the other group is refused too.
    const p = await cbot.request({ t: "card.patch", groupId: w.gid, messageId: ok.result.message.id, baseRev: 1, ops: [{ op: "set", id: "img", props: { fileId: theirs } }] });
    expect(p).toMatchObject({ ok: false, error: { reason: "card.limits" } });
    cbot.close();
  });
});
