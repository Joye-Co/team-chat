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

/**
 * What an example may use without declaring it. Each declaration is added only when the example
 * names the identifier and does not declare it itself. One line in the file, so a line of the
 * example is its line in the file minus 1.
 */
const AMBIENT: Record<string, string> = {
  bot: 'declare const bot: import("@chat/bot-sdk").Bot;',
  ctx: 'declare const ctx: import("@chat/bot-sdk").MessageContext;',
  groupId: "declare const groupId: string;",
  topicId: "declare const topicId: string;",
  fileId: "declare const fileId: string;",
  channelRows: "declare function channelRows(): Promise<string[][]>;",
  PAGE: "declare const PAGE: string;",
};

/** Typechecks every ```ts block of the files; returns the errors as `<file>:<line>: <message>`. */
async function typecheckFences(files: string[]): Promise<{ count: number; errors: string[] }> {
  const pkg = join(import.meta.dir, "..");
  const dir = join(pkg, ".tmp/doc-fences");
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  /** Module file name -> the doc file and the line of the block's first code line. */
  const where = new Map<string, { file: string; line: number }>();
  for (const file of files) {
    const doc = readFileSync(join(ROOT, file), "utf8");
    const re = /^```ts\n([\s\S]*?)^```$/gm;
    for (let m = re.exec(doc); m; m = re.exec(doc)) {
      const src = m[1] as string;
      const name = `fence-${String(where.size + 1).padStart(2, "0")}.ts`;
      const ambient = Object.entries(AMBIENT)
        .filter(([id]) => new RegExp(`\\b${id}\\b`).test(src) && !new RegExp(`\\b(const|let|var|function)\\s+${id}\\b`).test(src))
        .map(([, decl]) => decl)
        .join(" ");
      writeFileSync(join(dir, name), `${ambient}\n${src}\nexport {};\n`);
      where.set(name, { file, line: doc.slice(0, m.index).split("\n").length + 1 });
    }
  }
  writeFileSync(
    join(dir, "tsconfig.json"),
    JSON.stringify({ extends: "../../tsconfig.json", compilerOptions: { paths: { "@chat/bot-sdk": ["../../src/index.ts"] } }, include: ["./*.ts"] }),
  );
  // The repo's TypeScript is the native compiler (no JS API to call): run its command.
  const tsc = Bun.spawn([process.execPath, "x", "tsc", "--noEmit", "--pretty", "false", "-p", dir], { cwd: pkg, stdout: "pipe", stderr: "pipe" });
  const [out, err, code] = await Promise.all([new Response(tsc.stdout).text(), new Response(tsc.stderr).text(), tsc.exited]);
  const errors: string[] = [];
  for (const line of `${out}\n${err}`.split("\n")) {
    const m = /(fence-\d+\.ts)\((\d+),\d+\): (.*)$/.exec(line);
    const at = m && where.get(m[1] as string);
    if (m && at) errors.push(`${at.file}:${at.line + Number(m[2]) - 2}: ${m[3]}`);
    else if (/error/i.test(line)) errors.push(line.trim());
  }
  if (code !== 0 && errors.length === 0) errors.push(`tsc exited with ${code}: ${(out + err).trim().slice(0, 500)}`);
  if (errors.length === 0) rmSync(dir, { recursive: true, force: true });
  return { count: where.size, errors };
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

  test("every ts block of the guide and the README typechecks against the SDK", async () => {
    const { count, errors } = await typecheckFences(["docs/bots.md", "packages/bot-sdk/README.md"]);
    expect(count).toBeGreaterThanOrEqual(8);
    expect(errors).toEqual([]);
  }, 120_000);

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
    const documented = [...errors.matchAll(/`([a-z_]+)`/g)].map((m) => m[1] as string).filter((c) => c !== "code" && c !== "message" && c !== "ask");
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
