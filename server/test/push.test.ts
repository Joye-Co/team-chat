// Phone push (src/push): devices, the badge, APNs requests to a fake gateway, quick answers from
// a notification over HTTP, and removing devices on sign-out and on APNs 410.
import { env } from "cloudflare:test";
import { PushData, type CardV2 } from "@chat/protocol";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { quickActions } from "../src/push/payload";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil, type TestMember } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const realFetch = globalThis.fetch;
type Push = { path: string; headers: Record<string, string>; body: any };
let pushes: Push[] = [];
let answer: (p: Push) => Response = () => new Response(null, { status: 200 });

beforeEach(() => {
  pushes = [];
  answer = () => new Response(null, { status: 200 });
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    if (!req.url.startsWith("https://apns.test/")) return realFetch(input as RequestInfo, init);
    const p: Push = { path: new URL(req.url).pathname, headers: Object.fromEntries(req.headers), body: await req.json() };
    pushes.push(p);
    return answer(p);
  });
});
afterEach(() => vi.restoreAllMocks());

const hexToken = (seed: string) => [...seed.padEnd(32, "0")].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join("").slice(0, 64);

async function register(m: TestMember, token = hexToken(m.id), environment = "production") {
  const r = await api("/v1/me/push-devices", { token: m.token, body: { platform: "apns", token, appId: "dev.example.chat", environment } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return { device: r.body, token };
}

async function verifyJwt(jwt: string): Promise<{ head: any; claims: any }> {
  const [h, c, s] = jwt.split(".");
  const dec = (x: string) => Uint8Array.from(atob(x.replace(/-/g, "+").replace(/_/g, "/")), (ch) => ch.charCodeAt(0));
  const pem = env.APNS_TEST_PUBLIC_KEY.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
  const key = await crypto.subtle.importKey("spki", dec(pem), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
  const ok = await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, key, dec(s), new TextEncoder().encode(`${h}.${c}`));
  expect(ok).toBe(true);
  return { head: JSON.parse(new TextDecoder().decode(dec(h))), claims: JSON.parse(new TextDecoder().decode(dec(c))) };
}

const askCard = (from: string[], extra: Partial<CardV2> = {}): CardV2 =>
  ({
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "ask",
    title: "今天发版吗？",
    fallback: "今天发版吗？",
    ask: { from },
    blocks: [{ id: "info", type: "text", text: "改动都测过了。" }],
    actions: [
      { id: "ok", label: "发", style: "primary" },
      { id: "no", label: "先不发", kind: "reject" },
      { id: "wipe", label: "清空", style: "danger" },
    ],
    ...extra,
  }) as CardV2;

describe("push devices", () => {
  it("registers, lists, moves a known token to the new sign-in and removes", async () => {
    const alice = await createMember({ name: "Alice" });
    const bob = await createMember({ name: "Bob" });
    const bad = await api("/v1/me/push-devices", { token: alice.token, body: { platform: "apns", token: "XYZ", appId: "dev.example.chat", environment: "production" } });
    expect(bad.status).toBe(400);
    const { device, token } = await register(alice);
    expect(device).toMatchObject({ platform: "apns", appId: "dev.example.chat", environment: "production", current: true });
    expect(JSON.stringify(device)).not.toContain(token);
    const list = await api("/v1/me/push-devices", { token: alice.token });
    expect(list.body.devices).toHaveLength(1);
    // The same phone signs in as Bob: the token moves.
    await register(bob, token, "sandbox");
    expect((await api("/v1/me/push-devices", { token: alice.token })).body.devices).toHaveLength(0);
    const bobs = (await api("/v1/me/push-devices", { token: bob.token })).body.devices;
    expect(bobs).toMatchObject([{ id: device.id, environment: "sandbox", current: true }]);
    expect((await api(`/v1/me/push-devices/${device.id}`, { method: "DELETE", token: alice.token })).status).toBe(404);
    expect((await api("/v1/me/push-devices", { method: "DELETE", token: bob.token })).body).toEqual({ removed: 1 });
    expect((await api("/v1/me/push-devices", { token: bob.token })).body.devices).toHaveLength(0);
  });
});

describe("pushes", () => {
  it("sends a mention to the phone with a signed request, and nothing while that sign-in is connected", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const gid = await createGroup(owner, [alice.id], "发布");
    const { token } = await register(alice);
    const co = (await Client.connect(owner.token)).c;
    const sent = await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Alice 看一下这个", mentions: [alice.id] });
    await waitUntil(() => pushes.length >= 1, 5000, "mention push");
    const p = pushes[0];
    expect(p.path).toBe(`/3/device/${token}`);
    expect(p.headers).toMatchObject({ "apns-topic": "dev.example.chat", "apns-push-type": "alert", "apns-priority": "10", "apns-collapse-id": sent.topic.id });
    const { head, claims } = await verifyJwt(p.headers.authorization.replace(/^bearer /, ""));
    expect(head).toEqual({ alg: "ES256", kid: "TESTKEY001" });
    expect(claims.iss).toBe("TESTTEAM01");
    expect(p.body.aps).toMatchObject({ alert: { title: "发布", body: "Joye：@Alice 看一下这个" }, badge: 0, sound: "default", "thread-id": sent.topic.id, "mutable-content": 1 });
    expect(p.body.aps.category).toBeUndefined();
    expect(PushData.parse(p.body.tc)).toMatchObject({ v: 1, reason: "mention", notifyEvent: "mention", groupId: gid, messageId: sent.message.id });

    // Alice's phone is in front (its own sign-in is connected): no push.
    const ca = (await Client.connect(alice.token)).c;
    pushes = [];
    await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Alice 还在吗", mentions: [alice.id] });
    await ca.waitEvent((e) => e.type === "message.created" && e.data.message.body === "@Alice 还在吗");
    await new Promise((r) => setTimeout(r, 300));
    expect(pushes).toHaveLength(0);
    ca.close();
    co.close();
  });

  it("offers the safe answers of a waiting ask, answers over HTTP and clears the badge", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const bot = await createMember({ kind: "bot", name: "Ledger" });
    const gid = await createGroup(owner, [alice.id, bot.id], "发布");
    await register(alice);
    const cbot = (await Client.connect(bot.token, { features: ["interact/1"] })).c;
    const m = (await cbot.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: askCard([alice.id]) })).message;
    const waiting = await waitUntil(() => pushes.find((p) => p.body.tc?.reason === "waiting"), 5000, "waiting push");
    expect(waiting.body.aps.badge).toBe(1);
    expect(waiting.body.aps.category).toBe(`tc.ask.${m.id}.1`);
    expect(waiting.body.aps.alert.body).toContain("等你回复");
    expect(waiting.body.tc.ask).toEqual({ messageId: m.id, rev: 1, actions: [{ id: "ok", label: "发" }, { id: "no", label: "先不发" }] });
    expect((await api("/v1/me/badge", { token: alice.token })).body).toEqual({ waiting: 1 });

    // Answer from the notification: a plain HTTP request, no socket.
    pushes = [];
    const ans = await api(`/v1/groups/${gid}/messages/${m.id}/answer`, { token: alice.token, body: { rev: 1, actionId: "ok", clientEventId: "push-1" } });
    expect(ans.status, JSON.stringify(ans.body)).toBe(200);
    expect(ans.body).toMatchObject({ stored: true });
    const again = await api(`/v1/groups/${gid}/messages/${m.id}/answer`, { token: alice.token, body: { rev: 1, actionId: "ok", clientEventId: "push-1" } });
    expect(again.body).toMatchObject({ duplicate: true });
    const badge = await waitUntil(() => pushes.find((p) => !p.body.tc), 5000, "badge push");
    expect(badge.body).toEqual({ aps: { badge: 0 } });
    expect(badge.headers["apns-priority"]).toBe("5");
    expect((await api("/v1/me/badge", { token: alice.token })).body).toEqual({ waiting: 0 });
    // Not a member: forbidden.
    const bob = await createMember({ name: "Bob" });
    expect((await api(`/v1/groups/${gid}/messages/${m.id}/answer`, { token: bob.token, body: { rev: 1, actionId: "ok" } })).status).toBe(403);
    cbot.close();
  });

  it("only opens the app for an irreversible ask", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const bot = await createMember({ kind: "bot", name: "Ledger" });
    const gid = await createGroup(owner, [alice.id, bot.id]);
    await register(alice);
    const cbot = (await Client.connect(bot.token, { features: ["interact/1"] })).c;
    const card = askCard([alice.id]);
    card.ask = { from: [alice.id], irreversible: { verb: "发布", target: "1.4 到正式环境", consequence: "用户马上看到" } };
    await cbot.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: card });
    const waiting = await waitUntil(() => pushes.find((p) => p.body.tc?.reason === "waiting"), 5000, "waiting push");
    expect(waiting.body.tc.ask).toBeUndefined();
    expect(waiting.body.aps.category).toBeUndefined();
    cbot.close();
  });

  it("removes a device on APNs 410 and the devices of a revoked sign-in", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const gid = await createGroup(owner, [alice.id]);
    await register(alice);
    answer = () => Response.json({ reason: "Unregistered" }, { status: 410 });
    const co = (await Client.connect(owner.token)).c;
    await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Alice 1", mentions: [alice.id] });
    await waitUntil(() => pushes.length >= 1, 5000, "push");
    const count = async () => (await env.DB.prepare("SELECT count(*) AS n FROM push_devices WHERE member_id = ?").bind(alice.id).first<{ n: number }>())?.n;
    for (let i = 0; i < 50 && (await count()) !== 0; i++) await new Promise((r) => setTimeout(r, 50));
    expect(await count()).toBe(0);

    // A new device, then the sign-in is revoked: no more pushes and the device is gone.
    answer = () => new Response(null, { status: 200 });
    await register(alice, hexToken("second-phone"));
    await env.DB.prepare("UPDATE tokens SET revoked_at = ? WHERE id = ?").bind(Date.now(), alice.tokenId).run();
    pushes = [];
    await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@Alice 2", mentions: [alice.id] });
    await new Promise((r) => setTimeout(r, 500));
    expect(pushes).toHaveLength(0);
    expect(await count()).toBe(0);
    co.close();
  });
});

describe("quick actions", () => {
  it("keeps only answers that need no input and can be taken back", () => {
    const card = askCard(["x"]);
    expect(quickActions(card)).toEqual([{ id: "ok", label: "发" }, { id: "no", label: "先不发" }]);
    expect(quickActions({ ...card, ask: undefined })).toEqual([]);
    expect(quickActions({ ...card, blocks: [{ id: "d", type: "decision", options: [] }] })).toEqual([]);
    expect(quickActions({ ...card, actions: [{ id: "a", label: "批准", role: "approve" }, { id: "b", label: "好", confirm: { title: "确定？" } }, { id: "c", label: "看", kind: "link" }] })).toEqual([]);
  });
});
