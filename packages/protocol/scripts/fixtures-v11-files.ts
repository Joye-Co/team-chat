// 1.1 fixtures of the server-files gaps (GAPS.md SF-*): media file limits, multipart uploads,
// stored page events, desk notes, aside origins, PR merge state, group purpose and 产出
// previews. Called by gen-fixtures.ts.
import type { InteractCtx } from "./fixtures-v11-interact.ts";

export function addFilesFixtures({ ok, bad, T0, ID, base, message }: InteractCtx) {
  const FILE = "01JC0FIL0000000000000INDEX";

  // SF-1: files over 100 MB upload in parts.
  const SHA = "b".repeat(64);
  const BIG = "01JC0FIL00000000000000BIG1";
  const bigMeta = { id: BIG, groupId: ID.general, topicId: null, name: "录屏.mov", size: 150_000_000, mime: "video/quicktime", sha256: SHA, status: "pending", createdBy: ID.alice, createdAt: T0 };
  ok("http/create-file-request-multipart.json", "CreateFileRequest", { groupId: ID.general, name: "录屏.mov", size: 150_000_000, mime: "video/quicktime", sha256: SHA }, "1.1: over 100 MB, up to 2 GB");
  ok("http/create-file-response-multipart.json", "CreateFileResponse", {
    file: bigMeta,
    uploadUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/content`,
    multipart: { partBytes: 52_428_800, parts: 3, partUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/parts`, completeUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/complete` },
  });
  ok("http/upload-part-response.json", "UploadPartResponse", { part: 2, size: 52_428_800 });

  // SF-2: the page log of a 产出.
  const ART = "01JC0ART00000000000000OCT3";
  ok("http/artifact-events-response.json", "ArtifactEventsResponse", {
    events: [
      { eventId: "01JC0EV0000000000000000002", artifactId: ART, version: 2, name: "pick", kind: "emit", payload: { channel: "bili" }, actorId: ID.owner, at: T0 + 60_000 },
      { eventId: "01JC0EV0000000000000000001", artifactId: ART, version: 1, name: "ask.total", kind: "ask", payload: { month: "2026-10" }, actorId: ID.alice, at: T0, replied: true },
      { eventId: "01JC0EV0000000000000000000", artifactId: ART, version: 1, name: "pick", kind: "emit", actorId: ID.alice, at: T0 - 1 },
    ],
    hasMore: true,
  }, "newest first; the last one is from before payloads were kept");
  ok("http/artifact-events-response-empty.json", "ArtifactEventsResponse", { events: [], hasMore: false });

  // SF-11: the author's preview card of a 产出 version.
  const preview = { title: "报名看板", summary: "这周报名 38 人，B 站最多。", metrics: [{ label: "报名", value: "38 人", delta: "+12" }, { label: "B 站", value: "9" }] };
  ok("entities/artifact-version-preview.json", "ArtifactVersion", {
    artifactId: ART, version: 3, entry: "index.html", files: [{ path: "index.html", fileId: FILE, size: 12_000, mime: "text/html" }], createdBy: ID.bot, createdAt: T0, preview,
  });
  const createHtml = { groupId: ID.general, topicId: ID.topic, title: "看板", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: FILE }], preview };
  ok("http/create-artifact-preview.json", "CreateArtifactRequest", createHtml);
  ok("http/add-artifact-version-preview.json", "AddArtifactVersionRequest", { entry: "index.html", files: [{ path: "index.html", fileId: FILE }], preview: { summary: "只有一句话" } });
  bad("http/create-artifact-preview-too-many-metrics.json", "CreateArtifactRequest", { ...createHtml, preview: { metrics: Array.from({ length: 7 }, (_, i) => ({ label: `m${i}`, value: "1" })) } }, "at most 6 figures");
  bad("http/create-artifact-preview-long-label.json", "CreateArtifactRequest", { ...createHtml, preview: { metrics: [{ label: "x".repeat(41), value: "1" }] } }, "a figure label is at most 40 characters");

  // SF-8: a group's purpose line.
  ok("entities/group-purpose.json", "Group", { id: ID.general, kind: "group", name: "开发", memberIds: [ID.owner, ID.alice, ID.bot], createdBy: ID.owner, createdAt: T0, purpose: "qqbot、夹夹和 TeamChat 的开发" });
  ok("http/update-group-request-purpose.json", "UpdateGroupRequest", { purpose: "qqbot、夹夹和 TeamChat 的开发" }, "1.1: purpose only");
  ok("http/update-group-request-clear-purpose.json", "UpdateGroupRequest", { name: "开发", purpose: "" }, "1.1: rename and clear the purpose");
  ok("events/group.updated-purpose.json", "Event", { ...base, seq: 70, type: "group.updated", data: { name: "开发", purpose: "qqbot、夹夹和 TeamChat 的开发" } }, "1.1: the purpose changed");
  ok("events/group.updated-purpose-cleared.json", "Event", { ...base, seq: 71, type: "group.updated", data: { name: "开发", purpose: "" } });

  // SF-4: a side chat answer posted to its source topic.
  const fromAside = { groupId: ID.dm, topicId: "01JC0SIDE00000000000000AS1", messageId: "01JC0MSG000000000000ANSWR1" };
  ok("client/send-from-aside.json", "ClientFrame", { t: "send", rid: "r200", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "建议：重连后按 seq 补发。", fromAside }, "1.1: post to topic from 单独问");
  bad("client/send-from-aside-missing-message.json", "ClientFrame", { t: "send", rid: "r201", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x", fromAside: { groupId: ID.dm, topicId: fromAside.topicId } }, "fromAside names the answer message");
  ok("http/post-message-from-aside.json", "PostMessageRequest", { topicId: ID.topic, clientMsgId: ID.cmid, body: "建议：重连后按 seq 补发。", fromAside });
  ok("entities/message-from-aside.json", "Message", { ...message, fromAside });

  // SF-5: the approve action of a review ask, and a PR's queued merge.
  const review = {
    v: 2,
    rev: 1,
    intent: "ask",
    title: "合并 PR #61？",
    fallback: "Forge 改完了重连补发，请审一下。",
    ask: { from: ["requester"] },
    blocks: [{ type: "text", id: "why", text: "补发 3 条，没有重复。" }],
    actions: [
      { id: "merge", label: "合并", style: "primary", role: "approve" },
      { id: "changes", label: "要改", kind: "reject" },
    ],
  };
  ok("entities/card-v2-review-approve-role.json", "CardV2", review, "1.1: role approve marks the action that merges");
  ok("entities/card-v2-action-unknown-role.json", "CardV2", { ...review, actions: [{ id: "merge", label: "合并", role: "ship-it-later" }] }, "roles are open text; unknown ones mean nothing");
  bad("entities/card-v2-action-role-too-long.json", "CardV2", { ...review, actions: [{ id: "merge", label: "合并", role: "x".repeat(33) }] }, "a role is at most 32 characters", "card.limits");
  const prLink = { kind: "pr", repo: "Joye-Co/team-chat", number: 61, url: "https://github.com/Joye-Co/team-chat/pull/61", title: "fix: resend after reconnect", state: "open", checks: [{ name: "CI", conclusion: "success" }] };
  ok("entities/topic-link-pr-merge-queued.json", "TopicLink", { ...prLink, merge: { queuedAt: T0 } });
  ok("entities/topic-link-pr-merge-failed.json", "TopicLink", { ...prLink, merge: { queuedAt: T0, error: "merge conflict" } });
  ok("events/topic.linked-merge-failed.json", "Event", { ...base, seq: 72, actorId: ID.bot, type: "topic.linked", data: { topicId: ID.topic, link: { ...prLink, merge: { queuedAt: T0, error: "checks failed" } } } });

  // SF-3: desk notes and queued runs (排队).
  ok("http/put-desk-note-request.json", "PutDeskNoteRequest", { groupId: ID.general, text: "今天先把 1.1 发出去" });
  ok("http/put-desk-note-request-clear.json", "PutDeskNoteRequest", { groupId: ID.general, text: "" }, "clears the note");
  ok("http/put-desk-note-response.json", "PutDeskNoteResponse", { seq: 73 });
  ok("http/put-desk-note-response-same.json", "PutDeskNoteResponse", { seq: null }, "same text, nothing stored");
  ok("http/desk-notes-response.json", "DeskNotesResponse", { notes: [{ memberId: ID.bot, text: "今天先把 1.1 发出去", at: T0, seq: 73 }] });
  ok("events/desk.noted.json", "Event", { ...base, seq: 73, actorId: ID.bot, type: "desk.noted", data: { text: "今天先把 1.1 发出去" } });
  ok("events/desk.noted-cleared.json", "Event", { ...base, seq: 74, actorId: ID.bot, type: "desk.noted", data: { text: "" } });
  ok("entities/bot-run-queued.json", "BotRun", {
    id: "01JC0RUN00000000000000QUE2", botId: ID.bot, groupId: ID.general, topicId: ID.topic, status: "created", requestedBy: ID.alice, createdAt: T0, updatedAt: T0, queuedAt: T0, queuePosition: 1,
  }, "1.1: created, no machine, waiting for a free one");

  // SF-10: how long a rate-limited client waits.
  ok("http/error-response-retry-after.json", "ErrorBody", { code: "rate_limited", message: "too many frames", retryAfterMs: 50 });
  ok("server/ack-rate-limited.json", "ServerFrame", { t: "ack", rid: "r300", ok: false, error: { code: "rate_limited", message: "too many frames", retryAfterMs: 50 } });

  // SF-7: a table is at most 200 MB.
  const table = { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, preview: "ready" };
  const createCsv = { groupId: ID.general, topicId: ID.topic, title: "报名表", kind: "file", entry: "a.csv", files: [{ path: "a.csv", fileId: FILE }], media: table };
  ok("http/create-artifact-media-table.json", "CreateArtifactRequest", createCsv);
  bad("http/create-artifact-media-table-too-big.json", "CreateArtifactRequest", { ...createCsv, media: { ...table, bytes: 200_000_001 } }, "a table is at most 200 MB");
  ok("entities/media-table.json", "MediaInfo", { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, posterFileId: FILE, preview: "ready" });
}
