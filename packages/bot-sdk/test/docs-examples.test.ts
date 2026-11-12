// The card examples of docs/bots.md, run against the shipped protocol: every ```ts block
// that builds a card with `composeCard` runs with stub `bot`
// calls. Each posted card, each patch its handlers make and each patched rev must pass the
// protocol schema and the server's patch rules. Run: bun test test/docs-examples.test.ts
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
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
  const examples = fences("ts").filter((s) => s.includes("composeCard("));

  test("each card example posts cards and patches that the shipped protocol takes", async () => {
    expect(examples.length).toBeGreaterThan(0);
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
