// What a refusal tells its reader: which field, which limit, how long to wait, which secret.
import { SELF, env } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { requireAdmin } from "../src/core/auth";
import type { Env } from "../src/core/env";
import { ApiError, unwrap } from "../src/core/errors";
import { errorResponse } from "../src/core/http";
import { type HooksEnv, decryptSecret, encryptSecret } from "../src/integrations/secrets";
import { Daemon, world } from "./daemon";
import { BASE, api, createGroup, createMember } from "./helpers";

async function thrown(fn: () => Promise<unknown>): Promise<ApiError> {
  try {
    await fn();
  } catch (err) {
    if (err instanceof ApiError) return err;
    throw err;
  }
  throw new Error("nothing was thrown");
}

describe("machine link", () => {
  it("names the frame type and the field of a frame that fails its schema", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken);
    d.send({ t: "run.event", runId: "r1", seq: "not a number", at: Date.now(), event: { type: "text", text: "secret words" } });
    const err = await d.next((f) => f.t === "error");
    expect(err.error.code).toBe("invalid");
    expect(err.error.message).toMatch(/^invalid frame run\.event: seq /);
    expect(err.error.message).not.toContain("secret words");
    d.send({ no: "type" });
    const second = await d.next((f) => f.t === "error" && f !== err);
    expect(second.error.message).toMatch(/^invalid frame: /);
    d.close();
  });
});

describe("wrong method and bad query", () => {
  it("answers 405 with the Allow header for a path under another method", async () => {
    const res = await SELF.fetch(`${BASE}/v1/auth/redeem`);
    expect(res.status).toBe(405);
    expect(res.headers.get("allow")).toBe("POST");
    expect(await res.json()).toEqual({ error: { code: "invalid", message: "method not allowed: use POST" } });
    const ws = await SELF.fetch(`${BASE}/v1/ws`, { method: "POST" });
    expect(ws.status).toBe(405);
    expect(ws.headers.get("allow")).toBe("GET");
    expect((await api("/v1/no-such-route")).status).toBe(404);
  });

  it("names the query parameter that broke its rule", async () => {
    const m = await createMember();
    const gid = await createGroup(m, []);
    const decisions = await api(`/v1/groups/${gid}/decisions?limit=0`, { token: m.token });
    expect(decisions.status).toBe(400);
    expect(decisions.body.error.message).toMatch(/^invalid query: limit /);
    const none = await api("/v1/artifacts", { token: m.token });
    expect(none.body.error.message).toMatch(/^invalid query: groupId /);
    const topic = await api(`/v1/artifacts?groupId=${gid}&topicId=`, { token: m.token });
    expect(topic.body.error.message).toMatch(/^invalid query: topicId /);
  });
});

describe("members", () => {
  it("names the ids that are no active member", async () => {
    const m = await createMember();
    const other = await createMember();
    const r = await api("/v1/groups", { token: m.token, body: { name: "g", memberIds: [other.id, "01NOSUCHMEMBER", "01ALSOMISSING"] } });
    expect(r.status).toBe(400);
    expect(r.body.error).toEqual({
      code: "invalid",
      message: "unknown or deactivated member: 01NOSUCHMEMBER (and 1 more)",
      reason: "member.unknown",
      memberIds: ["01NOSUCHMEMBER", "01ALSOMISSING"],
    });
  });
});

