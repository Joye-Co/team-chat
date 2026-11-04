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

  // PC-7: 引用提问 from a side chat (单独问) to the group it is about.
  const asideRef = { messageId: M, sourceGroupId: G, cardRev: 3, blockId: "channels", part: { kind: "cell", keys: { row: "bili", col: 1 } }, label: "B 站 · 新增群 9" };
  ok("entities/block-ref-input-other-group.json", "BlockRefInput", asideRef, "a side chat quotes a cell of a card in the group it is about");
  ok("entities/block-ref-other-group.json", "BlockRef", { ...asideRef, sourceAuthorId: ID.bot, sourceTopicId: ID.topic });
  bad("entities/block-ref-input-bad-group.json", "BlockRefInput", { ...asideRef, sourceGroupId: "" }, "sourceGroupId is an id");

  // PC-4: the media of a version after the upload.
  const media = { kind: "video", mime: "video/mp4", bytes: 48_000_000, width: 1920, height: 1080, duration: 95.5, posterFileId: "01JC0FIL00000000000POSTER1", preview: "ready" };
  ok("http/put-artifact-media-request.json", "PutArtifactMediaRequest", { media });
  bad("http/put-artifact-media-request-too-long.json", "PutArtifactMediaRequest", { media: { ...media, duration: 5 * 3600 } }, "longer than 4 hours");
  bad("http/put-artifact-media-request-no-preview.json", "PutArtifactMediaRequest", { media: { kind: "video", mime: "video/mp4", bytes: 1 } }, "preview is required");
  ok("http/put-artifact-media-response.json", "PutArtifactMediaResponse", {
    version: { artifactId: "01JC0ART00000000000000OCT3", version: 2, entry: "demo.mp4", files: [{ path: "demo.mp4", fileId: "01JC0FIL00000000000000MP4V", size: 48_000_000, mime: "video/mp4" }], createdBy: ID.bot, createdAt: T0, media },
  });

  // SB-11, PC-3: test counts and a 录屏 on evidence lines.
  const proof = { v: 2, rev: 1, intent: "done", title: "修好了", fallback: "修好了", blocks: [
    { type: "evidence", id: "tests", label: "测试：12 个通过，1 个没通过", state: "fail", tests: { passed: 12, failed: 1, skipped: 2 } },
    { type: "evidence", id: "rec", label: "录屏", state: "pass", recording: { fileId: "01JC0FIL00000000000000MP4V", durationMs: 42_000, posterFileId: "01JC0FIL00000000000POSTER1" } },
  ] };
  ok("entities/card-v2-evidence-tests-recording.json", "CardV2", proof, "the server's test counts and a screen recording");
  bad("entities/card-v2-evidence-tests-negative.json", "CardV2", { ...proof, blocks: [{ type: "evidence", id: "tests", label: "测试", tests: { passed: -1, failed: 0 } }] }, "counts are not negative");
  bad("entities/card-v2-evidence-recording-no-file.json", "CardV2", { ...proof, blocks: [{ type: "evidence", id: "rec", label: "录屏", recording: { durationMs: 1 } }] }, "a recording names its file");

  // SB-10: times and estimates of plan steps.
  ok("http/set-run-plan-request-times.json", "SetRunPlanRequest", { plan: [
    { text: "读代码", state: "done", startedAt: T0, endedAt: T0 + 240_000 },
    { text: "修 sync 测试", state: "current", startedAt: T0 + 240_000, estimateMs: 600_000 },
    { text: "开 PR", state: "pending", estimateMs: 120_000 },
  ] }, "steps with when they started and ended, and an estimate");
  bad("http/set-run-plan-request-negative-estimate.json", "SetRunPlanRequest", { plan: [{ text: "读代码", state: "current", estimateMs: -1 }] }, "estimateMs is not negative");
}
