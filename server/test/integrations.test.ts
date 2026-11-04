import { env } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { normalize } from "../src/integrations/github";
import { signGithub } from "../src/integrations/secrets";
import {
  checkSuiteEvent,
  issuesEvent,
  pingEvent,
  pullRequestEvent,
  REPO,
  reviewEvent,
  workflowRunEvent,
} from "./github-payloads";
import { BASE, Client, api, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

async function setup(kind: "generic" | "github" = "generic", extra: Record<string, unknown> = {}) {
  const owner = await createMember({ level: "owner", name: "Owner" });
  const reviewer = await createMember({ name: "Reviewer" });
  const gid = await createGroup(owner, [reviewer.id]);
  const created = await api("/v1/hooks", { token: owner.token, body: { name: `${kind} hook`, groupId: gid, kind, ...extra } });
  expect(created.status, JSON.stringify(created.body)).toBe(200);
  const { c } = await Client.connect(owner.token);
  return { owner, reviewer, gid, hook: created.body.hook, url: created.body.url as string, secret: created.body.secret as string, c };
}

async function postIn(hookId: string, secret: string, body: unknown, headers: Record<string, string> = {}) {
  return api(`/v1/hooks/${hookId}/in`, { body, headers: { "x-hook-secret": secret, ...headers } });
}

let deliveries = 0;
async function postGithub(
  hookId: string,
  secret: string,
  event: string,
  payload: unknown,
  opts: { delivery?: string; signature?: string | null } = {},
) {
  const raw = JSON.stringify(payload);
  const headers: Record<string, string> = {
    "content-type": "application/json",
    "x-github-event": event,
    "x-github-delivery": opts.delivery ?? `d-${++deliveries}-${crypto.randomUUID()}`,
    "user-agent": "GitHub-Hookshot/abc123",
  };
  const sig = opts.signature === undefined ? await signGithub(secret, raw) : opts.signature;
  if (sig !== null) headers["x-hub-signature-256"] = sig;
  return api(`/v1/hooks/${hookId}/github`, { raw, headers });
}

async function events(c: Client, gid: string): Promise<any[]> {
  const page = await c.request({ t: "fetch", groupId: gid, afterSeq: 0, limit: 500 });
  expect(page.t).toBe("page");
  return page.events;
}

/** Topic state rebuilt from the group log: the latest snapshot plus topic.updated changes. */
async function topics(c: Client, gid: string): Promise<Map<string, any>> {
  const map = new Map<string, any>();
  for (const e of await events(c, gid)) {
    if (e.type === "topic.created") map.set(e.data.topic.id, { ...e.data.topic });
    if (e.type === "topic.updated") Object.assign(map.get(e.data.topicId), e.data);
  }
  return map;
}

/** Card messages per topic, with the latest card applied. */
async function cards(c: Client, gid: string): Promise<Map<string, any[]>> {
  const byId = new Map<string, any>();
  const out = new Map<string, any[]>();
  for (const e of await events(c, gid)) {
    if (e.type === "message.created") {
      const m = { ...e.data.message };
      byId.set(m.id, m);
      out.set(m.topicId, [...(out.get(m.topicId) ?? []), m]);
    }
    if (e.type === "message.updated" && e.data.card) byId.get(e.data.messageId).card = e.data.card;
  }
  return out;
}

describe("hook management", () => {
  it("creates a hook with a bot member, shows the secret once and deletes it", async () => {
    const owner = await createMember({ level: "owner" });
    const plain = await createMember();
    const gid = await createGroup(owner, [plain.id]);
    expect((await api("/v1/hooks", { token: plain.token, body: { name: "x", groupId: gid, kind: "generic" } })).status).toBe(403);
    expect((await api("/v1/hooks", { token: owner.token, body: { name: "x", groupId: "nope", kind: "generic" } })).status).toBe(404);
    expect(
      (await api("/v1/hooks", { token: owner.token, body: { name: "x", groupId: gid, topicId: "01NOPE", kind: "generic" } })).status,
    ).toBe(404);

    const r = await api("/v1/hooks", { token: owner.token, body: { name: "Deploys", groupId: gid, kind: "generic" } });
    expect(r.status).toBe(200);
    expect(r.body.secret).toMatch(/^whk_[A-Za-z0-9_-]{43}$/);
    expect(r.body.url).toBe(`${BASE}/v1/hooks/${r.body.hook.id}/in`);
    expect(r.body.hook).toMatchObject({ name: "Deploys", kind: "generic", groupId: gid, createdBy: owner.id, topicId: null });

    // The secret is not stored in plain text and never listed.
    const row = await env.DB.prepare("SELECT secret_enc FROM hooks WHERE id = ?").bind(r.body.hook.id).first<any>();
    expect(row.secret_enc).not.toContain(r.body.secret);
    const list = await api("/v1/hooks", { token: owner.token });
    expect(JSON.stringify(list.body)).not.toContain(r.body.secret);
    expect(list.body.find((h: any) => h.id === r.body.hook.id)).toBeTruthy();

    const members = await api("/v1/members", { token: owner.token });
    expect(members.body.find((m: any) => m.id === r.body.hook.memberId)).toMatchObject({ kind: "bot", name: "Deploys", level: "member" });
    const groups = await api("/v1/groups", { token: owner.token });
    expect(groups.body.find((g: any) => g.group.id === gid).group.memberIds).toContain(r.body.hook.memberId);

    const del = await api(`/v1/hooks/${r.body.hook.id}`, { method: "DELETE", token: owner.token });
    expect(del).toEqual({ status: 200, body: { ok: true } });
    expect((await api(`/v1/hooks/${r.body.hook.id}`, { method: "DELETE", token: owner.token })).status).toBe(404);
    const after = await api("/v1/members", { token: owner.token });
    expect(after.body.find((m: any) => m.id === r.body.hook.memberId).deactivated).toBe(true);
    expect((await postIn(r.body.hook.id, r.body.secret, { body: "hi" })).status).toBe(404);
    const audit = await env.DB.prepare("SELECT action FROM audit WHERE target = ? ORDER BY id").bind(r.body.hook.id).all<any>();
    expect(audit.results.map((a) => a.action)).toEqual(["hook.create", "hook.delete"]);
  });
});

describe("generic hook", () => {
  it("checks the secret and the kind", async () => {
    const { hook, secret, c } = await setup();
    expect((await postIn(hook.id, "whk_wrong", { body: "x" })).status).toBe(401);
    expect((await api(`/v1/hooks/${hook.id}/in`, { body: { body: "x" } })).status).toBe(401);
    expect((await postGithub(hook.id, secret, "ping", pingEvent(1))).status).toBe(404);
    expect((await postIn("01UNKNOWN", secret, { body: "x" })).status).toBe(404);
    c.close();
  });

  it("posts as the hook member, reuses topic keys and dedupes", async () => {
    const { hook, secret, gid, c } = await setup();
    const first = await postIn(hook.id, secret, { topicKey: "deploy:prod", title: "Prod deploys", body: "deploy 1 started" });
    expect(first.status, JSON.stringify(first.body)).toBe(200);
    expect(first.body.message).toMatchObject({ authorId: hook.memberId, body: "deploy 1 started", kind: "text" });
    expect(first.body.topic).toMatchObject({ title: "Prod deploys", createdBy: hook.memberId });
    const live = await c.waitEvent((e) => e.type === "message.created" && e.data.message.id === first.body.message.id);
    expect(live.groupId).toBe(gid);

    const second = await postIn(hook.id, secret, {
      topicKey: "deploy:prod",
      body: "",
      card: { v: 1, title: "Deploy 2", tone: "success", blocks: [{ type: "text", text: "done" }] },
    });
    expect(second.status, JSON.stringify(second.body)).toBe(200);
    expect(second.body.message).toMatchObject({ topicId: first.body.message.topicId, kind: "card", body: "Deploy 2" });

    // Idempotency-Key header: the retry returns the same message.
    const a = await postIn(hook.id, secret, { body: "once" }, { "idempotency-key": "job-77" });
    const b = await postIn(hook.id, secret, { body: "once" }, { "idempotency-key": "job-77" });
    expect(b.body).toMatchObject({ duplicate: true, message: { id: a.body.message.id } });
    // clientMsgId in the body wins and dedupes too.
    const x = await postIn(hook.id, secret, { body: "y", clientMsgId: "k1", topicId: first.body.message.topicId });
    const y = await postIn(hook.id, secret, { body: "y", clientMsgId: "k1", topicId: first.body.message.topicId });
    expect(y.body.duplicate).toBe(true);
    expect(y.body.message.id).toBe(x.body.message.id);

    // Same validation as send.
    expect((await postIn(hook.id, secret, { body: "" })).status).toBe(400);
    expect((await postIn(hook.id, secret, { body: "x", topicId: first.body.message.topicId, topicKey: "k" })).status).toBe(400);
    expect((await postIn(hook.id, secret, { body: "x", topicKey: "bad key!" })).status).toBe(400);
    expect((await postIn(hook.id, secret, { body: "x", topicId: "01NOTATOPIC" })).status).toBe(404);
    expect((await postIn(hook.id, secret, { body: "x", card: { v: 1, title: "t", blocks: [{ type: "nope" }] } })).status).toBe(400);
    const big = { v: 1, title: "t", blocks: [{ type: "code", text: "x".repeat(32_000) }, { type: "code", text: "x".repeat(32_000) }, { type: "code", text: "y".repeat(2000) }] };
    expect((await postIn(hook.id, secret, { body: "x", card: big })).status).toBe(413);

    const audit = await env.DB.prepare("SELECT count(*) AS n FROM audit WHERE action = 'hook.post' AND credential_id = ?")
      .bind(hook.id)
      .first<any>();
    expect(audit.n).toBe(4);
    c.close();
  });

  it("posts into the hook's default topic", async () => {
    const owner = await createMember({ level: "owner" });
    const gid = await createGroup(owner, []);
    const { c } = await Client.connect(owner.token);
    const t = await c.ok({ t: "send", groupId: gid, clientMsgId: crypto.randomUUID(), body: "alerts" });
    const r = await api("/v1/hooks", { token: owner.token, body: { name: "Alerts", groupId: gid, topicId: t.topic.id, kind: "generic" } });
    expect(r.body.hook.topicId).toBe(t.topic.id);
    const p = await postIn(r.body.hook.id, r.body.secret, { body: "disk 91%" });
    expect(p.body.message.topicId).toBe(t.topic.id);
    c.close();
  });
});

describe("github hook", () => {
  it("verifies X-Hub-Signature-256", async () => {
    const { hook, secret, url, c } = await setup("github");
    expect(url).toBe(`${BASE}/v1/hooks/${hook.id}/github`);
    expect((await postGithub(hook.id, secret, "ping", pingEvent(9))).body).toEqual({ ok: true });
    expect((await postGithub(hook.id, secret, "ping", pingEvent(9), { signature: null })).status).toBe(401);
    expect((await postGithub(hook.id, secret, "ping", pingEvent(9), { signature: "sha256=zz" })).status).toBe(401);
    const otherSig = await signGithub("whk_other", JSON.stringify(pingEvent(9)));
    expect((await postGithub(hook.id, secret, "ping", pingEvent(9), { signature: otherSig })).status).toBe(401);
    // A valid signature over a different body fails.
    const sig = await signGithub(secret, JSON.stringify(pingEvent(8)));
    expect((await postGithub(hook.id, secret, "ping", pingEvent(9), { signature: sig })).status).toBe(401);
    // A generic hook path refuses a github hook.
    expect((await postIn(hook.id, secret, { body: "x" })).status).toBe(404);
    // Unhandled events are accepted and dropped.
    expect((await postGithub(hook.id, secret, "star", { action: "created", repository: {} })).body).toEqual({ ok: true });
    c.close();
  });

  it("keeps one topic and one updated card per pull request and follows its status", async () => {
    const { owner, reviewer, hook, secret, gid, c } = await setup("github");
    const map = await api("/v1/integrations/github/logins", {
      method: "PUT",
      token: owner.token,
      body: { logins: { "Alice-GH": owner.id, "rev-gh": reviewer.id } },
    });
    expect(map.body).toEqual({ logins: { "alice-gh": owner.id, "rev-gh": reviewer.id } });

    const ok = async (event: string, payload: unknown, delivery?: string) => {
      const r = await postGithub(hook.id, secret, event, payload, { delivery });
      expect(r.status, JSON.stringify(r.body)).toBe(200);
    };
    await ok("pull_request", pullRequestEvent("opened"), "dlv-opened");
    let ts = await topics(c, gid);
    expect(ts.size).toBe(1);
    const [topic] = [...ts.values()];
    expect(topic.title).toBe(`${REPO}#42 Add webhook adapter`);
    expect(topic).toMatchObject({ status: "open", ownerId: owner.id, waitingOn: [] });

    await ok("pull_request", pullRequestEvent("review_requested", { reviewers: ["rev-gh", "stranger"] }, { requested_reviewer: { login: "rev-gh" } }));
    expect((await topics(c, gid)).get(topic.id)).toMatchObject({ status: "waiting_review", waitingOn: [reviewer.id] });

    await ok("check_suite", checkSuiteEvent("failure"));
    let cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(cs).toHaveLength(1);
    expect(cs[0].card).toMatchObject({ tone: "danger", status: { label: "等待评审" } });
    expect(JSON.stringify(cs[0].card)).toContain("GitHub Actions: 失败");

    // A workflow run without pull_requests (fork PRs) finds the PR by its head commit.
    await ok("workflow_run", workflowRunEvent("success", { name: "server", prs: [] }));
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(JSON.stringify(cs[0].card)).toContain("server: 通过");

    // A check that starts shows as pending until its result comes (check_run).
    const run = (action: string, status: string, conclusion: string | null) => ({
      action,
      check_run: { name: "e2e", head_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678", status, conclusion, html_url: "https://github.com/x", pull_requests: [{ number: 42 }] },
      repository: { full_name: REPO, html_url: `https://github.com/${REPO}` },
    });
    await ok("check_run", run("created", "in_progress", null));
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(JSON.stringify(cs[0].card)).toContain("e2e: 进行中");
    await ok("check_run", run("completed", "completed", "success"));
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(JSON.stringify(cs[0].card)).toContain("e2e: 通过");

    await ok("pull_request_review", reviewEvent("changes_requested", "rev-gh"));
    expect((await topics(c, gid)).get(topic.id)).toMatchObject({ status: "in_progress", waitingOn: [owner.id] });

    // New commits reset CI; review approved.
    await ok("pull_request", pullRequestEvent("synchronize", { sha: "b".repeat(40) }));
    await ok("pull_request_review", reviewEvent("approved", "rev-gh", { sha: "b".repeat(40) }));
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(cs[0].card).toMatchObject({ tone: "success", status: { label: "已批准" } });
    expect(JSON.stringify(cs[0].card)).not.toContain("GitHub Actions");
    // A CI result for the old head commit is ignored.
    await ok("check_suite", checkSuiteEvent("failure"));
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(cs[0].card.tone).toBe("success");

    // Redelivery of the same delivery id changes nothing.
    const before = (await events(c, gid)).length;
    await ok("pull_request", pullRequestEvent("opened"), "dlv-opened");
    expect((await events(c, gid)).length).toBe(before);

    await ok("pull_request", pullRequestEvent("closed", { state: "closed", merged: true, sha: "b".repeat(40) }));
    ts = await topics(c, gid);
    expect(ts.size).toBe(1);
    expect(ts.get(topic.id)).toMatchObject({ status: "done", waitingOn: [] });
    cs = (await cards(c, gid)).get(topic.id) as any[];
    expect(cs).toHaveLength(1);
    expect(cs[0]).toMatchObject({ authorId: hook.memberId, kind: "card" });
    expect(cs[0].card).toMatchObject({ tone: "success", status: { label: "已合并" } });
    c.close();
  });

  it("makes one topic when events for a new pull request arrive at the same time", async () => {
    const { hook, secret, gid, c } = await setup("github");
    const rs = await Promise.all([
      postGithub(hook.id, secret, "pull_request", pullRequestEvent("opened", { number: 7 })),
      postGithub(hook.id, secret, "check_suite", checkSuiteEvent("success", { prs: [7] })),
      postGithub(hook.id, secret, "workflow_run", workflowRunEvent("success", { prs: [7] })),
    ]);
    for (const r of rs) expect(r.status, JSON.stringify(r.body)).toBe(200);
    const ts = await topics(c, gid);
    expect(ts.size).toBe(1);
    const cs = [...(await cards(c, gid)).values()];
    expect(cs).toHaveLength(1);
    expect(cs[0]).toHaveLength(1);
    c.close();
  });

  it("links a pull request to an existing topic with 'Topic: <id>'", async () => {
    const { owner, hook, secret, gid, c } = await setup("github");
    const t = await c.ok({ t: "send", groupId: gid, clientMsgId: crypto.randomUUID(), body: "design the adapter" });
    const r = await postGithub(hook.id, secret, "pull_request", pullRequestEvent("opened", { number: 9, body: `Implements it.\n\nTopic: ${t.topic.id}` }));
    expect(r.status).toBe(200);
    const ts = await topics(c, gid);
    expect(ts.size).toBe(1);
    const cs = (await cards(c, gid)).get(t.topic.id) as any[];
    expect(cs.map((m) => m.kind)).toEqual(["text", "card"]);
    // The title stays the person's; status follows the PR.
    await postGithub(hook.id, secret, "pull_request", pullRequestEvent("closed", { number: 9, state: "closed", merged: true }));
    expect((await topics(c, gid)).get(t.topic.id)).toMatchObject({ title: "design the adapter", status: "done", ownerId: owner.id });
    c.close();
  });

  it("turns issues into topics and branch CI into a branch topic", async () => {
    const { hook, secret, gid, c } = await setup("github");
    await postGithub(hook.id, secret, "issues", issuesEvent("opened"));
    let ts = [...(await topics(c, gid)).values()];
    expect(ts).toHaveLength(1);
    expect(ts[0]).toMatchObject({ title: `${REPO}#7 Login fails on slow networks`, status: "open" });
    await postGithub(hook.id, secret, "issues", issuesEvent("closed"));
    expect((await topics(c, gid)).get(ts[0].id).status).toBe("done");

    await postGithub(hook.id, secret, "workflow_run", workflowRunEvent("failure", { sha: "c".repeat(40), branch: "main", prs: [] }));
    await postGithub(hook.id, secret, "check_suite", checkSuiteEvent("success", { sha: "c".repeat(40), branch: "main", prs: [] }));
    ts = [...(await topics(c, gid)).values()];
    expect(ts).toHaveLength(2);
    const branch = ts.find((t) => t.title === `${REPO} main CI`);
    expect(branch).toBeTruthy();
    const cs = (await cards(c, gid)).get(branch.id) as any[];
    expect(cs).toHaveLength(1);
    expect(cs[0].card.tone).toBe("danger");
    c.close();
  });

  it("checks the login map body", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    expect((await api("/v1/integrations/github/logins", { method: "PUT", token: m.token, body: { logins: {} } })).status).toBe(403);
    expect(
      (await api("/v1/integrations/github/logins", { method: "PUT", token: owner.token, body: { logins: { "bad login": m.id } } })).status,
    ).toBe(400);
    expect(
      (await api("/v1/integrations/github/logins", { method: "PUT", token: owner.token, body: { logins: { ok: "01NOBODY" } } })).status,
    ).toBe(400);
  });
});

describe("github payload parsing", () => {
  it("normalizes the documented payloads and ignores the rest", () => {
    expect(normalize("pull_request", pullRequestEvent("opened"))).toMatchObject({
      type: "pr",
      repo: REPO,
      number: 42,
      pr: { state: "open", author: "alice-gh", headRef: "feat/adapter", baseRef: "main" },
    });
    expect(normalize("pull_request", pullRequestEvent("closed", { state: "closed", merged: false }))).toMatchObject({
      pr: { state: "closed" },
    });
    expect(normalize("pull_request", pullRequestEvent("labeled"))).toBeNull();
    expect(normalize("pull_request_review", reviewEvent("commented", "rev-gh"))).toBeNull();
    expect(normalize("check_suite", { ...checkSuiteEvent("success"), action: "requested" })).toBeNull();
    expect(normalize("workflow_run", workflowRunEvent("failure", { prs: [3] }))).toMatchObject({
      type: "ci",
      name: "server",
      conclusion: "failure",
      prNumbers: [3],
    });
    expect(normalize("ping", pingEvent(1))).toBeNull();
    expect(() => normalize("pull_request", { action: "opened", repository: {} })).toThrow();
  });
});


describe("GitHub merge state (1.1, GAPS SF-5)", () => {
  const links = async (c: Client, gid: string) =>
    (await events(c, gid)).filter((e) => e.type === "topic.linked").map((e) => e.data.link);

  it("store a queued merge on the PR link, set its error when GitHub dequeues it, and drop it once merged", async () => {
    const { hook, secret, gid, c } = await setup("github");
    const post = async (action: string, o: Record<string, unknown> = {}, extra: Record<string, unknown> = {}) => {
      const r = await postGithub(hook.id, secret, "pull_request", pullRequestEvent(action, { number: 12, ...o }, extra));
      expect(r.status, JSON.stringify(r.body)).toBe(200);
    };
    await post("opened");
    expect((await links(c, gid)).at(-1).merge).toBeUndefined();
    const t0 = Date.now();
    await post("enqueued");
    const queued = (await links(c, gid)).at(-1);
    expect(queued.merge.queuedAt).toBeGreaterThanOrEqual(t0);
    expect(queued.merge.error).toBeUndefined();
    // A merge failure: the topic leaves 合并中 (the link says why).
    await post("dequeued", {}, { reason: "MERGE_CONFLICT" });
    const failed = (await links(c, gid)).at(-1);
    expect(failed.merge).toEqual({ queuedAt: queued.merge.queuedAt, error: "merge conflict" });
    // Auto-merge again, then merged: no queued merge on a merged PR.
    await post("auto_merge_enabled");
    expect((await links(c, gid)).at(-1).merge.error).toBeUndefined();
    await post("auto_merge_disabled", {}, { reason: "Base branch was modified" });
    expect((await links(c, gid)).at(-1).merge.error).toBe("Base branch was modified");
    await post("enqueued");
    await post("closed", { state: "closed", merged: true });
    const merged = (await links(c, gid)).at(-1);
    expect(merged).toMatchObject({ state: "merged" });
    expect(merged.merge).toBeUndefined();
    c.close();
  });
});
