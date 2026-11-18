// The examples of docs/bots.md (and the SDK README), checked against the shipped SDK, protocol
// and page bridge, so the guide cannot drift from the code:
// - every ```json block decodes, and every ```ts block typechecks against the SDK;
// - every ```ts block that builds a card with `composeCard` runs with stub `bot` calls: each
//   posted card, each patch its handlers make and each patched rev must pass the protocol schema
//   and the server's patch rules;
// - the page example: its declared events pass the server's rules, and every `teamchat.emit` /
//   `teamchat.ask` of its ```html block is declared with that kind and has a handler;
// - the `window.teamchat` reference names exactly the members and error codes of the real bridge
//   (crates/artifact-view/src/bridge.rs).
// Run: bun test test/docs-examples.test.ts
import { describe, expect, test } from "bun:test";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type ArtifactEventDecl, CardV2, type PatchOp } from "@chat/protocol";
import { blocks, CardDraft, checkArtifactData, checkArtifactEvents, composeCard, eventValueReaders, type PatchInput, type PublishPageInput } from "../src/index.ts";

const PKG = join(import.meta.dir, "..");
const ROOT = join(PKG, "../..");
const DOC = readFileSync(join(ROOT, "docs/bots.md"), "utf8");
const README = readFileSync(join(PKG, "README.md"), "utf8");
const BRIDGE = readFileSync(join(ROOT, "crates/artifact-view/src/bridge.rs"), "utf8");

// The server's patch rule, loaded at run time (its file is outside this package's typecheck).
const serverPatch = (await import(join(ROOT, "server/src/cards2/patch.ts"))) as {
  applyPatch(card: CardV2, ops: PatchOp[]): CardV2;
};

function fences(lang: string, doc = DOC): string[] {
  const out: string[] = [];
  const re = new RegExp(`^\`\`\`${lang}\\n([\\s\\S]*?)^\`\`\`$`, "gm");
  for (let m = re.exec(doc); m; m = re.exec(doc)) out.push(m[1] as string);
  return out;
}

type Ctx = Record<string, unknown>;
type Handler = (ctx: Ctx) => Promise<void>;
type Filter = { name?: string; block?: string };

/** `on…(handler)` or `on…(filter, handler)`, as the SDK takes them. */
function filtered(a: unknown, b: unknown): { filter: Filter; run: Handler } {
  if (typeof a === "function") return { filter: {}, run: a as Handler };
  return { filter: typeof a === "string" ? { name: a } : (a as Filter), run: b as Handler };
}

/** Runs one example with stubs; returns what it posted and published, and the handlers it set. */
async function run(source: string, page = "") {
  const posted: CardV2[] = [];
  const pages: PublishPageInput[] = [];
  const handlers = { event: [] as { filter: Filter; run: Handler }[], answer: [] as Handler[], page: [] as { filter: Filter; run: Handler }[] };
  const bot = {
    postCardV2: async (_target: unknown, card: CardV2) => {
      posted.push(card);
      return { message: { id: "m1" } };
    },
    publishPage: async (_target: unknown, input: PublishPageInput) => {
      // What the SDK checks before it uploads anything.
      checkArtifactEvents(input.events);
      checkArtifactData(input.data);
      pages.push(input);
      return { artifact: { id: "01JC0ART0000000000000000P1" }, version: { artifactId: "01JC0ART0000000000000000P1", version: pages.length } };
    },
    onCardEvent: (a: unknown, b?: unknown) => handlers.event.push(filtered(a, b)),
    onCardAnswer: (h: Handler) => handlers.answer.push(h),
    onArtifactEvent: (a: unknown, b?: unknown) => handlers.page.push(filtered(a, b)),
  };
  // The rows the example reads from its own data source: 渠道, 新增群, 占比, and the total as a
  // normal last row (Card v2 has no summaryRow).
  const channelRows = async () => [
    ["微博", "15", "39%"],
    ["邀请", "11", "29%"],
    ["B 站", "7", "18%"],
    ["抖音", "5", "13%"],
  ];
  const js = new Bun.Transpiler({ loader: "ts" }).transformSync(source.replace(/^import .*$/gm, ""));
  const fn = new Function("blocks", "composeCard", "bot", "groupId", "topicId", "channelRows", "PAGE", `return (async () => {\n${js}\n})();`);
  await fn(blocks, composeCard, bot, "g1", "t1", channelRows, page);
  return { posted, pages, handlers };
}

