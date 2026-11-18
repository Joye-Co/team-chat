// The posted result leaves out the `acceptance` block: the done card shows the criteria.
import { describe, expect, it } from "vitest";
import { buildPrompt, stripAcceptance } from "../src/bots/dispatch";

describe("stripAcceptance", () => {
  it("drops closed acceptance fences and keeps the rest", () => {
    const text = [
      "我写好了页面。",
      "",
      "```acceptance",
      '[{ "text": "创建一个 HTML 页面", "state": "pass" }]',
      "```",
      "",
      "",
      "```js",
      "x()",
      "```",
    ].join("\n");
    expect(stripAcceptance(text)).toBe("我写好了页面。\n\n```js\nx()\n```");
  });

  it("handles longer fences and keeps an unclosed one", () => {
    expect(stripAcceptance("a\n````acceptance\n[]\n````")).toBe("a");
    expect(stripAcceptance("a\n```acceptance\n[]")).toBe("a\n```acceptance\n[]");
  });

  it("is empty when the block was all there was", () => {
    expect(stripAcceptance("```acceptance\n[]\n```")).toBe("");
  });

  it("asks for the block only for checkable work", () => {
    const prompt = buildPrompt({ botName: "J", title: "t", messages: [], names: new Map(), request: { authorId: "a", body: "hi" }, acceptance: true });
    expect(prompt).toContain("Skip the block for questions");
  });

  it("teaches evidence that shows as passed, not the form the done card turns into a claim", () => {
    const prompt = buildPrompt({ botName: "J", title: "t", messages: [], names: new Map(), request: { authorId: "a", body: "hi" }, acceptance: true });
    expect(prompt).toContain("with text alone it shows as your claim");
    expect(prompt).toContain("Test counts come from the run's log");
    // The example is valid JSON and every `pass` in it has what the done card needs (proof.ts).
    const example = prompt.split("```acceptance\n")[1]?.split("\n```")[0] ?? "";
    const items = JSON.parse(example) as { state: string; evidence?: { url?: string; fileId?: string; artifact?: unknown; text?: string } }[];
    expect(items.length).toBeGreaterThanOrEqual(2);
    for (const i of items.filter((x) => x.state === "pass")) expect(Boolean(i.evidence?.url || i.evidence?.fileId || i.evidence?.artifact)).toBe(true);
    expect(example).not.toContain("passed");
  });
});

describe("the first rule of a run's prompt", () => {
  const base = { botName: "J", title: "t", messages: [], names: new Map(), request: { authorId: "a", body: "hi" } };

  it("sends long results to a 产出 or a card on a machine that has chat-run", () => {
    const prompt = buildPrompt({ ...base, context: "[#1 m M · a · now] hi", contextTools: true });
    expect(prompt).toContain("goes into a 产出 or a card before you end");
    expect(prompt).toContain("one or two sentences, in the language of the request");
    expect(prompt).not.toContain("posted into the topic as is");
  });

  it("keeps the short rule where there is no chat-run to make one with", () => {
    const prompt = buildPrompt(base);
    expect(prompt).toContain("posted into the topic as is, so keep it short");
    expect(prompt).not.toContain("产出 or a card");
  });
});
