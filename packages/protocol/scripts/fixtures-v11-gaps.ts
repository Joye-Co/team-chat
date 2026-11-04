// 1.1.0 gap fixes (protocol-cards): draft content hashes, anonymous answer counts and the other
// additive fields of the gap list. Called by gen-fixtures.ts.
import type { InteractCtx } from "./fixtures-v11-interact.ts";

export function addGapFixtures({ ok, bad, T0, ID }: InteractCtx) {
  const G = ID.general as string;
  const M = ID.msg as string;
  const JOYE = ID.owner as string;
  const HASH = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

  // PC-6: the draft text an answer approves, and the count viewers of a private card get.
  ok("client/card.answer-content-hash.json", "ClientFrame", { t: "card.answer", rid: "r201", groupId: G, messageId: M, rev: 2, actionId: "send", editedValues: { d: "明天上午发版。" }, contentHash: HASH }, "a draft answer names the sha256 of the text it approves");
  bad("client/card.answer-content-hash-upper.json", "ClientFrame", { t: "card.answer", rid: "r202", groupId: G, messageId: M, rev: 2, actionId: "send", contentHash: HASH.toUpperCase() }, "contentHash is 64 lowercase hex digits");
  ok("client/card.event-content-hash.json", "ClientFrame", { t: "card.event", rid: "r203", groupId: G, messageId: M, rev: 1, clientEventId: "01JC0CEV000000000000000099", actionId: "send", event: "action.press", contentHash: HASH });
  ok("entities/card-state-anonymous-count.json", "CardState", { ask: { id: "a1", status: "open", from: [JOYE, ID.alice], blocking: false }, answers: [], answerCount: 1 }, "an anonymous questionnaire as another member sees it: the count only");
  void T0;
}
