// Run context (docs/protocol.md "Run context", machine-protocol.md "Topic context"): compact
// lines within a character budget, resume sends only what is new, the 群聊 does not continue
// another request's session, and the run reads the rest with chat-run.
import { mainTopicId } from "@chat/protocol";
import { SELF } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import { type ContextRow, renderMessage, requestText, selectLines, stamp } from "../src/context/render";
import { Daemon, downViolations, mention, world } from "./daemon";
import { BASE, Client, type TestMember, api, clientMsgId, protocolViolations } from "./helpers";

/** A small file of the group (the bytes do not matter to the prompt). */
async function upload(m: TestMember, groupId: string, name: string, mime: string): Promise<string> {
  const bytes = new TextEncoder().encode("png");
  const sum = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
  const c = await api("/v1/files", { token: m.token, body: { groupId, name, size: bytes.length, mime, sha256: sum } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const row = (seq: number, body: string, ext: ContextRow["ext"] = {}, authorId = "u1"): ContextRow => ({
  id: `m${seq}`,
  seq,
  authorId,
  at: Date.UTC(2026, 10, 8, 6, 2),
  body,
  kind: "text",
  ext,
});
const names = (id: string) => ({ u1: "张三", u2: "李四" })[id] ?? id;

describe("render", () => {
  it("keeps files, quotes and cards as lines and cuts long bodies with a hint", () => {
    const text = renderMessage(
      row(7, "看下这张图", {
        replyTo: { messageId: "m3", authorId: "u2", excerpt: "先别动生产" },
        attachments: [{ fileId: "f1", name: "设计稿.png", mime: "image/png", size: 210_000, width: 1280, height: 720 }],
        artifactRefs: [{ artifactId: "a1", version: 3 }],
        cardV2: { title: "选部署时间" },
        cardState: { answers: [{ actorId: "u2", value: "周五晚上" }] },
      }),
      names,
    );
    expect(text).toContain("[#7 m m7 · 张三 · 11-08 14:02] 看下这张图");
    expect(text).toContain("↪ quotes m m3 (李四: 先别动生产)");
    expect(text).toContain("[file f f1 · 设计稿.png · image/png · 205 KB · 1280×720]");
    expect(text).toContain("[产出 a a1 v3]");
    expect(text).toContain("[card · 选部署时间 · 1 answers: 李四 周五晚上]");
    const long = renderMessage(row(8, "长".repeat(2000)), names, 600, "rest: chat-run topic message --id m8");
    expect([...long].length).toBeLessThan(700);
    expect(long).toContain("rest: chat-run topic message --id m8");
  });

  it("selects the first message and the newest ones within the budget", () => {
    const rows = Array.from({ length: 200 }, (_, i) => row(i + 1, `消息 ${i + 1} ${"x".repeat(200)}`));
    const newest = [...rows].reverse();
    const sel = selectLines(rows[0], newest, 200, names, 5000, { gap: (n, before) => `[gap ${n} before ${before}]` });
    expect(sel.rows[0].seq).toBe(1);
    expect(sel.rows[sel.rows.length - 1].seq).toBe(200);
    expect(sel.omitted).toBe(200 - sel.rows.length);
    expect(sel.text).toMatch(/\[gap \d+ before \d+\]/);
    expect(sel.chars).toBeLessThanOrEqual(5300);
    // A small topic fits whole: no gap.
    const small = selectLines(rows[0], [rows[2], rows[1], rows[0]], 3, names, 5000);
    expect(small.omitted).toBe(0);
    expect(small.rows.map((r) => r.seq)).toEqual([1, 2, 3]);
  });

  it("carries the first message whole up to a few thousand characters", () => {
    const first = row(1, `需求 ${"长".repeat(3000)} 结尾`);
    const sel = selectLines(first, [row(2, "x".repeat(1000)), first], 2, names, 20_000);
    expect(sel.text).toContain("结尾");
    expect(sel.text).not.toContain("x".repeat(700));
    const huge = selectLines(row(1, "长".repeat(9000)), [], 1, names, 20_000);
    expect(huge.chars).toBeLessThan(4200);
  });

  it("stamps times on the team's clock", () => {
    const at = Date.UTC(2026, 10, 8, 6, 2);
    expect(stamp(at)).toBe("11-08 14:02");
    expect(stamp(at, "UTC")).toBe("11-08 06:02");
    expect(stamp(at, "America/New_York")).toBe("11-08 01:02");
    expect(stamp(at, "Not/AZone")).toBe("11-08 14:02");
    expect(renderMessage(row(3, "hi"), names, 600, "", "UTC")).toContain("· 11-08 06:02]");
  });

  it("renders a request's files, 产出 and quote; an image alone is not empty", () => {
    const ext = {
      replyTo: { messageId: "m3", authorId: "u2", excerpt: "先别动生产" },
      attachments: [{ fileId: "f1", name: "截图.png", mime: "image/png", size: 2048, width: 800, height: 600 }],
      artifactRefs: [{ artifactId: "a1", version: 2 }],
    };
    const text = requestText("修一下这个", ext, names);
    expect(text.split("\n")[0]).toBe("修一下这个");
    expect(text).toContain("  ↪ quotes m m3 (李四: 先别动生产)");
    expect(text).toContain("  [file f f1 · 截图.png · image/png · 2 KB · 800×600]");
    expect(text).toContain("  [产出 a a1 v2]");
    const only = requestText("", { attachments: ext.attachments }, names);
    expect(only).toBe("  [file f f1 · 截图.png · image/png · 2 KB · 800×600]");
    expect(requestText("plain", undefined, names)).toBe("plain");
  });
});

async function finish(d: Daemon, runId: string, sessionId: string): Promise<void> {
  d.send({ t: "run.accept", runId });
  await d.acked(runId, d.done(runId, { status: "succeeded", resultText: "done", sessionId }));
}

/** Sends, waiting out the frame rate limit (long topics in tests). */
async function send(c: Client, gid: string, topicId: string, body: string, extra: Record<string, unknown> = {}) {
  const frame = { t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body, ...extra };
  for (;;) {
    const r = await c.request(frame);
    if (r.ok) return r.result;
    expect(r.error?.code, JSON.stringify(r)).toBe("rate_limited");
    await new Promise((x) => setTimeout(x, (r.error.retryAfterMs ?? 100) + 20));
  }
}

describe("run context", () => {
  it("carries the first message and the newest within the budget; resume gets only what is new", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume", "context"] });
    const { c } = await Client.connect(w.human.token);
    // A long topic without the bot: the first message is the requirement.
    const first = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "需求：登录页要支持手机号" });
    const topicId = first.message.topicId as string;
    for (let i = 2; i <= 80; i++) await send(c, w.gid, topicId, `第 ${i} 条 ${"讨论".repeat(400)}`);
    await send(c, w.gid, topicId, "@dev 按第 1 条做", { mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    expect(a.prompt).toContain("需求：登录页要支持手机号");
    expect(a.prompt).toContain("第 80 条");
    expect(a.prompt).toMatch(/messages left out; read them with chat-run topic history --before \d+/);
    expect(a.prompt).toContain("chat-run topic history");
    expect(a.resumeSessionId).toBeUndefined();

    // The developer view reads what the prompt carried.
    const ctx = await api(`/v1/runs/${a.runId}/context`, { token: w.owner.token });
    expect(ctx.status).toBe(200);
    expect(ctx.body.manifest).toMatchObject({ resumed: false, firstSeq: first.message.createdSeq });
    expect(ctx.body.manifest.omitted).toBeGreaterThan(0);
    expect(ctx.body.prompt).toBe(a.prompt);
    expect((await api(`/v1/runs/${a.runId}/context`, { token: w.human.token })).status).toBe(403);

    // The run reads older messages, a whole message and searches.
    const auth = { authorization: `Bearer ${w.machineToken}` };
    const hist = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/topic/messages?before=${first.message.createdSeq + 3}&limit=5`, { headers: auth });
    const h = (await hist.json()) as any;
    expect(h.messages.map((m: any) => m.seq)[0]).toBe(first.message.createdSeq);
    expect(h.messages[0].text).toContain("需求：登录页要支持手机号");
    const one = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/topic/messages/${first.message.id}`, { headers: auth });
    expect(((await one.json()) as any).message.authorName).toBe("Alice");
    const found = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/search?q=${encodeURIComponent("手机号")}`, { headers: auth });
    expect(((await found.json()) as any).hits.some((x: any) => x.messageId === first.message.id)).toBe(true);
    const bad = await SELF.fetch(`${BASE}/v1/machine/runs/${a.runId}/topic/messages?limit=500`, { headers: auth });
    expect(bad.status).toBe(400);

    await finish(d, a.runId, "thread-1");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    // Two new messages, then the next mention: the resumed session gets only those.
    await send(c, w.gid, topicId, "补充：还要支持邮箱");
    await send(c, w.gid, topicId, "@dev 再加一下", { mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBe("thread-1");
    expect(b.prompt).toContain("补充：还要支持邮箱");
    expect(b.prompt).toContain("再加一下");
    expect(b.prompt).not.toContain("需求：登录页要支持手机号");
    expect(b.prompt.length).toBeLessThan(2000);
    const bctx = await api(`/v1/runs/${b.runId}/context`, { token: w.owner.token });
    expect(bctx.body.manifest).toMatchObject({ resumed: true, messages: 1 });
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("the request and its follow-ups carry their files and the message they quote", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["context"] });
    const { c } = await Client.connect(w.human.token);
    const file = await upload(w.human, w.gid, "截图.png", "image/png");
    const quoted = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body: "登录按钮点了没反应" });
    const topicId = quoted.message.topicId as string;
    await send(c, w.gid, topicId, "@dev 修一下这个", {
      mentions: [w.bot.id],
      replyTo: { messageId: quoted.message.id },
      attachments: [{ fileId: file, name: "截图.png", size: 3, mime: "image/png", sha256: "0".repeat(64) }],
    });
    const a = await d.next((f) => f.t === "run.assign");
    const req = a.prompt.slice(a.prompt.indexOf("Request from"));
    expect(req).toContain("修一下这个");
    expect(req).toContain(`[file f ${file} · 截图.png · image/png`);
    expect(req).toContain(`↪ quotes m ${quoted.message.id} (Alice: 登录按钮点了没反应)`);
    d.send({ t: "run.accept", runId: a.runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "running");

    // A follow-up that is only an image reaches the running CLI as its file line.
    const second = await upload(w.human, w.gid, "第二张.png", "image/png");
    await send(c, w.gid, topicId, "", { attachments: [{ fileId: second, name: "第二张.png", size: 3, mime: "image/png", sha256: "0".repeat(64) }] });
    const input = await d.next((f) => f.t === "run.input");
    expect(input.text).toContain(`[file f ${second} · 第二张.png`);
    // Not taken by the CLI: the follow-up run's prompt carries it as well.
    await d.acked(a.runId, d.done(a.runId, { status: "succeeded", resultText: "ok" }));
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.prompt.slice(b.prompt.indexOf("Request from"))).toContain(`[file f ${second}`);
    d.send({ t: "run.accept", runId: b.runId });
    await d.acked(b.runId, d.done(b.runId, { status: "succeeded", resultText: "ok" }));
    d.close();
    c.close();
  });

  it("in the 群聊 a new request starts a new session; quoting the bot continues it", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["resume"] });
    const { c } = await Client.connect(w.human.token);
    const main = mainTopicId(w.gid);
    await send(c, w.gid, main, "@dev 看下 CI", { mentions: [w.bot.id] });
    const a = await d.next((f) => f.t === "run.assign");
    await finish(d, a.runId, "thread-a");
    const done = await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === a.runId && e.data.run.status === "done");

    await send(c, w.gid, main, "@dev 另一件事：写周报", { mentions: [w.bot.id] });
    const b = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId);
    expect(b.resumeSessionId).toBeUndefined();
    await finish(d, b.runId, "thread-b");
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === b.runId && e.data.run.status === "done");

    // Quoting the bot's result continues that bot's last session.
    const result = done.data.run.resultMessageId as string | undefined;
    expect(result).toBeDefined();
    {
      await send(c, w.gid, main, "@dev 这个再看看", { mentions: [w.bot.id], replyTo: { messageId: result } });
      const x = await d.next((f) => f.t === "run.assign" && f.runId !== a.runId && f.runId !== b.runId);
      expect(x.resumeSessionId).toBe("thread-b");
      d.send({ t: "run.accept", runId: x.runId });
      await d.acked(x.runId, d.done(x.runId, { status: "succeeded", resultText: "ok" }));
    }
    d.close();
    c.close();
  });
});
