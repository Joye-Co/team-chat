// Unit tests of pages (two-way HTML 产出) and live card events against the fake server
// (test/fake.ts) and a fake fetch: a page with its first data and preview, the data snapshot and
// the page log, handlers by name, and the readers of an event value. No network, no wrangler.
// Run: bun test test/artifacts.test.ts
import { afterEach, describe, expect, test } from "bun:test";
import { type CardV2, MAX_ARTIFACT_DATA_BYTES } from "@chat/protocol";
import {
  blocks,
  Bot,
  type CardEventContext,
  ChatError,
  checkArtifactData,
  composeCard,
  eventValueReaders,
  HttpClient,
  MemoryStateStore,
} from "../src/index.ts";
import { BOT_ID, FakeServer, GROUP_ID, HUMAN_ID, message, TOPIC_ID } from "./fake.ts";

const ART = "01JC0ART00000000000000OCT3";
const bots: Bot[] = [];
afterEach(async () => {
  for (const b of bots.splice(0)) await b.stop();
});

type Call = { method: string; path: string; search: string; body?: Record<string, unknown> };

/** A fetch that records each request and answers the routes a page needs. */
function fakeFetch(answer: (c: Call) => unknown = () => ({})) {
  const calls: Call[] = [];
  const impl = async (input: URL | string | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input));
    const c: Call = { method: init?.method ?? "GET", path: url.pathname, search: url.search };
    if (typeof init?.body === "string") c.body = JSON.parse(init.body) as Record<string, unknown>;
    calls.push(c);
    let out: unknown;
    if (c.method === "POST" && c.path === "/v1/files") out = { file: { id: `F${calls.length}` }, uploadUrl: `${url.origin}/v1/files/F${calls.length}/content` };
    else if (c.method === "PUT" && c.path.endsWith("/content")) out = { id: c.path.split("/")[3], name: "index.html", size: 1, mime: "text/html", sha256: "0".repeat(64) };
    else if (c.method === "POST" && c.path.startsWith("/v1/artifacts")) out = { artifact: { id: ART }, version: { artifactId: ART, version: c.path.endsWith("/versions") ? 2 : 1 } };
    else out = answer(c);
    return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
  };
  return { calls, fetch: impl as typeof fetch };
}

async function start(server: FakeServer, fetchImpl?: typeof fetch): Promise<Bot> {
  const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: new MemoryStateStore(), logLevel: "error", stopTimeoutMs: 500, fetch: fetchImpl });
  bots.push(bot);
  await bot.start();
  return bot;
}

const until = async (what: string, ok: () => boolean) => {
  for (let i = 0; i < 300 && !ok(); i++) await new Promise((r) => setTimeout(r, 10));
  if (!ok()) throw new Error(`timed out: ${what}`);
};

/** The ChatError a call throws, with `text`: its message without the leading code. */
const text = (err: ChatError): string => err.message.replace(/^[a-z_]+: /, "");
const caught = (fn: () => unknown): { code: string; message: string; chat: boolean } => {
  try {
    fn();
  } catch (err) {
    return { code: (err as ChatError).code, message: text(err as ChatError), chat: err instanceof ChatError };
  }
  throw new Error("did not throw");
};

