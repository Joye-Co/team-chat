// Unit tests of the checks the SDK runs on a card before anything is sent: the live events a
// block may declare (the server's own table), card error texts that name the block, and the ids
// a `CardDraft` is given. A fake server, no network. Run: bun test test/card-checks.test.ts
import { afterEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { type CardV2, type CardV2Block, InputKind } from "@chat/protocol";
import {
  blockEvents,
  blocks,
  Bot,
  CardDraft,
  ChatError,
  checkCardV2,
  composeCard,
  MemoryStateStore,
  opsFit,
} from "../src/index.ts";
import { FakeServer, GROUP_ID, message, TOPIC_ID } from "./fake.ts";

const ROOT = join(import.meta.dir, "../../..");
// The server's rule, loaded at run time (its file is outside this package's typecheck).
const serverInputs = (await import(join(ROOT, "server/src/cards2/inputs.ts"))) as { eventsOf(b: CardV2Block): string[] };

const ART = "01JC0ART00000000000000OCT3";
const bots: Bot[] = [];
afterEach(async () => {
  for (const b of bots.splice(0)) await b.stop();
});

async function start(server: FakeServer): Promise<Bot> {
  const bot = new Bot({ url: "http://127.0.0.1:1", token: "t", WebSocket: server.WebSocket as never, stateStore: new MemoryStateStore(), logLevel: "error", stopTimeoutMs: 500 });
  bots.push(bot);
  await bot.start();
  return bot;
}

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

describe("live events: what a block can send", () => {
  const leaf = (type: string, extra: Record<string, unknown> = {}) => ({ type, id: "b", ...extra }) as unknown as CardV2Block;
  const TYPES = [
    "text", "fields", "table", "list", "progress", "code", "image", "divider", "link", "decision", "draft", "plan", "metric", "chart", "diff",
    "evidence", "ref", "rank", "rating", "scale", "confirm", "imageChoice", "matrix", "slots", "poll", "buttons", "diagram", "embed", "stack",
    "row", "group", "compare", "tabs", "questionnaire",
  ];

  test("blockEvents is the server's eventsOf for every block type and input kind", () => {
    const samples = [...TYPES.map((t) => leaf(t)), ...InputKind.options.map((kind) => leaf("input", { kind }))];
    for (const b of samples) expect([b.type, (b as { kind?: string }).kind, blockEvents(b)]).toEqual([b.type, (b as { kind?: string }).kind, serverInputs.eventsOf(b)]);
    // The list above has every type the builders make.
    const built = new Set(Object.keys(blocks).filter((k) => k !== "slider"));
    for (const t of built) expect(TYPES.includes(t) || t === "input").toBe(true);
  });

  test("blocks.buttons declares button.press unless events is given", () => {
    const b = blocks.buttons({ id: "more", buttons: [{ id: "split", label: "按渠道拆开" }] });
    expect(b.events).toEqual(["button.press"]);
    expect(b.fallbackText).toBe("按渠道拆开");
    expect(blocks.buttons({ id: "more", buttons: [{ id: "split", label: "拆" }], events: [] }).events).toEqual([]);
    // The card the builder alone makes has buttons the server takes a press of.
    const card = composeCard({ intent: "info", title: "月报", blocks: [b] });
    expect(card.blocks[0]?.events).toEqual(["button.press"]);
  });

  test("composeCard refuses an event the block type cannot send, and says what it can", () => {
    const decision = blocks.decision({ id: "check", label: "对不对", options: [{ id: "ok", label: "对" }, { id: "no", label: "不对" }], events: ["chart.select"] });
    const err = caught(() => composeCard({ intent: "info", title: "x", blocks: [decision] }));
    expect(err.chat).toBe(true);
    expect(err.code).toBe("invalid");
    expect(err.message).toBe('card: block "check" (decision) cannot send chart.select; it can send: choice.change, choice.submit');
    expect(caught(() => composeCard({ intent: "info", title: "x", blocks: [{ ...blocks.text("t", "hi"), events: ["button.press"] }] })).message).toBe(
      'card: block "t" (text) cannot send button.press; it sends no live events',
    );
    // input.submit is for single-line inputs only.
    const area = blocks.input({ id: "note", kind: "textarea", label: "备注", events: ["input.submit"] });
    expect(caught(() => composeCard({ intent: "info", title: "x", blocks: [area] })).message).toContain('block "note" (input textarea) cannot send input.submit; it can send: input.change');
    // An embed's events are its page's names, not card events.
    expect(() => composeCard({ intent: "info", title: "x", blocks: [blocks.embed({ id: "p", artifactId: ART, version: 1, events: ["signup.join"] })] })).not.toThrow();
    // A nested block is checked too.
    expect(() => composeCard({ intent: "info", title: "x", blocks: [blocks.stack("s", [{ ...blocks.metric({ id: "m", label: "日活", value: 1 }), events: ["table.sort"] }])] })).toThrow(/block "m" \(metric\) cannot send table.sort/);
  });
});

describe("card errors name the block and list more than one problem", () => {
  const m = (id: string) => blocks.metric({ id, label: id, value: 1 });

  test("a repeated id, two primary actions and a bad field say which", () => {
    expect(caught(() => composeCard({ intent: "info", title: "x", blocks: [m("a"), m("b"), m("a")] })).message).toBe("card: card.limits: block ids are unique (used twice: a)");
    const two = caught(() =>
      composeCard({ intent: "ask", title: "x", ask: { from: ["owners"] }, blocks: [m("a")], actions: [{ id: "go", label: "好", style: "primary" }, { id: "send", label: "发", style: "primary" }] }),
    );
    expect(two.message).toBe("card: actions: card.limits: at most one primary action (primary: go, send)");
    const field = caught(() => checkCardV2({ v: 2, rev: 1, intent: "info", title: "x", fallback: "x", blocks: [m("a"), { type: "chart", id: "trend", kind: "line", x: ["1"], series: [{ name: "s", values: ["1"] }] } as never] }));
    expect(field.message).toStartWith('card: block "trend" (chart) series.0.values.0: ');
    // A write rule names its block itself.
    expect(caught(() => composeCard({ intent: "info", title: "x", blocks: [blocks.row("r", [m("a"), m("b"), m("c"), m("d")])] })).message).toBe("card: card.limits: row r: 2..3 children");
  });

  test("up to 5 problems, then a count", () => {
    const bad = (id: string) => ({ type: "metric", id, label: 1, value: "x" }) as never;
    const err = caught(() => checkCardV2({ v: 2, rev: 1, intent: "info", title: "x", fallback: "x", blocks: [bad("a"), bad("b"), bad("c"), bad("d")] }));
    const lines = err.message.replace(/^card: /, "").split("; ");
    expect(lines).toHaveLength(6);
    expect(lines.slice(0, 5).map((l) => l.split(":")[0])).toEqual(['block "a" (metric) label', 'block "a" (metric) value', 'block "b" (metric) label', 'block "b" (metric) value', 'block "c" (metric) label']);
    expect(lines[5]).toBe("and 3 more");
  });
});

describe("CardDraft checks the ids it is given", () => {
  const card = (): CardV2 =>
    composeCard({
      intent: "info",
      title: "月报",
      blocks: [
        blocks.row("kpis", [blocks.metric({ id: "m_dau", label: "日活", value: 2340 }), blocks.metric({ id: "m_groups", label: "新增群", value: 38 })]),
        blocks.tabs({ id: "tabs", tabs: [{ id: "t1", label: "图", children: [blocks.text("a", "x")] }, { id: "t2", label: "表", children: [blocks.text("b", "y")] }] }),
      ],
    });

  test("set, replace, remove and an insert's placement throw for a block the card does not have", () => {
    const d = new CardDraft(card());
    const err = caught(() => d.set("m_dua", { value: 1 }));
    expect(err.chat).toBe(true);
    expect(err.code).toBe("invalid");
    expect(err.message).toBe('set: no block "m_dua" in rev 1; blocks: kpis, m_dau, m_groups, tabs, a, b');
    expect(() => d.replace("nope", blocks.text("nope", "x"))).toThrow(/: replace: no block "nope"/);
    expect(() => d.remove("nope")).toThrow(/: remove: no block "nope"/);
    expect(() => d.insert(blocks.text("n", "x"), { after: "nope" })).toThrow(/: insert "n" after: no block "nope"/);
    expect(() => d.insert(blocks.text("n", "x"), { parent: "nope" })).toThrow(/: insert "n" parent: no block "nope"/);
    expect(() => d.insert(blocks.text("a", "again"))).toThrow('rev 1 has a block "a" already; use set or replace');
    // A container whose nested child is in the card already: the error names the container and the
    // child, and the container's own op is not recorded either.
    const kids = () => [blocks.metric({ id: "m_dau", label: "日活", value: 2 }), blocks.metric({ id: "m_new", label: "新增", value: 3 })];
    expect(() => d.replace("kpis", blocks.row("kpis", kids()))).toThrow(
      'replace "kpis": rev 1 has its child "m_dau" already; change that block with set or replace, or remove it first',
    );
    expect(() => d.insert(blocks.row("more", kids()))).toThrow('insert "more": rev 1 has its child "m_dau" already');
    // Nothing was recorded by the refused calls.
    expect(d.ops).toEqual([]);
  });

  test("ids the draft inserts count, ids it removes do not; a tab id is a parent", () => {
    const d = new CardDraft(card());
    d.insert(blocks.group({ id: "drill", title: "拆开", children: [blocks.text("t", "x")] }), { after: "kpis" });
    d.set("t", { text: "y" }).insert(blocks.text("c", "z"), { parent: "t2" }).insert(blocks.text("u", "u"), { parent: "drill" });
    d.remove("kpis");
    expect(() => d.set("m_dau", { value: 1 })).toThrow(/no block "m_dau" in rev 1; blocks: tabs, a, b, drill, t, c, u$/);
    d.remove("drill");
    expect(() => d.set("t", { text: "z" })).toThrow(ChatError);
    // A replaced block keeps its id and brings its nested children.
    d.replace("a", blocks.stack("ignored", [blocks.text("inner", "in")]));
    d.set("inner", { text: "ok" });
    expect(d.ops.at(-3)).toEqual({ op: "replace", id: "a", block: { type: "stack", id: "a", children: ["inner"] } });
    expect(opsFit(card(), d.ops)).toBe(true);
  });

  test("set takes the block's fields by type, or loose props", () => {
    const d = new CardDraft(card());
    d.set<"metric">("m_dau", { value: 2341, delta: 1 });
    // @ts-expect-error a metric has no `text`
    d.set<"metric">("m_dau", { text: "x" });
    d.set("m_groups", { value: 40 });
    expect(d.ops).toHaveLength(3);
  });

  test("a draft with a wrong id rejects the patch before a frame is sent; a raw op list goes to the server", async () => {
    const server = new FakeServer();
    server.on("send", (f) => ({ ok: true, result: { message: message({ id: "01JC0MSG0000000000000000D1", kind: "card", cardV2: f.cardV2 }) } }));
    server.on("card.patch", () => ({ ok: false, error: { code: "invalid", message: "op 0: no block nope" } }));
    const bot = await start(server);
    const posted = await bot.postCardV2({ groupId: GROUP_ID, topicId: TOPIC_ID }, card());
    const err = (await bot.patchCard(GROUP_ID, posted.message.id, (d) => d.set("nope", { value: 1 })).catch((e) => e)) as ChatError;
    expect(text(err)).toStartWith('set: no block "nope" in rev 1');
    expect(server.of("card.patch")).toHaveLength(0);
    const raw = (await bot.patchCard(GROUP_ID, posted.message.id, [{ op: "set", id: "nope", props: { value: 1 } }]).catch((e) => e)) as ChatError;
    expect(text(raw)).toBe("op 0: no block nope");
    expect(server.of("card.patch")).toHaveLength(1);
  });
});
