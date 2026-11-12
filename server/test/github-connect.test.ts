// A person connects their own GitHub (src/github, design-github-identity G2): the device code
// sign-in the server starts and polls, encrypted storage, status, disconnect and the cleanup when
// a member is deactivated. GitHub is a fake fetch; the real one is never called.
import { env } from "cloudflare:test";
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { loadToken } from "../src/github";
import { unavailableReason } from "../src/github/store";
import { Client, api, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const realFetch = globalThis.fetch;

type Fake = {
  /** Answers to `POST /login/oauth/access_token`, in order; the last one repeats. */
  token: Record<string, unknown>[];
  user: { status: number; body?: unknown };
  seen: { url: string; body: string; auth: string | null }[];
};

let tokenN = 0;
/** A made-up token in GitHub's OAuth format. */
function fakeToken(): string {
  tokenN++;
  return `gho_${"T".repeat(30)}${String(tokenN).padStart(6, "0")}`;
}

function fakeGithub(over: Partial<Fake> = {}): Fake {
  const f: Fake = {
    token: [{ error: "authorization_pending" }],
    user: { status: 200, body: { id: 4242, login: "Octo-Cat", name: "Octo Cat" } },
    seen: [],
    ...over,
  };
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    const u = new URL(req.url);
    if (u.host !== "github.com" && u.host !== "api.github.com") return realFetch(input as RequestInfo, init);
    const body = req.method === "GET" ? "" : await req.text();
    f.seen.push({ url: req.url, body, auth: req.headers.get("authorization") });
    if (u.pathname === "/login/device/code") {
      return Response.json({ device_code: "dev-code-secret-123", user_code: "ABCD-1234", verification_uri: "https://github.com/login/device", expires_in: 899, interval: 5 });
    }
    if (u.pathname === "/login/oauth/access_token") {
      const next = f.token.length > 1 ? f.token.shift()! : f.token[0];
      return Response.json(next);
    }
    if (u.pathname === "/user") return Response.json(f.user.body ?? { message: "Bad credentials" }, { status: f.user.status });
    return new Response("not found", { status: 404 });
  });
  return f;
}

afterEach(() => {
  vi.restoreAllMocks();
});

async function startDevice(token: string): Promise<string> {
  const r = await api("/v1/me/github/device", { method: "POST", token });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return r.body.deviceId as string;
}

const poll = (token: string, id: string) => api(`/v1/me/github/device/${id}/poll`, { method: "POST", token });

describe("github availability", () => {
  it("names the missing piece", () => {
    const key = "a-secret-key-of-enough-length";
    // product.json ships no client id, so without the Worker variable there is none.
    expect(unavailableReason({ HOOK_SECRET_KEY: key } as never)).toBe("no_client_id");
    expect(unavailableReason({ GITHUB_CLIENT_ID: "  ", HOOK_SECRET_KEY: key } as never)).toBe("no_client_id");
    expect(unavailableReason({ GITHUB_CLIENT_ID: "abc" } as never)).toBe("no_secret_key");
    expect(unavailableReason({ GITHUB_CLIENT_ID: "abc", HOOK_SECRET_KEY: "short" } as never)).toBe("no_secret_key");
    expect(unavailableReason({ GITHUB_CLIENT_ID: "abc", HOOK_SECRET_KEY: key } as never)).toBeNull();
  });

  it("status says available, not connected, nothing waits", async () => {
    const m = await createMember();
    const r = await api("/v1/me/github", { token: m.token });
    expect(r.status).toBe(200);
    expect(r.body).toEqual({ available: true, waits: [] });
  });

  it("is for people only", async () => {
    const bot = await createMember({ kind: "bot" });
    expect((await api("/v1/me/github", { token: bot.token })).status).toBe(403);
    expect((await api("/v1/me/github/device", { method: "POST", token: bot.token })).status).toBe(403);
  });
});

