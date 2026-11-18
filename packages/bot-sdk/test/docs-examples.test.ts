// The card examples of docs/bots.md, run against the shipped protocol: every ```json block
// decodes, and every ```ts block that builds a card with `composeCard` runs with stub `bot`
// calls. Each posted card, each patch its handlers make and each patched rev must pass the
// protocol schema and the server's patch rules. Every ```ts block of docs/bots.md and of the
// package README is also typechecked against the SDK. Run: bun test test/docs-examples.test.ts
import { describe, expect, test } from "bun:test";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CardV2, type PatchOp } from "@chat/protocol";
import { blocks, CardDraft, composeCard, type PatchInput } from "../src/index.ts";

const ROOT = join(import.meta.dir, "../../..");
const DOC = readFileSync(join(ROOT, "docs/bots.md"), "utf8");

// The server's patch rule, loaded at run time (its file is outside this package's typecheck).
const serverPatch = (await import(join(ROOT, "server/src/cards2/patch.ts"))) as {
  applyPatch(card: CardV2, ops: PatchOp[]): CardV2;
};

function fences(lang: string): string[] {
  const out: string[] = [];
  const re = new RegExp(`^\`\`\`${lang}\\n([\\s\\S]*?)^\`\`\`$`, "gm");
  for (let m = re.exec(DOC); m; m = re.exec(DOC)) out.push(m[1] as string);
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

type Handler = (ctx: Record<string, unknown>) => Promise<void>;

/** Runs one example with stubs; returns the posted cards and the handlers it set. */
async function run(source: string) {
  const posted: CardV2[] = [];
  const handlers: { event: Handler[]; answer: Handler[] } = { event: [], answer: [] };
  const bot = {
    postCardV2: async (_target: unknown, card: CardV2) => {
      posted.push(card);
      return { message: { id: "m1" } };
    },
    onCardEvent: (h: Handler) => handlers.event.push(h),
    onCardAnswer: (h: Handler) => handlers.answer.push(h),
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
  const fn = new Function("blocks", "composeCard", "bot", "groupId", "topicId", "channelRows", `return (async () => {\n${js}\n})();`);
  await fn(blocks, composeCard, bot, "g1", "t1", channelRows);
  return { posted, handlers };
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
            const ctx = { blockId: d.id, name: "choice.change", value: { value: o.id }, card, patch };
            for (const h of handlers.event) await h(ctx);
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
      }
    }
  });
});
