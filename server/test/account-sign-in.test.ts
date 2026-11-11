import { env } from "cloudflare:test";
import { QrOfferResponse, QrPollResponse, QrRequestResponse, RedeemResponse } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { hashPassword, verifyPassword } from "../src/password/hash";
import { api, createMember } from "./helpers";

const ADMIN = { "x-admin-secret": "test-admin-secret" };

function addr(): string {
  return `p${crypto.randomUUID().slice(0, 8)}@example.com`;
}

function ip(): Record<string, string> {
  return { "cf-connecting-ip": `10.${Math.floor(Math.random() * 250)}.${Math.floor(Math.random() * 250)}.${Math.floor(Math.random() * 250)}` };
}

async function addEmail(token: string, email: string): Promise<void> {
  expect((await api("/v1/me/email/code", { token, body: { email } })).status).toBe(200);
  const r = await api(`/v1/admin/email-outbox?email=${encodeURIComponent(email)}`, { headers: ADMIN });
  const code = (r.body.codes as { purpose: string; code: string }[]).find((c) => c.purpose === "add")!.code;
  expect((await api("/v1/me/email", { token, body: { email, code } })).status).toBe(200);
}

/** Makes `token` look like it signed in long ago, so changes need the current password. */
async function age(token: string): Promise<void> {
  const me = await api("/v1/me/devices", { token });
  const id = (me.body.devices as { id: string; current: boolean }[]).find((d) => d.current)!.id;
  await env.DB.prepare("UPDATE tokens SET created_at = ? WHERE id = ?").bind(Date.now() - 3600_000, id).run();
}

describe("password hash", () => {
  it("is PBKDF2 with a salt and verifies", async () => {
    const a = await hashPassword("hunter2hunter2");
    const b = await hashPassword("hunter2hunter2");
    expect(a).toMatch(/^pbkdf2-sha256\$100000\$[A-Za-z0-9_-]{22}\$[A-Za-z0-9_-]{43}$/);
    expect(a).not.toBe(b);
    expect(await verifyPassword("hunter2hunter2", a)).toBe(true);
    expect(await verifyPassword("hunter2hunter3", a)).toBe(false);
    expect(await verifyPassword("x", "garbage")).toBe(false);
  });
});

describe("password sign-in", () => {
  it("sets a password, signs in with it, changes and removes it", async () => {
    const m = await createMember();
    const email = addr();
    await addEmail(m.token, email);
    expect((await api("/v1/me/password", { token: m.token })).body).toEqual({ set: false, updatedAt: null });

    const set = await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "correct horse 电池" } });
    expect(set.status, JSON.stringify(set.body)).toBe(200);
    expect(set.body.set).toBe(true);
    const row = await env.DB.prepare("SELECT hash FROM member_passwords WHERE member_id = ?").bind(m.id).first<{ hash: string }>();
    expect(row!.hash).not.toContain("correct horse");

    const r = await api("/v1/auth/password/sign-in", { body: { email: email.toUpperCase(), password: "correct horse 电池", deviceName: "iPhone" }, headers: ip() });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const signed = RedeemResponse.parse(r.body);
    expect(signed.member.id).toBe(m.id);
    expect((await api("/v1/me", { token: signed.token })).body.id).toBe(m.id);

    const wrong = await api("/v1/auth/password/sign-in", { body: { email, password: "wrong password", deviceName: "iPhone" }, headers: ip() });
    expect(wrong.status).toBe(401);
    const unknown = await api("/v1/auth/password/sign-in", { body: { email: addr(), password: "whatever1", deviceName: "iPhone" }, headers: ip() });
    expect(unknown.status).toBe(401);

    // A token that signed in long ago needs the current password.
    await age(m.token);
    const noCurrent = await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "a new password" } });
    expect(noCurrent.status).toBe(400);
    expect(noCurrent.body.error.reason).toBe("password.current");
    const badCurrent = await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "a new password", currentPassword: "nope nope" } });
    expect(badCurrent.status).toBe(400);
    const changed = await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "a new password", currentPassword: "correct horse 电池" } });
    expect(changed.status).toBe(200);

    // The fresh token from the sign-in may change it without the current one.
    const fresh = await api("/v1/me/password/remove", { token: signed.token, body: {} });
    expect(fresh.status, JSON.stringify(fresh.body)).toBe(200);
    expect(fresh.body).toEqual({ set: false, updatedAt: null });
    const gone = await api("/v1/auth/password/sign-in", { body: { email, password: "a new password", deviceName: "iPhone" }, headers: ip() });
    expect(gone.status).toBe(401);

    const audit = await env.DB.prepare("SELECT action FROM audit WHERE target = ?").bind(m.id).all<{ action: string }>();
    expect(audit.results.map((a) => a.action)).toEqual(expect.arrayContaining(["password.set", "password.sign_in", "password.remove"]));
    const activity = await env.DB.prepare("SELECT kind FROM activity WHERE action = 'password.sign_in' AND target = ?").bind(m.id).first<{ kind: string }>();
    expect(activity?.kind).toBe("login");
  });

  it("refuses short passwords and bots", async () => {
    const m = await createMember();
    const short = await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "1234567" } });
    expect(short.status).toBe(400);
    const bot = await createMember({ kind: "bot" });
    const r = await api("/v1/me/password", { method: "PUT", token: bot.token, body: { password: "12345678" } });
    expect(r.status).toBe(403);
  });

  it("limits wrong tries per address", async () => {
    const m = await createMember();
    const email = addr();
    await addEmail(m.token, email);
    await api("/v1/me/password", { method: "PUT", token: m.token, body: { password: "right password" } });
    for (let i = 0; i < 10; i++) {
      const r = await api("/v1/auth/password/sign-in", { body: { email, password: `wrong ${i} pass`, deviceName: "x" }, headers: ip() });
      expect(r.status).toBe(401);
    }
    const r = await api("/v1/auth/password/sign-in", { body: { email, password: "right password", deviceName: "x" }, headers: ip() });
    expect(r.status).toBe(429);
  });
});