describe("github device sign-in", () => {
  it("starts on the server and keeps the device code encrypted", async () => {
    const gh = fakeGithub();
    const m = await createMember();
    const r = await api("/v1/me/github/device", { method: "POST", token: m.token });
    expect(r.status).toBe(200);
    expect(r.body).toMatchObject({ userCode: "ABCD-1234", verificationUri: "https://github.com/login/device", interval: 5 });
    expect(r.body.expiresAt).toBeGreaterThan(Date.now());
    expect(r.body.expiresAt).toBeLessThanOrEqual(Date.now() + 15 * 60_000);
    expect(JSON.stringify(r.body)).not.toContain("dev-code-secret-123");
    expect(JSON.parse(gh.seen[0].body)).toEqual({ client_id: "test-github-client", scope: "repo read:org workflow" });
    const row = await env.DB.prepare("SELECT * FROM github_device WHERE id = ?").bind(r.body.deviceId).first<any>();
    expect(row.member_id).toBe(m.id);
    expect(row.device_enc.startsWith("v1.")).toBe(true);
    expect(row.device_enc).not.toContain("dev-code-secret-123");

    // A second start replaces the first.
    const again = await startDevice(m.token);
    const rows = await env.DB.prepare("SELECT id FROM github_device WHERE member_id = ?").bind(m.id).all<{ id: string }>();
    expect(rows.results.map((x) => x.id)).toEqual([again]);
  });

  it("pending, then slow_down with the new interval", async () => {
    fakeGithub({ token: [{ error: "authorization_pending" }, { error: "slow_down", interval: 10 }] });
    const m = await createMember();
    const id = await startDevice(m.token);
    expect((await poll(m.token, id)).body).toEqual({ state: "pending" });
    expect((await poll(m.token, id)).body).toEqual({ state: "slow_down" });
    const row = await env.DB.prepare("SELECT interval_s FROM github_device WHERE id = ?").bind(id).first<{ interval_s: number }>();
    expect(row?.interval_s).toBe(10);
  });

  it("expired and denied end the sign-in", async () => {
    const m = await createMember();
    fakeGithub({ token: [{ error: "expired_token" }] });
    const a = await startDevice(m.token);
    expect((await poll(m.token, a)).body).toEqual({ state: "expired" });
    expect((await poll(m.token, a)).status).toBe(404);

    vi.restoreAllMocks();
    fakeGithub({ token: [{ error: "access_denied" }] });
    const b = await startDevice(m.token);
    expect((await poll(m.token, b)).body).toEqual({ state: "denied" });
    expect(await env.DB.prepare("SELECT 1 FROM github_device WHERE id = ?").bind(b).first()).toBeNull();
  });

  it("expires after its time without asking GitHub", async () => {
    const gh = fakeGithub();
    const m = await createMember();
    const id = await startDevice(m.token);
    await env.DB.prepare("UPDATE github_device SET expires_at = ? WHERE id = ?").bind(Date.now() - 1, id).run();
    const before = gh.seen.length;
    expect((await poll(m.token, id)).body).toEqual({ state: "expired" });
    expect(gh.seen.length).toBe(before);
  });

  it("only the person who started it can poll it", async () => {
    fakeGithub();
    const m = await createMember();
    const other = await createMember();
    const id = await startDevice(m.token);
    const r = await poll(other.token, id);
    expect(r.status).toBe(404);
    expect(r.body.error.code).toBe("not_found");
  });

  it("a GitHub refusal does not leak what GitHub said", async () => {
    fakeGithub({ token: [{ error: "device_flow_disabled", error_description: "secret-ish detail" }] });
    const m = await createMember();
    const id = await startDevice(m.token);
    const r = await poll(m.token, id);
    expect(r.status).toBe(409);
    expect(r.body.error.reason).toBe("github.refused");
    expect(JSON.stringify(r.body)).not.toContain("secret-ish");
  });

  it("done: saves the token encrypted, writes the login map, pushes github.status", async () => {
    const tok = fakeToken();
    const gh = fakeGithub({ token: [{ error: "authorization_pending" }, { access_token: tok, token_type: "bearer", scope: "repo,read:org,workflow" }] });
    const logs: string[] = [];
    for (const k of ["log", "warn", "error", "info", "debug"] as const) {
      vi.spyOn(console, k).mockImplementation((...args: unknown[]) => {
        logs.push(args.map(String).join(" "));
      });
    }
    const m = await createMember({ name: "Ana" });
    const withFeature = await Client.connect(m.token, { features: ["github/1"] });
    const without = await Client.connect(m.token);

    const id = await startDevice(m.token);
    expect((await poll(m.token, id)).body).toEqual({ state: "pending" });
    const done = await poll(m.token, id);
    expect(done.status).toBe(200);
    expect(done.body.state).toBe("done");
    expect(done.body.account).toMatchObject({ login: "Octo-Cat", name: "Octo Cat", status: "ok" });
    expect(JSON.stringify(done.body)).not.toContain(tok);
    expect(gh.seen.find((s) => s.url.endsWith("/user"))?.auth).toBe(`Bearer ${tok}`);

    const row = await env.DB.prepare("SELECT * FROM github_accounts WHERE member_id = ?").bind(m.id).first<any>();
    expect(row).toMatchObject({ github_user_id: 4242, login: "Octo-Cat", name: "Octo Cat", scopes: "repo,read:org,workflow", status: "ok" });
    expect(row.token_enc.startsWith("v1.")).toBe(true);
    expect(row.token_enc).not.toContain(tok);
    expect(await env.DB.prepare("SELECT 1 FROM github_device WHERE id = ?").bind(id).first()).toBeNull();
    expect(await loadToken(env as never, m.id)).toEqual({ token: tok, login: "Octo-Cat" });

    const map = await env.DB.prepare("SELECT member_id FROM github_logins WHERE login = 'octo-cat'").first<{ member_id: string }>();
    expect(map?.member_id).toBe(m.id);

    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'github.connect' AND target = ?").bind(m.id).first<{ details: string }>();
    expect(JSON.parse(audit!.details)).toEqual({ login: "Octo-Cat", githubUserId: 4242 });
    const allAudit = await env.DB.prepare("SELECT details FROM audit").all<{ details: string }>();
    expect(JSON.stringify(allAudit.results)).not.toContain(tok);

    const pushed = await withFeature.c.next((f) => f.t === "transient" && f.event.type === "github.status");
    expect(pushed.event.account).toMatchObject({ login: "Octo-Cat", status: "ok" });
    await new Promise((r) => setTimeout(r, 100));
    expect(without.c.frames.some((f) => f.t === "transient" && f.event?.type === "github.status")).toBe(false);

    const status = await api("/v1/me/github", { token: m.token });
    expect(status.body).toEqual({ available: true, account: done.body.account, waits: [] });
    expect(logs.join("\n")).not.toContain(tok);
    expect(logs.join("\n")).not.toContain("dev-code-secret-123");
    withFeature.c.close();
    without.c.close();
  });

  it("does not overwrite a login an owner mapped by hand", async () => {
    const other = await createMember();
    await env.DB.prepare("INSERT INTO github_logins (login, member_id, updated_at) VALUES ('hand-set', ?, 1)").bind(other.id).run();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user: { status: 200, body: { id: 5151, login: "Hand-Set", name: null } } });
    const m = await createMember();
    const id = await startDevice(m.token);
    const done = await poll(m.token, id);
    expect(done.body.state).toBe("done");
    expect(done.body.account.name).toBeUndefined();
    const map = await env.DB.prepare("SELECT member_id FROM github_logins WHERE login = 'hand-set'").first<{ member_id: string }>();
    expect(map?.member_id).toBe(other.id);
  });

  it("taken: one GitHub account connects to one member", async () => {
    const user = { status: 200, body: { id: 7777, login: "shared", name: "Shared" } };
    const first = await createMember();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user });
    expect((await poll(first.token, await startDevice(first.token))).body.state).toBe("done");

    vi.restoreAllMocks();
    const second = await createMember();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user });
    const id = await startDevice(second.token);
    expect((await poll(second.token, id)).body).toEqual({ state: "taken" });
    expect(await env.DB.prepare("SELECT 1 FROM github_accounts WHERE member_id = ?").bind(second.id).first()).toBeNull();
    expect(await env.DB.prepare("SELECT 1 FROM github_device WHERE id = ?").bind(id).first()).toBeNull();

    // The same person connecting the same account again is fine.
    vi.restoreAllMocks();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user });
    expect((await poll(first.token, await startDevice(first.token))).body.state).toBe("done");
  });

  it("a token is bound to its member: swapped rows do not decrypt", async () => {
    const a = await createMember();
    const b = await createMember();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user: { status: 200, body: { id: 9001, login: "a-gh" } } });
    await poll(a.token, await startDevice(a.token));
    vi.restoreAllMocks();
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user: { status: 200, body: { id: 9002, login: "b-gh" } } });
    await poll(b.token, await startDevice(b.token));
    const ra = await env.DB.prepare("SELECT token_enc FROM github_accounts WHERE member_id = ?").bind(a.id).first<{ token_enc: string }>();
    await env.DB.prepare("UPDATE github_accounts SET token_enc = ? WHERE member_id = ?").bind(ra!.token_enc, b.id).run();
    await expect(loadToken(env as never, b.id)).rejects.toThrow();
  });
});

