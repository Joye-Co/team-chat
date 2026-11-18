// A refused card says everything that is wrong with it at once, and `replace` of a container
// takes the children it no longer names with it.
import { type CardV2, type CardV2Block, cardBlockEvents, type PatchOp } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import { eventsOf } from "../src/cards2/inputs";
import { applyPatch, checkCard } from "../src/cards2/patch";
import { type ApiError, issuesText } from "../src/core/errors";
import { Daemon, mention, world } from "./daemon";
import { Client, clientMsgId } from "./helpers";

const text = (id: string): CardV2Block => ({ type: "text", id, text: id }) as CardV2Block;
const stack = (id: string, children: string[]): CardV2Block => ({ type: "stack", id, children }) as CardV2Block;
const card = (blocks: CardV2Block[]): CardV2 => ({ v: 2, catalog: "tc.card/2.1", rev: 1, intent: "info", title: "t", fallback: "t", blocks }) as CardV2;
const ids = (c: CardV2) => c.blocks.map((b) => b.id);
const refused = (f: () => unknown): ApiError => {
  try {
    f();
  } catch (e) {
    return e as ApiError;
  }
  throw new Error("not refused");
};

describe("a refused card lists every problem", () => {
  const primaries = [
    { id: "go", label: "好", style: "primary" },
    { id: "send", label: "发", style: "primary" },
  ];

  it("checkCard names all of them and keeps the first reason", () => {
    const e = refused(() => checkCard({ ...card([text("a"), text("a"), stack("s", ["nope"])]), actions: primaries }));
    expect(e.code).toBe("invalid");
    expect(e.reason).toBe("card.limits");
    expect(e.message).toContain("block ids are unique (used twice: a)");
    expect(e.message).toContain("stack s names no block nope");
    expect(e.message).toContain("actions card.limits: at most one primary action (primary: go, send)");
  });

  it("a patch that breaks two rules reports both", () => {
    const ops: PatchOp[] = [
      { op: "insert", block: stack("t", ["missing"]) },
      { op: "card", props: { actions: primaries as never } },
    ];
    const e = refused(() => applyPatch(card([text("a")]), ops));
    expect(e.message).toContain("stack t names no block missing");
    expect(e.message).toContain("at most one primary action");
  });

  it("issuesText lists at most five, says how many more and repeats nothing", () => {
    const issue = (n: number) => ({ path: ["blocks", n], message: `bad ${n}` });
    expect(issuesText([0, 1, 2, 3, 4, 5, 6].map(issue))).toBe("blocks.0 bad 0; blocks.1 bad 1; blocks.2 bad 2; blocks.3 bad 3; blocks.4 bad 4; and 2 more");
    expect(issuesText([issue(1), issue(1)])).toBe("blocks.1 bad 1");
    expect(issuesText([{ path: [], message: "bad" }])).toBe("(root) bad");
  });

  it("issuesText puts the issue that names the rule first", () => {
    const plain = { path: ["title"], message: "too long" };
    const named = { path: ["blocks"], message: "card.limits: at most 80 blocks" };
    expect(issuesText([plain, named])).toBe("blocks card.limits: at most 80 blocks; title too long");
  });
});

describe("a refused card over the wire", () => {
  const broken = {
    v: 2,
    catalog: "tc.card/2.1",
    rev: 1,
    intent: "info",
    title: "t",
    fallback: "t",
    blocks: [text("a"), text("a"), stack("s", ["nope"])],
  };

  it("chat-run's card.post gets every problem", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["output"] });
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 发一张卡");
    const { runId } = await d.next((f) => f.t === "run.assign" && f.topicId === topicId);
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.id === runId && e.data.run.status === "running");
    d.event(runId, { type: "output", outputId: "o1", op: "card.post", body: { cardV2: broken } });
    const res = await d.next((f) => f.t === "run.output.result" && f.outputId === "o1");
    expect(res.ok).toBe(false);
    expect(res.error.reason).toBe("card.limits");
    expect(res.error.message).toContain("block ids are unique (used twice: a)");
    expect(res.error.message).toContain("stack s names no block nope");
    c.close();
    d.close();
  });

  it("a WebSocket send gets every problem", async () => {
    const w = await world();
    const { c } = await Client.connect(w.human.token);
    const topicId = await mention(c, w, "@dev 发一张卡");
    const ack = await c.request({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "", cardV2: broken });
    expect(ack.ok).toBe(false);
    expect(ack.error.reason).toBe("card.limits");
    expect(ack.error.message).toContain("block ids are unique (used twice: a)");
    expect(ack.error.message).toContain("stack s names no block nope");
    c.close();
  });
});

describe("replace of a container", () => {
  const start = card([stack("s", ["a", "b"]), text("a"), text("b"), text("c")]);

  it("removes a child the new block no longer names", () => {
    expect(ids(applyPatch(start, [{ op: "replace", id: "s", block: stack("s", ["a"]) }]))).toEqual(["s", "a", "c"]);
  });

  it("removes every child when the container becomes a leaf", () => {
    expect(ids(applyPatch(start, [{ op: "replace", id: "s", block: text("s") }]))).toEqual(["s", "c"]);
  });

  it("removes the children of a removed child container too", () => {
    const nested = card([stack("s", ["t", "c"]), stack("t", ["a"]), text("a"), text("c")]);
    expect(ids(applyPatch(nested, [{ op: "replace", id: "s", block: stack("s", ["c"]) }]))).toEqual(["s", "c"]);
  });

  it("keeps a child that another container takes in the same patch", () => {
    const two = card([stack("s", ["a", "b"]), stack("t", ["c"]), text("a"), text("b"), text("c")]);
    const next = applyPatch(two, [
      { op: "replace", id: "s", block: stack("s", ["a"]) },
      { op: "replace", id: "t", block: stack("t", ["c", "b"]) },
    ]);
    expect(ids(next)).toEqual(["s", "t", "a", "b", "c"]);
  });

  it("keeps a block inserted again at the top level in the same patch", () => {
    const next = applyPatch(start, [
      { op: "replace", id: "s", block: stack("s", ["a"]) },
      { op: "remove", id: "b" },
      { op: "insert", block: text("b") },
    ]);
    expect(ids(next)).toEqual(["s", "a", "c", "b"]);
  });

  it("leaves a replace that keeps its children alone", () => {
    expect(ids(applyPatch(start, [{ op: "replace", id: "s", block: stack("s", ["b", "a"]) }]))).toEqual(["s", "a", "b", "c"]);
  });
});

it("the server's event table is the protocol's", () => {
  expect(eventsOf).toBe(cardBlockEvents);
});
