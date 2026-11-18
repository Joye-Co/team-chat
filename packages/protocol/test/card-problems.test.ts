// The write rules of a card as a list: every problem, in the order `cardV2Problem` finds them,
// and problems that name what they are about.
import { describe, expect, test } from "bun:test";
import { type CardV2Block, CardV2, cardBlockEvents, cardV2Problem, cardV2Problems, CardV2Shape } from "../src/index.ts";

const shape = (over: Record<string, unknown>) =>
  CardV2Shape.parse({ v: 2, catalog: "tc.card/2.1", rev: 1, intent: "info", title: "t", fallback: "t", blocks: [], ...over });
const text = (id: string) => ({ type: "text", id, text: id });

describe("cardV2Problems", () => {
  test("lists every broken rule, the first one being cardV2Problem's", () => {
    const card = shape({
      blocks: [text("a"), text("a"), { type: "stack", id: "s", children: ["nope"] }, { type: "row", id: "r", children: ["a"] }],
      actions: [
        { id: "go", label: "好", style: "primary" },
        { id: "send", label: "发", style: "primary" },
      ],
    });
    const all = [...cardV2Problems(card)];
    expect(all.map((p) => p.message)).toEqual([
      "block ids are unique (used twice: a)",
      "stack s names no block nope",
      "row r: 2..3 children",
      "at most one primary action (primary: go, send)",
    ]);
    expect(all[0]).toEqual(cardV2Problem(card) as never);
  });

  test("a valid card has none", () => {
    expect([...cardV2Problems(shape({ blocks: [text("a")] }))]).toEqual([]);
    expect(cardV2Problem(shape({ blocks: [text("a")] }))).toBeUndefined();
  });

  test("a child that does not exist does not stop the walk", () => {
    const card = shape({ blocks: [{ type: "stack", id: "s", children: ["gone"] }, { type: "stack", id: "t", children: ["s"] }] });
    expect([...cardV2Problems(card)].map((p) => p.message)).toEqual(["stack s names no block gone"]);
  });

  test("the schema reports each problem as its own issue, the primary action under actions", () => {
    const r = CardV2.safeParse({
      v: 2,
      rev: 1,
      intent: "info",
      title: "t",
      fallback: "t",
      blocks: [text("a"), text("a"), text("b"), text("b")],
      actions: [
        { id: "go", label: "好", style: "primary" },
        { id: "send", label: "发", style: "primary" },
      ],
    });
    expect(r.success).toBe(false);
    expect(r.error?.issues.map((i) => [i.path.join("."), i.message])).toEqual([
      ["blocks", "card.limits: block ids are unique (used twice: a, b)"],
      ["actions", "card.limits: at most one primary action (primary: go, send)"],
    ]);
  });

  test("repeated action ids are named", () => {
    const card = shape({ actions: [{ id: "x", label: "一" }, { id: "x", label: "二" }] });
    expect(cardV2Problem(card)).toEqual({ reason: "card.limits", message: "action ids are unique (used twice: x)", path: ["actions"] });
  });
});

describe("cardBlockEvents", () => {
  const events = (b: Record<string, unknown>) => cardBlockEvents(b as CardV2Block);
  test("says which live events a block can send", () => {
    expect(events({ type: "decision" })).toEqual(["choice.change", "choice.submit"]);
    expect(events({ type: "input", kind: "text" })).toEqual(["input.change", "input.submit"]);
    expect(events({ type: "input", kind: "slider" })).toEqual(["input.change"]);
    expect(events({ type: "chart" })).toEqual(["chart.select", "chart.clear"]);
    expect(events({ type: "buttons" })).toEqual(["button.press"]);
    expect(events({ type: "text" })).toEqual([]);
  });
});
