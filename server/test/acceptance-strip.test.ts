// The posted result leaves out the `acceptance` block: the done card shows the criteria.
import { describe, expect, it } from "vitest";
import { stripAcceptance } from "../src/bots/dispatch";

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
});