describe("github disconnect and deactivation", () => {
  it("disconnect deletes the connection and tells the person's clients", async () => {
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user: { status: 200, body: { id: 3131, login: "leaver" } } });
    const m = await createMember();
    await poll(m.token, await startDevice(m.token));
    const { c } = await Client.connect(m.token, { features: ["github/1"] });
    const r = await api("/v1/me/github", { method: "DELETE", token: m.token });
    expect(r.status).toBe(200);
    expect(r.body).toEqual({ ok: true });
    const pushed = await c.next((f) => f.t === "transient" && f.event.type === "github.status" && !f.event.account);
    expect(pushed.event).toEqual({ type: "github.status" });
    expect(await env.DB.prepare("SELECT 1 FROM github_accounts WHERE member_id = ?").bind(m.id).first()).toBeNull();
    expect((await api("/v1/me/github", { token: m.token })).body).toEqual({ available: true, waits: [] });
    expect(await env.DB.prepare("SELECT 1 FROM audit WHERE action = 'github.disconnect' AND target = ?").bind(m.id).first()).not.toBeNull();
    // The login map stays.
    expect(await env.DB.prepare("SELECT 1 FROM github_logins WHERE login = 'leaver'").first()).not.toBeNull();
    // Disconnecting again is harmless.
    expect((await api("/v1/me/github", { method: "DELETE", token: m.token })).status).toBe(200);
    c.close();
  });

  it("a deactivated member's connection and sign-in in progress are deleted", async () => {
    fakeGithub({ token: [{ access_token: fakeToken(), scope: "repo" }], user: { status: 200, body: { id: 6161, login: "gone" } } });
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    await poll(m.token, await startDevice(m.token));
    vi.restoreAllMocks();
    fakeGithub();
    await startDevice(m.token);
    const r = await api(`/v1/members/${m.id}`, { method: "PATCH", token: owner.token, body: { deactivated: true } });
    expect(r.status).toBe(200);
    await waitUntil(async () => true);
    expect(await env.DB.prepare("SELECT 1 FROM github_accounts WHERE member_id = ?").bind(m.id).first()).toBeNull();
    expect(await env.DB.prepare("SELECT 1 FROM github_device WHERE member_id = ?").bind(m.id).first()).toBeNull();
  });
});
