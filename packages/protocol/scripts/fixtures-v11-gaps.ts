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

  // PC-8: 记一条, decisions recorded by hand.
  const recorded = { id: "01JC0DEC000000000000000001", seq: 412, topicId: ID.topic, text: "周五停止合并，下周一发版。", recordedBy: JOYE, recordedAt: T0 };
  ok("entities/recorded-decision.json", "RecordedDecision", recorded);
  ok("entities/recorded-decision-group.json", "RecordedDecision", { ...recorded, topicId: null }, "recorded for the group, not a 话题");
  ok("http/record-decision-request.json", "RecordDecisionRequest", { text: "周五停止合并，下周一发版。", topicId: ID.topic });
  ok("http/record-decision-request-minimal.json", "RecordDecisionRequest", { text: "用 Postgres。" });
  bad("http/record-decision-request-blank.json", "RecordDecisionRequest", { text: "   " }, "text is required");
  bad("http/record-decision-request-too-long.json", "RecordDecisionRequest", { text: "长".repeat(1001) }, "at most 1000 characters");
  ok("http/record-decision-response.json", "RecordDecisionResponse", { decision: recorded });
  ok("http/decisions-response-recorded.json", "DecisionsResponse", { decisions: [], hasMore: false, recorded: [recorded] }, "decisions with the ones recorded by hand");
  ok("events/decision.recorded.json", "Event", { seq: 412, groupId: G, at: T0, actorId: JOYE, type: "decision.recorded", data: { decision: recorded } });

  // PC-9: the author's work state on the card (红 失败, 蓝 处理中).
  const done = { v: 2, catalog: "tc.card/2.1", rev: 2, intent: "done", title: "发版", fallback: "发版", blocks: [{ id: "t", type: "text", text: "发版失败：签名过期。" }] };
  ok("entities/card-v2-status-failed.json", "CardV2", { ...done, status: "failed" }, "the author marked the card failed");
  ok("entities/card-v2-status-executing.json", "CardV2", { ...done, intent: "ask", status: "executing", ask: { from: [JOYE] }, actions: [{ id: "ok", label: "好" }] }, "an answered ask the author is carrying out");
  bad("entities/card-v2-status-unknown.json", "CardV2", { ...done, status: "paused" }, "status is executing or failed");
  ok("client/card.patch-status.json", "ClientFrame", { t: "card.patch", rid: "r204", groupId: G, messageId: M, baseRev: 2, ops: [{ op: "card", props: { status: "failed" } }] });
  ok("client/card.patch-status-clear.json", "ClientFrame", { t: "card.patch", rid: "r205", groupId: G, messageId: M, baseRev: 3, ops: [{ op: "card", props: { status: null } }] }, "null clears the status");
}