describe("docs/bots.md examples", () => {
  test("every json block decodes", () => {
    for (const j of fences("json")) expect(() => JSON.parse(j)).not.toThrow();
  });

  test("every ts block of the guide and the README typechecks against the SDK", () => {
    const blocksOf = [...fences("ts").map((s) => ["docs/bots.md", s] as const), ...fences("ts", README).map((s) => ["README.md", s] as const)];
    expect(blocksOf.length).toBeGreaterThanOrEqual(8);
    // What an example may use without declaring it: a started bot, a handler context, ids, its
    // own data source and the page of the page example. A block that makes its own `bot` shadows.
    const ambient = [
      'import type * as __sdk from "@chat/bot-sdk";',
      "declare const bot: __sdk.Bot;",
      "declare const ctx: __sdk.MessageContext;",
      "declare const groupId: string;",
      "declare const topicId: string;",
      "declare const fileId: string;",
      "declare const channelRows: () => Promise<string[][]>;",
      "declare const PAGE: string;",
    ].join("\n");
    const dir = join(PKG, ".tmp", "doc-fences");
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const names: string[] = [];
    blocksOf.forEach(([doc, src], i) => {
      const lines = src.split("\n");
      // Imports stay at the top of the module (also the ones that span lines); the rest is the body.
      const imports: string[] = [];
      const body: string[] = [];
      let inImport = false;
      for (const line of lines) {
        if (inImport || /^import\s/.test(line)) {
          imports.push(line);
          inImport = !/from\s+["'][^"']+["'];?\s*(\/\/.*)?$/.test(line);
        } else body.push(line);
      }
      const name = `fence-${String(i + 1).padStart(2, "0")}.ts`;
      names.push(`${name} (${doc})`);
      writeFileSync(join(dir, name), `${imports.join("\n")}\n${ambient}\nexport async function example(): Promise<void> {\n${body.join("\n")}\n}\n`);
    });
    writeFileSync(
      join(dir, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: {
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "bundler",
          strict: true,
          noEmit: true,
          skipLibCheck: true,
          allowImportingTsExtensions: true,
          verbatimModuleSyntax: true,
          types: ["bun"],
          typeRoots: [join(PKG, "node_modules/@types")],
          paths: { "@chat/bot-sdk": [join(PKG, "src/index.ts")] },
        },
        include: ["*.ts"],
      }),
    );
    const r = Bun.spawnSync([join(PKG, "node_modules/.bin/tsc"), "--noEmit", "-p", join(dir, "tsconfig.json")], { cwd: dir, stdout: "pipe", stderr: "pipe" });
    const out = `${r.stdout.toString()}${r.stderr.toString()}`.trim();
    expect(r.exitCode === 0 ? "" : `${out}\nblocks: ${names.join(", ")}`).toBe("");
  });

  const examples = fences("ts").filter((s) => s.includes("composeCard("));

  test("the card examples are found", () => {
    expect(examples.length).toBeGreaterThan(0);
  });

  test("each card example posts cards and patches that the shipped protocol takes", async () => {
    let patched = 0;
    for (const src of examples) {
      const { posted, handlers } = await run(src);
      expect(posted.length).toBeGreaterThan(0);
      for (const first of posted) {
        const decoded = CardV2.safeParse(first);
        expect(decoded.success ? "" : JSON.stringify(decoded.error.issues)).toBe("");
        let patches = 0;
        // Each option of each decision, from the posted rev: a live choice, then the stored answer.
        for (const d of first.blocks)
          for (const o of d.type === "decision" ? d.options : []) {
            let card = first;
            const patch = async (input: PatchInput) => {
              let ops: PatchOp[];
              if (Array.isArray(input)) ops = input;
              else {
                const draft = new CardDraft(card);
                input(draft);
                ops = draft.ops;
              }
              patches++;
              card = serverPatch.applyPatch(card, ops);
              const again = CardV2.safeParse(card);
              expect(again.success ? "" : JSON.stringify(again.error.issues)).toBe("");
              return { cardV2: card };
            };
            const value = { value: o.id };
            const ctx = { blockId: d.id, name: "choice.change", value, card, patch, ...eventValueReaders(value) };
            for (const h of handlers.event) {
              if ((h.filter.name ?? ctx.name) === ctx.name && (h.filter.block ?? d.id) === d.id) await h.run(ctx);
            }
            const answer = {
              ...ctx,
              card,
              askId: first.ask?.id ?? "a1",
              choice: (id = "choice") => (id === d.id ? o.id : undefined),
              choices: (id = "choice") => (id === d.id ? [o.id] : []),
            };
            for (const h of handlers.answer) await h(answer);
          }
        if (handlers.event.length + handlers.answer.length > 0) expect(patches).toBeGreaterThan(0);
        patched += patches;
      }
    }
    // The report example patches its card from a live pick and from the answer.
    expect(patched).toBeGreaterThan(0);
  });

  test("the page example: the page calls only what its bot declares, and every event has a handler", async () => {
    const source = fences("ts").find((s) => s.includes("publishPage("));
    const html = fences("html")[0];
    if (!source || !html) throw new Error("docs/bots.md has no page example (a ts block with publishPage and an html block)");
    const { pages, posted, handlers } = await run(source, html);
    expect(pages).toHaveLength(1);
    const input = pages[0] as PublishPageInput;
    expect(input.html).toBe(html);
    const declared = (input.events ?? []) as ArtifactEventDecl[];
    expect(declared.length).toBeGreaterThan(0);
    const kindOf = (name: string) => {
      const d = declared.find((e) => e.name === name);
      return d ? (d.kind ?? "emit") : undefined;
    };
    // The page: each call names a declared event of that kind, and the bridge is guarded.
    const calls = [...html.matchAll(/\b(?:tc|teamchat)\.(emit|ask)\(\s*["']([^"']+)["']/g)].map((m) => ({ kind: m[1], name: m[2] as string }));
    expect(calls.length).toBeGreaterThan(0);
    for (const c of calls) expect(`${c.kind} ${c.name}: declared as ${kindOf(c.name)}`).toBe(`${c.kind} ${c.name}: declared as ${c.kind}`);
    expect(html).toContain("window.teamchat");
    // The bot: a handler per declared event, and none for a name the version does not declare.
    for (const h of handlers.page) if (h.filter.name !== undefined) expect(kindOf(h.filter.name)).toBeDefined();
    for (const d of declared) expect(handlers.page.some((h) => h.filter.name === undefined || h.filter.name === d.name) ? "" : `no handler for ${d.name}`).toBe("");
    // Every declared event is used by the page.
    for (const d of declared) expect(calls.some((c) => c.name === d.name) ? "" : `the page never sends ${d.name}`).toBe("");
    // The card that shows the page names only declared events.
    const embed = posted.flatMap((c) => c.blocks).find((b) => b.type === "embed");
    expect(embed).toBeDefined();
    for (const name of embed?.events ?? []) expect(kindOf(name)).toBeDefined();
  });
});