describe("pages: the first data snapshot, the preview, the snapshot and the page log", () => {
  const preview = { title: "报名", summary: "周六 14:00", metrics: [{ label: "已报名", value: "12 人", delta: "+3" }] };

  test("publishHtml sends data and preview with a new 产出 and with a new version", async () => {
    const { calls, fetch } = fakeFetch();
    const http = new HttpClient({ url: "http://127.0.0.1:1", token: "t", fetch });
    const base = { groupId: GROUP_ID, topicId: TOPIC_ID, title: "报名", html: "<p>x</p>", data: { people: [] }, preview };
    await http.publishHtml(base);
    await http.publishHtml({ ...base, data: null, artifactId: ART });
    const created = calls.find((c) => c.method === "POST" && c.path === "/v1/artifacts")?.body;
    expect(created).toMatchObject({ kind: "html", entry: "index.html", data: { people: [] }, preview });
    const added = calls.find((c) => c.path === `/v1/artifacts/${ART}/versions`)?.body;
    // null is a snapshot too: it is sent, not dropped.
    expect(added).toMatchObject({ data: null, preview });
    expect(added && "data" in added).toBe(true);
    // Without them the request has neither field.
    calls.length = 0;
    await http.publishHtml({ groupId: GROUP_ID, topicId: TOPIC_ID, title: "报名", html: "<p>x</p>" });
    const plain = calls.find((c) => c.path === "/v1/artifacts")?.body as Record<string, unknown>;
    expect("data" in plain || "preview" in plain).toBe(false);
  });

  test("a snapshot over 256 KiB, a value that is not JSON and a bad preview fail before any request", async () => {
    const { calls, fetch } = fakeFetch();
    const http = new HttpClient({ url: "http://127.0.0.1:1", token: "t", fetch });
    const base = { groupId: GROUP_ID, topicId: TOPIC_ID, title: "t", html: "<p>x</p>" };
    const big = { text: "数".repeat(MAX_ARTIFACT_DATA_BYTES / 3) };
    await expect(http.publishHtml({ ...base, data: big })).rejects.toMatchObject({ code: "too_large" });
    await expect(http.putArtifactData(ART, 1, big)).rejects.toMatchObject({ code: "too_large" });
    await expect(http.putArtifactData(ART, 1, undefined)).rejects.toMatchObject({ code: "invalid" });
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(caught(() => checkArtifactData(loop)).code).toBe("invalid");
    expect(() => checkArtifactData({ text: "数".repeat(1000) })).not.toThrow();
    const seven = Array.from({ length: 7 }, (_, i) => ({ label: `m${i}`, value: "1" }));
    const err = (await http.publishHtml({ ...base, preview: { metrics: seven } }).catch((e) => e)) as ChatError;
    expect(err.code).toBe("invalid");
    expect(text(err)).toStartWith("preview: metrics");
    expect(calls).toHaveLength(0);
  });

  test("getArtifactData, putArtifactData and artifactEvents call the routes of docs/protocol.md", async () => {
    const { calls, fetch } = fakeFetch((c) => (c.method === "PUT" ? { dataRev: 5 } : c.path.endsWith("/events") ? { events: [], hasMore: false } : { data: { n: 1 }, dataRev: 4, updatedAt: 9 }));
    const http = new HttpClient({ url: "http://127.0.0.1:1", token: "t", fetch });
    expect(await http.getArtifactData(ART, 3)).toEqual({ data: { n: 1 }, dataRev: 4, updatedAt: 9 });
    expect(await http.putArtifactData(ART, 3, { n: 2 }, { baseDataRev: 4, cause: "PE1" })).toEqual({ dataRev: 5 });
    await http.putArtifactData(ART, 3, null);
    expect(await http.artifactEvents(ART, { before: "01JC0EV0000000000000000009", limit: 20 })).toEqual({ events: [], hasMore: false });
    await http.artifactEvents(ART);
    expect(calls.map((c) => `${c.method} ${c.path}${c.search}`)).toEqual([
      `GET /v1/artifacts/${ART}/versions/3/data`,
      `PUT /v1/artifacts/${ART}/versions/3/data`,
      `PUT /v1/artifacts/${ART}/versions/3/data`,
      `GET /v1/artifacts/${ART}/events?before=01JC0EV0000000000000000009&limit=20`,
      `GET /v1/artifacts/${ART}/events`,
    ]);
    expect(calls[1]?.body).toEqual({ data: { n: 2 }, baseDataRev: 4, cause: "PE1" });
    expect(calls[2]?.body).toEqual({ data: null });
  });

  test("ctx.publishPage fills the topic and the run; ctx.postCard posts in the topic with a stable id", async () => {
    const server = new FakeServer();
    server.on("send", (f) => ({ ok: true, result: { message: message({ id: "01JC0MSG0000000000000000P1", cardV2: f.cardV2 }) } }));
    const { calls, fetch } = fakeFetch((c) => (c.path === "/v1/runs" ? { id: "01JC0RUN000000000000000001", botId: BOT_ID, groupId: GROUP_ID, topicId: TOPIC_ID, status: "running", createdAt: 1, updatedAt: 1 } : {}));
    const bot = await start(server, fetch);
    bot.onMention(async (ctx) => {
      await ctx.setPlan([{ text: "做页面", state: "current" }]);
      const page = await ctx.publishPage({ title: "报名", html: "<p>x</p>", events: [{ name: "signup.join" }], data: { people: [] } });
      await ctx.postCard(composeCard({ intent: "info", title: "报名", blocks: [blocks.embed({ id: "page", artifactId: page.artifact.id, version: page.version.version })] }));
    });
    server.event({ seq: 1, groupId: GROUP_ID, at: Date.now(), actorId: HUMAN_ID, type: "message.created", data: { message: message({ id: "01JC0MSG0000000000000000Q1", authorId: HUMAN_ID, body: "@Ledger", mentions: [BOT_ID] }) } });
    const sent = await server.next((f) => f.t === "send" && f.cardV2 !== undefined);
    expect(sent.topicId).toBe(TOPIC_ID);
    expect(sent.clientMsgId).toBe("bot:01JC0MSG0000000000000000Q1:0");
    expect((sent.cardV2 as CardV2).blocks[0]).toMatchObject({ type: "embed", artifactId: ART, version: 1 });
    const created = calls.find((c) => c.method === "POST" && c.path === "/v1/artifacts")?.body;
    expect(created).toMatchObject({ groupId: GROUP_ID, topicId: TOPIC_ID, runId: "01JC0RUN000000000000000001", events: [{ name: "signup.join" }], data: { people: [] } });
    // Outside a handler, and with no run in the topic: no runId.
    await bot.publishPage({ groupId: GROUP_ID, topicId: "01JC0TPC0000000000000000T9" }, { title: "x", html: "<p>y</p>" });
    expect(calls.filter((c) => c.method === "POST" && c.path === "/v1/artifacts").at(-1)?.body).not.toHaveProperty("runId");
  });

  test("onArtifactEvent by name; ctx.data reads the snapshot; a push that is too large fails before it is sent", async () => {
    const server = new FakeServer();
    server.on("artifact.data", () => ({ ok: true, result: { dataRev: 8 } }));
    const { calls, fetch } = fakeFetch(() => ({ data: { people: ["a"] }, dataRev: 7, updatedAt: 1 }));
    const bot = await start(server, fetch);
    const seen: string[] = [];
    const errors: unknown[] = [];
    bot.onError((e) => errors.push(e));
    bot.onArtifactEvent("signup.join", async (ctx) => {
      seen.push(`join:${ctx.name}`);
      const { data, dataRev } = await ctx.data();
      await ctx.pushData({ people: [...(data as { people: string[] }).people, "b"] }, { baseDataRev: dataRev });
    });
    bot.onArtifactEvent("signup.big", async (ctx) => {
      await ctx.pushData({ text: "x".repeat(MAX_ARTIFACT_DATA_BYTES) });
    });
    bot.onArtifactEvent((ctx) => {
      seen.push(`any:${ctx.name}`);
    });
    const ev = (eventId: string, name: string) => ({ type: "artifact.event", groupId: GROUP_ID, topicId: TOPIC_ID, artifactId: ART, version: 3, eventId, name, kind: "emit", payload: null, actorId: HUMAN_ID, at: Date.now() });
    server.transient(ev("PE1", "signup.join"));
    server.transient(ev("PE2", "report.flag"));
    server.transient(ev("PE3", "signup.big"));
    const push = await server.next((f) => f.t === "artifact.data");
    expect(push).toMatchObject({ artifactId: ART, version: 3, data: { people: ["a", "b"] }, baseDataRev: 7, cause: "PE1" });
    expect(calls.map((c) => `${c.method} ${c.path}`)).toEqual([`GET /v1/artifacts/${ART}/versions/3/data`]);
    // The named handler did not run for another name; that event is resolved by the SDK.
    expect((await server.next((f) => f.t === "interaction.resolve" && f.eventId === "PE2")).result).toBe("ignored");
    expect((await server.next((f) => f.t === "interaction.resolve" && f.eventId === "PE3")).result).toBe("failed");
    expect(seen).toEqual(["join:signup.join", "any:signup.join", "any:report.flag", "any:signup.big"]);
    expect(server.of("artifact.data")).toHaveLength(1);
    expect((errors[0] as ChatError).code).toBe("too_large");
    expect(() => bot.onArtifactEvent("signup.join", undefined as never)).toThrow(ChatError);
  });
});

