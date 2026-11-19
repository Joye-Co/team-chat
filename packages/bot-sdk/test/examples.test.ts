// The runnable examples (examples/report-bot.ts, examples/page-bot.ts and its page) driven with
// their real handlers against the fake server: every card they post and every patch they make
// passes the server's own patch and schema rules, and the page calls only what its bot declares.
// Run: bun test test/examples.test.ts
import { afterEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { ArtifactPreview, CardV2, CreateArtifactRequest, type PatchOp } from "@chat/protocol";
import { attach as attachPage, EVENTS, PAGE, type Sheet } from "../../../examples/page-bot.ts";
import { attach as attachReport, reportCard } from "../../../examples/report-bot.ts";
import { Bot, checkArtifactEvents, MemoryStateStore } from "../src/index.ts";
import { BOT_ID, FakeServer, GROUP_ID, HUMAN_ID, message, TOPIC_ID } from "./fake.ts";

const ROOT = join(import.meta.dir, "../../..");
// The server's patch rule, loaded at run time (its file is outside this package's typecheck).
const serverPatch = (await import(join(ROOT, "server/src/cards2/patch.ts"))) as {
  applyPatch(card: CardV2, ops: PatchOp[]): CardV2;
};

const ARTIFACT_ID = "01JC0ART0000000000000SHEET";
const bots: Bot[] = [];
afterEach(async () => {
  for (const b of bots.splice(0)) await b.stop();
});

/** A fake HTTP side: file uploads, one 产出 with its data snapshot (with the server's rev rule). */
function fakeHttp() {
  const state = { data: null as unknown, dataRev: 0, created: [] as Record<string, unknown>[], uploads: 0 };
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  const fetchImpl = async (input: URL | string | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input));
    const method = init?.method ?? "GET";
    const body = typeof init?.body === "string" ? (JSON.parse(init.body) as Record<string, unknown>) : undefined;
    if (method === "POST" && url.pathname === "/v1/files") {
      const id = `01JC0F1LE00000000000000${String(++state.uploads).padStart(3, "0")}`;
      return json({ file: { id }, uploadUrl: `${url.origin}/v1/files/${id}/content` });
    }
    if (method === "PUT" && /^\/v1\/files\/[^/]+\/content$/.test(url.pathname)) {
      return json({ id: url.pathname.split("/")[3], name: "index.html", size: 1, mime: "text/html", sha256: "0".repeat(64) });
    }
    if (method === "POST" && url.pathname === "/v1/artifacts") {
      const req = CreateArtifactRequest.safeParse(body);
      if (!req.success) return json({ error: { code: "invalid", message: JSON.stringify(req.error.issues) } }, 400);
      state.created.push(body as Record<string, unknown>);
      if (req.data.data !== undefined) {
        state.data = req.data.data;
        state.dataRev = 1;
      }
      const version = { artifactId: ARTIFACT_ID, version: 1, entry: req.data.entry, files: [], createdBy: BOT_ID, createdAt: 1, events: req.data.events, dataRev: state.dataRev, preview: req.data.preview };
      return json({ artifact: { id: ARTIFACT_ID, groupId: GROUP_ID, topicId: TOPIC_ID, title: req.data.title, kind: "html", latestVersion: 1 }, version });
    }
    if (method === "GET" && url.pathname === `/v1/artifacts/${ARTIFACT_ID}/versions/1/data`) {
      return json({ data: state.data, dataRev: state.dataRev, updatedAt: 1 });
    }
    return json({ error: { code: "not_found", message: `${method} ${url.pathname}` } }, 404);
  };
  return { state, fetch: fetchImpl as typeof fetch };
}

async function start(server: FakeServer, fetchImpl?: typeof fetch): Promise<Bot> {
  const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: new MemoryStateStore(), logLevel: "error", stopTimeoutMs: 500, fetch: fetchImpl });
  bots.push(bot);
  return bot;
}