describe("rate limits", () => {
  it("says which sign-in limit was hit and how long to wait", async () => {
    const headers = { "cf-connecting-ip": "198.51.100.77", "content-type": "application/json" };
    let res: Response | null = null;
    for (let i = 0; i < 11; i++) {
      res = await SELF.fetch(`${BASE}/v1/auth/redeem`, { method: "POST", headers, body: JSON.stringify({ inviteCode: "0000-0000", deviceName: "mac" }) });
      if (i < 10) await res.text();
    }
    expect(res?.status).toBe(429);
    const body = (await res?.json()) as { error: { code: string; message: string; retryAfterMs: number } };
    expect(body.error.code).toBe("rate_limited");
    expect(body.error.message).toMatch(/^too many invite code attempts, try again in \d+ s$/);
    expect(body.error.retryAfterMs).toBeGreaterThan(0);
    expect(body.error.retryAfterMs).toBeLessThanOrEqual(60_000);
    expect(Number(res?.headers.get("retry-after"))).toBe(Math.ceil(body.error.retryAfterMs / 1000));
  });

  it("carries the wait of a post limit over HTTP", async () => {
    const m = await createMember();
    const gid = await createGroup(m, []);
    const post = () =>
      SELF.fetch(`${BASE}/v1/groups/${gid}/messages`, {
        method: "POST",
        headers: { authorization: `Bearer ${m.token}`, "content-type": "application/json" },
        body: "{}",
      });
    const all = await Promise.all(Array.from({ length: 80 }, post));
    const limited = all.filter((r) => r.status === 429);
    expect(limited.length).toBeGreaterThan(0);
    const body = (await limited[0].json()) as { error: { message: string; retryAfterMs: number } };
    expect(body.error.message).toBe("too many posts");
    expect(body.error.retryAfterMs).toBeGreaterThanOrEqual(0);
    expect(limited[0].headers.get("retry-after")).toBe(String(Math.ceil(body.error.retryAfterMs / 1000)));
    for (const r of all) if (r !== limited[0]) await r.text();
  });

  it("keeps the wait of an error that crossed a Durable Object", () => {
    let err: unknown;
    try {
      unwrap({ ok: false, error: { code: "rate_limited", message: "one press per second", retryAfterMs: 400 } });
    } catch (e) {
      err = e;
    }
    expect((err as ApiError).retryAfterMs).toBe(400);
    const res = errorResponse(err);
    expect(res.status).toBe(429);
    expect(res.headers.get("retry-after")).toBe("1");
    expect(errorResponse(new ApiError("invalid", "x")).headers.get("retry-after")).toBeNull();
  });
});

describe("secrets that are not set", () => {
  it("tells a missing ADMIN_SECRET from a wrong one", async () => {
    const wrong = await api("/v1/admin/bootstrap", { headers: { "x-admin-secret": "nope" }, body: { ownerName: "x" } });
    expect(wrong.status).toBe(401);
    expect(wrong.body.error).toEqual({ code: "unauthorized", message: "invalid admin secret" });
    const request = new Request(`${BASE}/v1/admin/bootstrap`, { headers: { "x-admin-secret": "anything" } });
    const missing = await thrown(() => requireAdmin(request, { ...env, ADMIN_SECRET: undefined } as unknown as Env));
    expect(missing.code).toBe("unauthorized");
    expect(missing.reason).toBe("admin.secret_missing");
    expect(missing.message).toContain("ADMIN_SECRET is not set");
    await requireAdmin(new Request(BASE, { headers: { "x-admin-secret": "test-admin-secret" } }), env as unknown as Env);
  });

  it("names HOOK_SECRET_KEY when hook secrets cannot be written or read", async () => {
    const hooksEnv = env as unknown as HooksEnv;
    const missing = await thrown(() => encryptSecret({ ...hooksEnv, HOOK_SECRET_KEY: undefined }, "h1", "whk_x"));
    expect(missing.code).toBe("internal");
    expect(missing.reason).toBe("hooks.secret_missing");
    expect(missing.message).toContain("wrangler secret put HOOK_SECRET_KEY");
    const stored = await encryptSecret(hooksEnv, "h1", "whk_x");
    expect(await decryptSecret(hooksEnv, "h1", stored)).toBe("whk_x");
    const rotated = await thrown(() => decryptSecret({ ...hooksEnv, HOOK_SECRET_KEY: "another-secret-key-0123456789" }, "h1", stored));
    expect(rotated.reason).toBe("hooks.secret_unreadable");
    expect((await thrown(() => decryptSecret(hooksEnv, "h1", "garbage"))).reason).toBe("hooks.secret_unreadable");
    expect((await thrown(() => decryptSecret({ ...hooksEnv, HOOK_SECRET_KEY: "" }, "h1", stored))).reason).toBe("hooks.secret_missing");
  });

  it("answers an inbound post to an unreadable hook with the reason", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const created = await api("/v1/hooks", { token: owner.token, body: { name: "Deploys", groupId: gid, kind: "generic" } });
    expect(created.status, JSON.stringify(created.body)).toBe(200);
    await env.DB.prepare("UPDATE hooks SET secret_enc = 'v1.AAAA.BBBB' WHERE id = ?").bind(created.body.hook.id).run();
    const r = await api(`/v1/hooks/${created.body.hook.id}/in`, { headers: { "x-hook-secret": created.body.secret }, body: { body: "hi" } });
    expect(r.status).toBe(500);
    expect(r.body.error.code).toBe("internal");
    expect(r.body.error.reason).toBe("hooks.secret_unreadable");
  });
});
