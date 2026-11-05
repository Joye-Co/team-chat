import { describe, expect, it } from "vitest";
import { doneCard } from "../src/bots/proof";

describe("done card text", () => {
  it("says its first line once: the title, then only what follows", () => {
    const card = doneCard({
      runId: "r1",
      result: "我写好了页面。`chat-run artifact create` 这条命令需要审批。\n\n页面是一个深色背景的居中卡片。",
      prs: [],
      artifacts: [],
      acceptance: [{ text: "创建一个 HTML 页面", state: "pass" }],
    })!;
    expect(card.title).toBe("我写好了页面。chat-run artifact create 这条命令需要审批。");
    const texts = card.blocks.filter((b) => b.type === "text").map((b) => (b as { text: string }).text);
    expect(texts).toEqual(["页面是一个深色背景的居中卡片。"]);
    expect(card.blocks.some((b) => b.type === "evidence")).toBe(true);
  });

  it("a one-line result is the title only", () => {
    const card = doneCard({ runId: "r1", result: "发好了。", prs: [], artifacts: [], acceptance: [] })!;
    expect(card.title).toBe("发好了。");
    expect(card.blocks.filter((b) => b.type === "text")).toEqual([]);
  });
});