function mention(server: FakeServer, id: string): void {
  server.event({
    seq: 1,
    groupId: GROUP_ID,
    at: Date.now(),
    actorId: HUMAN_ID,
    type: "message.created",
    data: { message: message({ id, authorId: HUMAN_ID, body: "@Ledger 发一下", mentions: [BOT_ID] }) },
  });
}

const until = async (what: string, ok: () => boolean) => {
  for (let i = 0; i < 300 && !ok(); i++) await new Promise((r) => setTimeout(r, 10));
  if (!ok()) throw new Error(`timed out: ${what}`);
};

describe("examples/report-bot.ts", () => {
  /** `send` stores the card; `card.patch` runs the server's applyPatch, so a bad patch fails here. */
  function cardServer() {
    const server = new FakeServer();
    const cards = new Map<string, CardV2>();
    const refused: string[] = [];
    let n = 0;
    const stored = (id: string, card: CardV2) => message({ id, kind: "card", card: { v: 1, title: card.title, blocks: [] }, cardV2: card });
    server.on("send", (f) => {
      const id = `01JC0MSG0000000000000000R${++n}`;
      const card = f.cardV2 as CardV2 | undefined;
      if (card) cards.set(id, card);
      return { ok: true, result: { message: card ? stored(id, card) : message({ id, body: f.body }) } };
    });
    server.on("card.patch", (f) => {
      const id = f.messageId as string;
      const cur = cards.get(id) as CardV2;
      if (f.baseRev !== cur.rev) return { ok: false, error: { code: "conflict", message: "stale", reason: "card.stale_rev" } };
      try {
        const next = CardV2.parse(serverPatch.applyPatch(cur, f.ops as PatchOp[]));
        cards.set(id, next);
        return { ok: true, result: { message: stored(id, next), rev: next.rev } };
      } catch (err) {
        refused.push(String((err as Error).message));
        return { ok: false, error: { code: "invalid", message: String((err as Error).message) } };
      }
    });
    return { server, cards, refused };
  }

  const live = (messageId: string, n: number, o: Record<string, unknown>) => ({
    type: "card.event",
    groupId: GROUP_ID,
    topicId: TOPIC_ID,
    messageId,
    eventId: `01JC0EV00000000000000000E${n}`,
    rev: 1,
    actorId: HUMAN_ID,
    at: Date.now(),
    ...o,
  });

  test("the posted card fits the protocol and its buttons declare button.press", () => {
    const card = reportCard(HUMAN_ID);
    expect(CardV2.safeParse(card).success).toBe(true);
    expect(card.catalog).toBe("tc.card/2.1");
    expect(card.blocks.find((b) => b.id === "more")?.events).toEqual(["button.press"]);
  });

  test("a mention posts the card; a point, the button, a pick and the submit each patch it in place", async () => {
    const { server, cards, refused } = cardServer();
    const bot = await start(server);
    const errors: unknown[] = [];
    bot.onError((e) => errors.push(e));
    attachReport(bot);
    await bot.start();
    mention(server, "01JC0MSG0000000000000000Q1");
    const sent = await server.next((f) => f.t === "send" && f.cardV2 !== undefined);
    expect(server.of("react")).toHaveLength(1);
    expect((sent.cardV2 as CardV2).ask?.from).toEqual([HUMAN_ID]);
    await until("the card is stored", () => cards.size === 1);
    const id = [...cards.keys()][0] as string;
    const card = () => cards.get(id) as CardV2;
    const block = (bid: string) => card().blocks.find((b) => b.id === bid) as Record<string, unknown> | undefined;

    // A chart point.
    server.transient(live(id, 1, { blockId: "trend", event: "chart.select", value: { series: "oct", x: "10-20", y: 2301, index: 2 }, part: { kind: "point", keys: { series: "oct", x: "10-20" } } }));
    await until("rev 2", () => card().rev === 2);
    expect(block("point")?.text).toBe("10-20 这一周，10 月日活 2,301。");

    // A point that is not of this chart changes nothing and is resolved for the person.
    server.transient(live(id, 2, { blockId: "trend", event: "chart.select", value: { series: "nope", index: 99 } }));
    expect((await server.next((f) => f.t === "interaction.resolve" && f.eventId === "01JC0EV00000000000000000E2")).result).toBe("ignored");

    // The inline button: the table appears, the button goes.
    server.transient(live(id, 3, { blockId: "more", event: "button.press", value: { buttonId: "channels" } }));
    await until("rev 3", () => card().rev === 3);
    expect(block("by_channel")?.rowKeys).toEqual(["weibo", "invite", "bili", "douyin", "total"]);
    expect(block("more")).toBeUndefined();

    // "有问题", live: an input and the submit's inputs; "对" takes them back.
    server.transient(live(id, 4, { blockId: "check", event: "choice.change", value: { value: "wrong" } }));
    await until("rev 4", () => card().rev === 4);
    expect(block("what")?.type).toBe("input");
    expect(card().actions?.[0]?.inputs).toEqual(["check", "what"]);
    server.transient(live(id, 5, { blockId: "check", event: "choice.change", value: { value: "ok" } }));
    await until("rev 5", () => card().rev === 5);
    expect(block("what")).toBeUndefined();
    server.transient(live(id, 6, { blockId: "check", event: "choice.change", value: { value: "wrong" } }));
    await until("rev 6", () => card().rev === 6);

    // The submit (a stored answer).
    server.event({
      seq: 2,
      groupId: GROUP_ID,
      at: Date.now(),
      actorId: HUMAN_ID,
      type: "card.answered",
      data: { messageId: id, rev: 6, actionId: "send", askId: "verify", answerId: "01JC0ANS000000000000000001", inputs: { check: { value: "wrong" }, what: { value: "新增群后台是 40" } } },
    });
    await until("rev 7", () => card().rev === 7);
    expect(card().ask).toBeUndefined();
    expect(card().actions ?? []).toEqual([]);
    expect(block("check")).toMatchObject({ type: "evidence", label: "要核对：新增群后台是 40", state: "info" });
    expect((await server.next((f) => f.t === "topic.update")).status).toBe("in_progress");
    expect(refused).toEqual([]);
    expect(errors).toEqual([]);
  });
});

