import { env } from "cloudflare:test";
import { RedeemResponse } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { api, createMember } from "./helpers";

const ADMIN = { "x-admin-secret": "test-admin-secret" };

/** The newest code the local outbox holds for `email` and `purpose`. */
async function lastCode(email: string, purpose: "sign_in" | "add"): Promise<string | undefined> {
  const r = await api(`/v1/admin/email-outbox?email=${encodeURIComponent(email)}`, { headers: ADMIN });
  expect(r.status).toBe(200);
  return (r.body.codes as { purpose: string; code: string }[]).find((c) => c.purpose === purpose)?.code;
}

function addr(): string {
  return `m${crypto.randomUUID().slice(0, 8)}@example.com`;
}

/** Adds `email` as the sign-in email of `token`'s member. */
async function addEmail(token: string, email: string): Promise<void> {
  expect((await api("/v1/me/email/code", { token, body: { email } })).status).toBe(200);
  const code = await lastCode(email, "add");
  const r = await api("/v1/me/email", { token, body: { email: email.toUpperCase(), code } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  expect(r.body).toEqual({ email });
}

describe("email sign-in", () => {
  it("adds an email, then signs in on a new device with a code", async () => {
    const m = await createMember();
    const email = addr();
    expect((await api("/v1/me/email", { token: m.token })).body).toEqual({ email: null });
    await addEmail(m.token, email);
    expect((await api("/v1/me/email", { token: m.token })).body).toEqual({ email });

    const sent = await api("/v1/auth/email/code", { body: { email: ` ${email} ` } });
    expect(sent.status).toBe(200);
    expect(sent.body.sent).toBe(true);
    const code = await lastCode(email, "sign_in");
    expect(code).toMatch(/^[0-9]{6}$/);
    const r = await api("/v1/auth/email/sign-in", { body: { email, code, deviceName: "新的 Mac" } });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
    const signed = RedeemResponse.parse(r.body);
    expect(signed.member.id).toBe(m.id);
    expect((await api("/v1/me", { token: signed.token })).body.id).toBe(m.id);
    const audit = await env.DB.prepare("SELECT action FROM audit WHERE target = ? ORDER BY id").bind(m.id).all<{ action: string }>();
    expect(audit.results.map((a) => a.action)).toEqual(expect.arrayContaining(["email.set", "email.sign_in"]));

    // Single use.
    const again = await api("/v1/auth/email/sign-in", { body: { email, code, deviceName: "Mac" } });
    expect(again.status).toBe(401);
  });

  it("answers the same for an unknown address and sends nothing", async () => {
    const email = addr();
    const r = await api("/v1/auth/email/code", { body: { email } });
    expect(r.status).toBe(200);
    expect(r.body.sent).toBe(true);
    expect(await lastCode(email, "sign_in")).toBeUndefined();
  });

  it("stops a code after five wrong tries", async () => {
    const m = await createMember();
    const email = addr();
    await addEmail(m.token, email);
    await api("/v1/auth/email/code", { body: { email } });
    const code = (await lastCode(email, "sign_in"))!;
    const wrong = code === "000000" ? "111111" : "000000";
    for (let i = 0; i < 5; i++) {
      const r = await api("/v1/auth/email/sign-in", { body: { email, code: wrong, deviceName: "Mac" }, headers: { "cf-connecting-ip": `10.0.0.${i}` } });
      expect(r.status).toBe(401);
    }
    const right = await api("/v1/auth/email/sign-in", { body: { email, code, deviceName: "Mac" } });
    expect(right.status).toBe(401);
  });

  it("refuses an email another member signs in with, and a deactivated member", async () => {
    const a = await createMember();
    const b = await createMember();
    const email = addr();
    await addEmail(a.token, email);
    const taken = await api("/v1/me/email/code", { token: b.token, body: { email } });
    expect(taken.status).toBe(409);

    await env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(a.id).run();
    await api("/v1/auth/email/code", { body: { email } });
    expect(await lastCode(email, "sign_in")).toBeUndefined();
  });

  it("removes my email, and refuses bad bodies and bots", async () => {
    const m = await createMember();
    const email = addr();
    await addEmail(m.token, email);
    expect((await api("/v1/me/email", { token: m.token, method: "DELETE" })).body).toEqual({ email: null });
    await api("/v1/auth/email/code", { body: { email } });
    expect(await lastCode(email, "sign_in")).toBeUndefined();

    expect((await api("/v1/auth/email/code", { body: { email: "nope" } })).status).toBe(400);
    expect((await api("/v1/auth/email/sign-in", { body: { email, code: "12", deviceName: "Mac" } })).status).toBe(400);
    const bot = await createMember({ kind: "bot" });
    expect((await api("/v1/me/email/code", { token: bot.token, body: { email: addr() } })).status).toBe(403);
    expect((await api(`/v1/admin/email-outbox?email=${email}`)).status).toBe(401);
  });
});