describe("docs/bots.md: the window.teamchat reference is the real bridge", () => {
  const section = DOC.slice(DOC.indexOf("### The page side: `window.teamchat`"), DOC.indexOf("**Acceptance criteria"));

  test("the table lists exactly the members the bridge defines", () => {
    const def = BRIDGE.match(/Object\.defineProperty\(window, 'teamchat', \{ value: Object\.freeze\(\{([\s\S]*?)\}\) \}\);/);
    if (!def) throw new Error("bridge.rs no longer defines window.teamchat the way this test reads it");
    const real = [...(def[1] as string).matchAll(/^\s{4}([a-zA-Z]+):/gm)].map((m) => m[1] as string).sort();
    const documented = [...section.matchAll(/^\| `teamchat\.([a-zA-Z]+)/gm)].map((m) => m[1] as string).sort();
    expect(real.length).toBeGreaterThan(0);
    expect(documented).toEqual(real);
  });

  test("the error codes and limits are the bridge's", () => {
    const codes = [...BRIDGE.matchAll(/Reject::[A-Za-z]+ => "([a-z_]+)"/g)].map((m) => m[1] as string);
    expect(codes.length).toBeGreaterThan(0);
    const errors = section.slice(section.indexOf("**Errors.**"), section.indexOf("**Parts to ask about.**"));
    const documented = [...errors.matchAll(/`([a-z_]+)`/g)].map((m) => m[1] as string).filter((c) => c !== "code" && c !== "ask");
    expect([...new Set(documented)].sort()).toEqual([...new Set(codes)].sort());
    const num = (name: string) => {
      const m = BRIDGE.match(new RegExp(`pub const ${name}: [a-z0-9]+ = ([0-9_ *]+);`));
      if (!m) throw new Error(`bridge.rs has no ${name}`);
      return (m[1] as string).split("*").reduce((a, x) => a * Number(x.replace(/_/g, "").trim()), 1);
    };
    expect(num("MAX_PAYLOAD_BYTES")).toBe(16 * 1024);
    expect(section).toContain("at most 16 KiB");
    expect(num("MAX_OPEN_ASKS")).toBe(4);
    expect(section).toContain("at most 4 open asks");
    expect([num("ASK_TIMEOUT_DEFAULT_MS"), num("ASK_TIMEOUT_MAX_MS")]).toEqual([30_000, 120_000]);
    expect(section).toContain("Default 30 s, at most 120 s");
    expect([num("MAX_REF_LABEL_CHARS"), num("MAX_REF_VALUE_BYTES"), num("REF_PER_SECOND")]).toEqual([80, 2048, 2]);
    expect(section).toContain("`label` 1–80 characters, `value` at most 2048 bytes of JSON, at most 2 per second");
  });
});