describe("examples/page-bot.ts and its page", () => {
  test("the declarations pass the server's rules", () => {
    expect(() => checkArtifactEvents(EVENTS)).not.toThrow();
  });

  test("every teamchat.emit and teamchat.ask of the page is declared with that kind", () => {
    const calls = [...PAGE.matchAll(/\b(?:tc|teamchat)\.(emit|ask)\(\s*(?:"([^"]+)"|'([^']+)'|(\w+))/g)];
    const named = calls.filter((m) => m[2] ?? m[3]).map((m) => ({ kind: m[1], name: (m[2] ?? m[3]) as string }));
    expect(named.length).toBeGreaterThan(0);
    for (const c of named) expect(EVENTS.some((e) => e.name === c.name && (e.kind ?? "emit") === c.kind) ? "" : `${c.kind} ${c.name} is not declared`).toBe("");
    // The page works without the bridge: every use of it is guarded.
    expect(PAGE).toContain("window.teamchat");
    expect(PAGE).toContain("if (!tc)");
  });

  test("a mention publishes the page with events, the first snapshot and a preview, and embeds it", async () => {
    const server = new FakeServer();
    const http = fakeHttp();
    const bot = await start(server, http.fetch);
    const errors: unknown[] = [];
    bot.onError((e) => errors.push(e));
    attachPage(bot);
    await bot.start();
    mention(server, "01JC0MSG0000000000000000Q2");
    const sent = await server.next((f) => f.t === "send" && f.cardV2 !== undefined);
    const req = http.state.created[0] as Record<string, unknown>;
    expect(req.events).toEqual(EVENTS);
    expect(req.data).toEqual({ title: "团建报名", when: "周六 14:00 · 奥森公园南门", seats: 12, people: [] });
    expect(ArtifactPreview.safeParse(req.preview).success).toBe(true);
    const card = sent.cardV2 as CardV2;
    expect(CardV2.safeParse(card).success).toBe(true);
    expect(card.blocks[0]).toMatchObject({ type: "embed", artifactId: ARTIFACT_ID, version: 1, events: EVENTS.map((e) => e.name) });
    expect(errors).toEqual([]);
  });

  test("join, a concurrent push, seats and leave: the sheet lives in the snapshot, not in the process", async () => {
    const server = new FakeServer();
    const http = fakeHttp();
    http.state.data = { title: "团建报名", when: "周六", seats: 3, people: [{ memberId: "01JC0MBR00000000000000ELSE", name: "小林" }] } satisfies Sheet;
    http.state.dataRev = 3;
    let conflicts = 1;
    server.on("artifact.data", (f) => {
      // The server's rule: a push based on another rev is refused.
      if (f.baseDataRev !== http.state.dataRev) return { ok: false, error: { code: "conflict", message: "stale data rev" } };
      if (conflicts-- > 0) {
        // Someone else signs up between this handler's read and its push.
        http.state.data = { ...(http.state.data as Sheet), people: [...(http.state.data as Sheet).people, { memberId: "01JC0MBR000000000000000BEN", name: "Ben" }] };
        http.state.dataRev++;
        return { ok: false, error: { code: "conflict", message: "stale data rev" } };
      }
      http.state.data = f.data;
      return { ok: true, result: { dataRev: ++http.state.dataRev } };
    });
    const bot = await start(server, http.fetch);
    const errors: unknown[] = [];
    bot.onError((e) => errors.push(e));
    attachPage(bot);
    await bot.start();
    const ev = (n: number, name: string, kind: "emit" | "ask", payload: unknown) => ({
      type: "artifact.event",
      groupId: GROUP_ID,
      topicId: TOPIC_ID,
      artifactId: ARTIFACT_ID,
      version: 1,
      eventId: `PE${n}`,
      name,
      kind,
      payload,
      actorId: HUMAN_ID,
      at: Date.now(),
    });
    const people = () => (http.state.data as Sheet).people.map((p) => [p.name, p.note]);

    // The payload is the page's: a note that is too long is cut, another shape is dropped.
    server.transient(ev(1, "signup.join", "emit", { note: `  不吃辣${"！".repeat(60)}`, admin: true }));
    await until("the join is pushed", () => http.state.dataRev === 5);
    expect(people()).toEqual([["小林", undefined], ["Ben", undefined], ["Joye", `不吃辣${"！".repeat(37)}`]]);
    const pushes = server.of("artifact.data");
    expect(pushes).toHaveLength(2);
    expect(pushes.map((f) => f.baseDataRev)).toEqual([3, 4]);
    expect(pushes.every((f) => f.cause === "PE1")).toBe(true);

    server.transient(ev(2, "signup.seats", "ask", null));
    const reply = await server.next((f) => f.t === "artifact.reply" && f.eventId === "PE2");
    expect(reply).toMatchObject({ ok: true, payload: { left: 0 } });

    // The sheet is full: another member's join changes nothing, and is still answered by a push.
    server.transient({ ...ev(3, "signup.join", "emit", {}), actorId: "01JC0MBR0000000000000000XY" });
    await until("the refused join is pushed", () => http.state.dataRev === 6);
    expect(people()).toHaveLength(3);

    server.transient(ev(4, "signup.leave", "emit", null));
    await until("the leave is pushed", () => http.state.dataRev === 7);
    expect(people()).toEqual([["小林", undefined], ["Ben", undefined]]);
    // Each event got its own answer: no automatic resolve or decline.
    expect(server.of("interaction.resolve")).toHaveLength(0);
    expect(server.of("artifact.reply")).toHaveLength(1);
    expect(errors).toEqual([]);
  });
});