describe("code sign-in", () => {
  it("offer: a new device claims the code once; the offerer sees it used", async () => {
    const m = await createMember();
    const o = await api("/v1/me/qr/offer", { token: m.token, body: {} });
    expect(o.status, JSON.stringify(o.body)).toBe(200);
    const offer = QrOfferResponse.parse(o.body);
    expect(offer.code).toMatch(/^[0-9A-Z]{4}-[0-9A-Z]{4}$/);
    expect((await api(`/v1/me/qr/offers/${offer.id}`, { token: m.token })).body).toEqual({ state: "waiting" });

    const claim = await api("/v1/auth/qr/claim", { body: { code: offer.code.toLowerCase().replace("-", " "), deviceName: "Joye 的 iPhone" }, headers: ip() });
    expect(claim.status, JSON.stringify(claim.body)).toBe(200);
    const signed = RedeemResponse.parse(claim.body);
    expect(signed.member.id).toBe(m.id);
    expect((await api(`/v1/me/qr/offers/${offer.id}`, { token: m.token })).body).toEqual({ state: "used", deviceName: "Joye 的 iPhone" });

    const again = await api("/v1/auth/qr/claim", { body: { code: offer.code, deviceName: "x" }, headers: ip() });
    expect(again.status).toBe(401);
    const other = await createMember();
    expect((await api(`/v1/me/qr/offers/${offer.id}`, { token: other.token })).status).toBe(404);
    const devices = await api("/v1/me/devices", { token: m.token });
    expect((devices.body.devices as { name: string }[]).map((d) => d.name)).toContain("Joye 的 iPhone");
  });

  it("request: a signed-in person peeks and approves; the new device gets the token once", async () => {
    const m = await createMember();
    const q = await api("/v1/auth/qr/request", { body: { deviceName: "Joye 的 MacBook" }, headers: ip() });
    expect(q.status).toBe(200);
    const req = QrRequestResponse.parse(q.body);
    const poll = () => api("/v1/auth/qr/poll", { body: { id: req.id, secret: req.secret }, headers: ip() });
    expect((await poll()).body).toEqual({ state: "waiting" });
    expect((await api("/v1/auth/qr/poll", { body: { id: req.id, secret: "wrong" }, headers: ip() })).body).toEqual({ state: "expired" });

    const peek = await api("/v1/me/qr/peek", { token: m.token, body: { code: req.code }, headers: ip() });
    expect(peek.body).toEqual({ deviceName: "Joye 的 MacBook", expiresAt: req.expiresAt });
    const ok = await api("/v1/me/qr/approve", { token: m.token, body: { code: req.code }, headers: ip() });
    expect(ok.status, JSON.stringify(ok.body)).toBe(200);
    expect((await api("/v1/me/qr/approve", { token: m.token, body: { code: req.code }, headers: ip() })).status).toBe(404);

    const done = QrPollResponse.parse((await poll()).body);
    expect(done.state).toBe("approved");
    expect(done.member!.id).toBe(m.id);
    expect((await api("/v1/me", { token: done.token! })).body.id).toBe(m.id);
    expect((await poll()).body).toEqual({ state: "expired" });
    const audit = await env.DB.prepare("SELECT action FROM audit WHERE target = ?").bind(m.id).all<{ action: string }>();
    expect(audit.results.map((a) => a.action)).toEqual(expect.arrayContaining(["qr.approve", "qr.sign_in"]));
  });

  it("refuses expired codes and wrong codes, and limits tries", async () => {
    const m = await createMember();
    const offer = QrOfferResponse.parse((await api("/v1/me/qr/offer", { token: m.token, body: {} })).body);
    await env.DB.prepare("UPDATE qr_sign_ins SET expires_at = ? WHERE id = ?").bind(Date.now() - 1, offer.id).run();
    expect((await api("/v1/auth/qr/claim", { body: { code: offer.code, deviceName: "x" }, headers: ip() })).status).toBe(401);
    expect((await api(`/v1/me/qr/offers/${offer.id}`, { token: m.token })).body).toEqual({ state: "expired" });
    expect((await api("/v1/me/qr/peek", { token: m.token, body: { code: "0000-0000" }, headers: ip() })).status).toBe(404);

    const headers = ip();
    for (let i = 0; i < 10; i++) {
      expect((await api("/v1/auth/qr/claim", { body: { code: `ZZZZ-ZZZ${i}`, deviceName: "x" }, headers })).status).toBe(401);
    }
    expect((await api("/v1/auth/qr/claim", { body: { code: "ZZZZ-ZZZZ", deviceName: "x" }, headers })).status).toBe(429);
  });

  it("signing a device out stops its token at once", async () => {
    const m = await createMember();
    const offer = QrOfferResponse.parse((await api("/v1/me/qr/offer", { token: m.token, body: {} })).body);
    const signed = RedeemResponse.parse((await api("/v1/auth/qr/claim", { body: { code: offer.code, deviceName: "phone" }, headers: ip() })).body);
    const list = await api("/v1/me/devices", { token: m.token });
    const phone = (list.body.devices as { id: string; name: string }[]).find((d) => d.name === "phone")!;
    expect((await api(`/v1/me/devices/${phone.id}`, { method: "DELETE", token: m.token })).status).toBe(200);
    expect((await api("/v1/me", { token: signed.token })).status).toBe(401);
  });
});