describe("live events: values and filters", () => {
  test("the readers take the shape of each event and give undefined or [] for another", () => {
    expect(eventValueReaders({ value: "wrong" }).choice()).toBe("wrong");
    expect(eventValueReaders({ value: "other", other: "别的" }).choices()).toEqual(["other"]);
    expect(eventValueReaders({ values: ["a", "b", 3] }).choices()).toEqual(["a", "b"]);
    expect(eventValueReaders("o2").choice()).toBe("o2");
    expect(eventValueReaders(["o1", "o2"]).choices()).toEqual(["o1", "o2"]);
    expect(eventValueReaders({ value: "A", note: "更快" }).choice()).toBe("A");
    expect(eventValueReaders({ buttonId: "split" }).button()).toBe("split");
    expect(eventValueReaders({ values: ["weibo", "bili"] }).rows()).toEqual(["weibo", "bili"]);
    expect(eventValueReaders({ tabId: "t2" }).tab()).toBe("t2");
    expect(eventValueReaders({ series: "oct", x: "10-20", y: 2301, index: 2 }).point()).toEqual({ series: "oct", x: "10-20", y: 2301, index: 2 });
    expect(eventValueReaders({ series: "oct", x: {}, y: "2301", index: -1 }).point()).toEqual({ series: "oct" });
    expect(eventValueReaders({ column: "新增群", dir: "desc" }).sort()).toEqual({ column: "新增群", dir: "desc" });
    for (const bad of [undefined, null, 7, true, { value: 3 }, { buttonId: 1 }, { tabId: {} }, { column: "a", dir: "up" }, { series: 1 }]) {
      const r = eventValueReaders(bad);
      expect([r.choice(), r.choices(), r.button(), r.rows(), r.tab(), r.point(), r.sort()]).toEqual([undefined, [], undefined, [], undefined, undefined, undefined]);
    }
  });

  test("onCardEvent by name and by block: only matching handlers run, the context has the readers", async () => {
    const server = new FakeServer();
    const card = composeCard({
      intent: "info",
      title: "月报",
      blocks: [
        blocks.buttons({ id: "more", buttons: [{ id: "split", label: "按渠道拆开" }] }),
        blocks.decision({ id: "check", label: "对不对", options: [{ id: "ok", label: "对" }, { id: "wrong", label: "有问题" }], events: ["choice.change"] }),
      ],
    });
    server.on("send", (f) => ({ ok: true, result: { message: message({ id: "01JC0MSG0000000000000000C1", kind: "card", cardV2: f.cardV2 }) } }));
    const bot = await start(server);
    const posted = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, card);
    const seen: string[] = [];
    const note = (tag: string, read: (ctx: CardEventContext) => unknown) => async (ctx: CardEventContext) => {
      seen.push(`${tag}:${String(read(ctx))}`);
    };
    bot.onCardEvent("button.press", note("press", (c) => c.button()));
    bot.onCardEvent({ block: "check" }, note("check", (c) => c.choice()));
    bot.onCardEvent({ name: "choice.change", block: "more" }, note("never", () => ""));
    bot.onCardEvent(note("any", (c) => c.name));
    const live = (eventId: string, o: Record<string, unknown>) => ({ type: "card.event", groupId: GROUP_ID, topicId: TOPIC_ID, messageId: posted.message.id, eventId, rev: 1, actorId: HUMAN_ID, at: Date.now(), ...o });
    server.transient(live("E1", { blockId: "more", event: "button.press", value: { buttonId: "split" } }));
    server.transient(live("E2", { blockId: "check", event: "choice.change", value: { value: "wrong" } }));
    await until("both events are resolved", () => server.of("interaction.resolve").length === 2);
    expect(seen).toEqual(["press:split", "any:button.press", "check:wrong", "any:choice.change"]);
    expect(() => bot.onCardEvent("button.press", undefined as never)).toThrow(ChatError);
  });
});
