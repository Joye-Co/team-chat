// V1 extension fixtures. Called by gen-fixtures.ts with its helpers and sample data.
import { MAX_ATTACHMENTS, MAX_CARD_ACTIONS, MAX_FILE_BYTES, MAX_SEARCH_LIMIT, MAX_UPLOAD_BYTES, MAX_WAITING_ON } from "../src/index.ts";

type Fixture = { kind: string; valid: boolean; description?: string; value: unknown; normalized?: unknown };
type Obj = Record<string, unknown>;
export type Ctx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  add: (path: string, f: Fixture) => void;
  T0: number;
  ID: Record<string, string>;
  topic: Obj;
  message: Obj;
  generalSummary: Obj;
  dmSummary: Obj;
  ownerFull: Obj;
  aliceMinimal: Obj;
  base: Obj;
};

const SHA_A = "a".repeat(64);
const SHA_B = "0123456789abcdef".repeat(4);

export function addV1Fixtures({ ok, bad, add, T0, ID, topic, message, generalSummary, dmSummary, ownerFull, aliceMinimal, base }: Ctx) {
  const X = {
    file: "01JBX0FILE00000000000000AH",
    file2: "01JBX0FILE00000000000000AJ",
    file3: "01JBX0FILE00000000000000AK",
    art: "01JBX0ART000000000000000AM",
    hook: "01JBX0HOOK00000000000000AN",
    hookBot: "01JBX0HOOKBOT00000000000AP",
    audit: "01JBX0AUDIT0000000000000AQ",
    topic2: "01JBX0TOPIC0000000000000AR",
    diag: "01JBX0DIAG00000000000000AS",
    token: "01JBX0TOKEN0000000000000AT",
  };

  // -------------------------------------------------------------------------
  // 1. Topics: waitingOn, ownerId, lastMessageAt, follow
  // -------------------------------------------------------------------------
  const topicV1 = { ...topic, status: "waiting_review", waitingOn: [ID.alice], ownerId: ID.bot, lastMessageAt: T0 + 9000 };
  ok("entities/topic-v1.json", "Topic", topicV1, "waitingOn, ownerId and lastMessageAt set");
  ok("entities/topic-v1-owner-null.json", "Topic", { ...topic, waitingOn: [], ownerId: null }, "empty waitingOn, owner explicitly cleared");
  bad("entities/topic-waiting-on-not-array.json", "Topic", { ...topic, waitingOn: ID.alice }, "wrong type for waitingOn");
  ok("events/topic.updated-v1.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, status: "waiting_answer", waitingOn: [ID.owner, ID.alice], ownerId: ID.bot } }, "status, waitingOn and owner changed together");
  ok("events/topic.updated-owner-cleared.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, ownerId: null, waitingOn: [] } }, "owner cleared, nobody waited on");
  ok("events/topic.updated-last-message-at.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, lastMessageAt: T0 + 4000 } }, "recomputed after the newest message was deleted");
  ok("client/topic.update-v1.json", "ClientFrame", { t: "topic.update", rid: "r20", groupId: ID.general, topicId: ID.topic, status: "waiting_review", waitingOn: [ID.owner], ownerId: ID.bot });
  ok("client/topic.update-owner-null.json", "ClientFrame", { t: "topic.update", rid: "r21", groupId: ID.general, topicId: ID.topic, ownerId: null, waitingOn: [] }, "clear owner and waitingOn");
  bad("client/topic.update-waiting-on-too-many.json", "ClientFrame", { t: "topic.update", rid: "r22", groupId: ID.general, topicId: ID.topic, waitingOn: Array.from({ length: MAX_WAITING_ON + 1 }, (_, i) => `m${i}`) }, "limit: waitingOn 21 members");
  ok("client/topic.follow.json", "ClientFrame", { t: "topic.follow", rid: "r23", groupId: ID.general, topicId: ID.topic, follow: true });
  ok("client/topic.follow-unfollow.json", "ClientFrame", { t: "topic.follow", rid: "r24", groupId: ID.general, topicId: ID.topic, follow: false });
  bad("client/topic.follow-missing-follow.json", "ClientFrame", { t: "topic.follow", rid: "r25", groupId: ID.general, topicId: ID.topic }, "missing follow");

  // -------------------------------------------------------------------------
  // 2. Read positions per topic
  // -------------------------------------------------------------------------
  const topicState = { topicId: ID.topic, readSeq: 40, unread: 2, mentions: 1, following: true };
  ok("entities/topic-state.json", "TopicState", topicState);
  ok("entities/topic-state-minimal.json", "TopicState", { topicId: X.topic2, readSeq: 12, unread: 0, mentions: 0 }, "following omitted");
  bad("entities/topic-state-missing-unread.json", "TopicState", { topicId: ID.topic, readSeq: 1, mentions: 0 }, "missing unread");
  const summaryV1 = { ...generalSummary, topics: [topicState, { topicId: X.topic2, readSeq: 30, unread: 0, mentions: 0, following: true }], muted: false, notify: "mentions" };
  ok("entities/group-summary-v1.json", "GroupSummary", summaryV1, "per-topic state and prefs");
  ok("entities/group-summary-muted.json", "GroupSummary", { ...dmSummary, topics: [], muted: true, notify: "none" });
  bad("entities/group-summary-bad-notify.json", "GroupSummary", { ...generalSummary, notify: "some" }, "unknown notify level");
  ok("client/read-topic.json", "ClientFrame", { t: "read", groupId: ID.general, seq: 42, topicId: ID.topic }, "move only this topic's read position");
  bad("client/read-topic-id-null.json", "ClientFrame", { t: "read", groupId: ID.general, seq: 42, topicId: null }, "null where only omission is allowed");
  ok("server/reads.json", "ServerFrame", { t: "reads", groupId: ID.general, readSeq: 40, unread: 1, mentions: 0, topics: [{ ...topicState, readSeq: 42, unread: 0, mentions: 0 }] }, "another device read one topic");
  ok("server/reads-group.json", "ServerFrame", { t: "reads", groupId: ID.general, readSeq: 42, unread: 0, mentions: 0, topics: [] }, "whole group read");
  bad("server/reads-missing-topics.json", "ServerFrame", { t: "reads", groupId: ID.general, readSeq: 40, unread: 0, mentions: 0 }, "missing topics");
  ok("server/welcome-v1.json", "ServerFrame", { t: "welcome", protocol: 1, me: ownerFull, members: [ownerFull, aliceMinimal], groups: [summaryV1, { ...dmSummary, notify: "all" }], serverTime: T0 + 60_000 }, "group summaries with topic state and prefs");
  ok("server/ack-ok-topic-follow.json", "ServerFrame", { t: "ack", rid: "r23", ok: true, result: topicState }, "ack of topic.follow: result is a TopicState");
  ok("client/topic.mute.json", "ClientFrame", { t: "topic.mute", rid: "r26", groupId: ID.general, topicId: ID.topic, muted: true });
  ok("client/topic.mute-unmute.json", "ClientFrame", { t: "topic.mute", rid: "r27", groupId: ID.general, topicId: ID.topic, muted: false });
  bad("client/topic.mute-missing-muted.json", "ClientFrame", { t: "topic.mute", rid: "r28", groupId: ID.general, topicId: ID.topic }, "missing muted");
  ok("entities/topic-state-muted.json", "TopicState", { topicId: X.topic2, readSeq: 12, unread: 0, mentions: 0, muted: true }, "muted, not followed");
  ok("server/ack-ok-topic-mute.json", "ServerFrame", { t: "ack", rid: "r26", ok: true, result: { ...topicState, muted: true } }, "ack of topic.mute: result is a TopicState");

  // -------------------------------------------------------------------------
  // 4. Cards (before messages, which embed them)
  // -------------------------------------------------------------------------
  const cardMinimal = { v: 1, title: "构建", blocks: [] };
  const cardFull = {
    v: 1,
    title: "PR #128 已合并 ✅",
    tone: "success",
    status: { label: "已合并" },
    blocks: [
      { type: "text", text: "**修复**登录重连的问题\n\n- 加了测试" },
      { type: "fields", fields: [{ label: "作者", value: "Alice" }, { label: "分支", value: "fix/reconnect" }] },
      { type: "table", columns: ["检查", "结果"], rows: [["lint", "通过"], ["test", "通过"]] },
      { type: "list", items: [{ text: "写测试", status: "done" }, { text: "发布", status: "in_progress" }, { text: "通知" }] },
      { type: "progress", value: 0.75, label: "3 / 4" },
      { type: "progress", value: 1 },
      { type: "code", lang: "rust", text: "fn main() {}\n" },
      { type: "code", text: "plain" },
      { type: "image", fileId: X.file, alt: "截图" },
      { type: "divider" },
      { type: "link", label: "在 GitHub 查看", target: { kind: "url", url: "https://github.com/Joye-Co/team-chat/pull/128" } },
      { type: "link", label: "相关话题", target: { kind: "topic", groupId: ID.general, topicId: X.topic2 } },
      { type: "link", label: "发布说明", target: { kind: "artifact", artifactId: X.art, version: 2 } },
      { type: "link", label: "最新发布说明", target: { kind: "artifact", artifactId: X.art } },
    ],
    actions: [
      { id: "approve", label: "批准", style: "primary", value: { pr: 128, ok: true } },
      { id: "reject", label: "拒绝", style: "danger", confirm: "确定拒绝吗？", value: "reject" },
      { id: "later", label: "稍后" },
    ],
  };
  ok("entities/card-full.json", "Card", cardFull, "every block type, tone, status and actions");
  ok("entities/card-minimal.json", "Card", cardMinimal, "no blocks, no actions");
  ok("entities/card-tones.json", "Card", { ...cardMinimal, tone: "danger", actions: [{ id: "retry.v1:a-b_c", label: "重试", style: "default" }] }, "action id with every allowed punctuation");
  bad("entities/card-v2.json", "Card", { ...cardMinimal, v: 2 }, "unsupported card version");
  bad("entities/card-bad-tone.json", "Card", { ...cardMinimal, tone: "pink" }, "unknown tone");
  bad("entities/card-unknown-block.json", "Card", { ...cardMinimal, blocks: [{ type: "chart", data: [] }] }, "unknown block type is rejected on write");
  bad("entities/card-table-row-mismatch.json", "Card", { ...cardMinimal, blocks: [{ type: "table", columns: ["a", "b"], rows: [["1"]] }] }, "limit: row has fewer cells than columns");
  bad("entities/card-table-too-many-rows.json", "Card", { ...cardMinimal, blocks: [{ type: "table", columns: ["a"], rows: Array.from({ length: 201 }, (_, i) => [`${i}`]) }] }, "limit: 201 table rows");
  bad("entities/card-progress-over-one.json", "Card", { ...cardMinimal, blocks: [{ type: "progress", value: 1.5 }] }, "limit: progress above 1");
  bad("entities/card-link-javascript.json", "Card", { ...cardMinimal, blocks: [{ type: "link", label: "x", target: { kind: "url", url: "javascript:alert(1)" } }] }, "only http and https links");
  bad("entities/card-duplicate-action-ids.json", "Card", { ...cardMinimal, actions: [{ id: "a", label: "A" }, { id: "a", label: "B" }] }, "action ids are unique");
  bad("entities/card-too-many-actions.json", "Card", { ...cardMinimal, actions: Array.from({ length: MAX_CARD_ACTIONS + 1 }, (_, i) => ({ id: `a${i}`, label: "x" })) }, "limit: 11 actions");
  bad("entities/card-action-bad-id.json", "Card", { ...cardMinimal, actions: [{ id: "a b", label: "x" }] }, "action id with a space");
  bad("entities/card-action-value-too-big.json", "Card", { ...cardMinimal, actions: [{ id: "a", label: "x", value: "v".repeat(4096) }] }, "limit: action value over 4 KiB");
  bad("entities/card-too-big.json", "Card", { ...cardMinimal, blocks: Array.from({ length: 9 }, () => ({ type: "text", text: "长".repeat(3000) })) }, "limit: card over 64 KiB of JSON");
  bad("entities/card-missing-blocks.json", "Card", { v: 1, title: "x" }, "missing blocks");
  ok("client/card.action.json", "ClientFrame", { t: "card.action", rid: "r30", groupId: ID.general, messageId: ID.msg2, actionId: "approve", value: { pr: 128, ok: true } });
  ok("client/card.action-minimal.json", "ClientFrame", { t: "card.action", rid: "r31", groupId: ID.general, messageId: ID.msg2, actionId: "later" }, "value omitted");
  bad("client/card.action-missing-action-id.json", "ClientFrame", { t: "card.action", rid: "r32", groupId: ID.general, messageId: ID.msg2 }, "missing actionId");
  bad("client/card.action-bad-action-id.json", "ClientFrame", { t: "card.action", rid: "r33", groupId: ID.general, messageId: ID.msg2, actionId: "" }, "empty actionId");
  ok("events/card.actioned.json", "Event", { ...base, actorId: ID.alice, type: "card.actioned", data: { messageId: ID.msg2, actionId: "approve", value: { pr: 128, ok: true } } });
  ok("events/card.actioned-no-value.json", "Event", { ...base, actorId: ID.alice, type: "card.actioned", data: { messageId: ID.msg2, actionId: "later" } }, "action without a value");
  bad("events/card.actioned-missing-message-id.json", "Event", { ...base, type: "card.actioned", data: { actionId: "a" } }, "missing messageId");

  // -------------------------------------------------------------------------
  // 3. Messages: quotes, attachments, 产出 references, cards, kinds
  // -------------------------------------------------------------------------
  const fileRefImage = { fileId: X.file, name: "截图 2026-11-03.png", size: 482_113, mime: "image/png", width: 1920, height: 1080, sha256: SHA_A };
  const fileRefPdf = { fileId: X.file2, name: "spec.pdf", size: 0, mime: "application/pdf", sha256: SHA_B };
  const replyTo = { messageId: ID.msg, authorId: ID.owner, excerpt: "大家好 👋" };
  const artifactRef = { artifactId: X.art, version: 2 };
  ok("entities/file-ref.json", "FileRef", fileRefImage, "image with pixel size");
  ok("entities/file-ref-minimal.json", "FileRef", fileRefPdf, "no pixel size, empty file");
  bad("entities/file-ref-missing-sha.json", "FileRef", { fileId: X.file, name: "a", size: 1, mime: "text/plain" }, "missing sha256");
  const messageV1 = { ...message, id: ID.msg2, kind: "text", replyTo, attachments: [fileRefImage, fileRefPdf], artifactRefs: [artifactRef] };
  ok("entities/message-v1.json", "Message", messageV1, "quote, attachments and 产出 reference");
  const messageCard = { ...message, id: ID.msg2, authorId: ID.bot, body: "PR #128 已合并", mentions: [], kind: "card", card: cardFull };
  ok("entities/message-card.json", "Message", messageCard, "card message with a plain-text body");
  ok("entities/message-system.json", "Message", { ...message, body: "Alice 加入了群", mentions: [], kind: "system" }, "system message");
  bad("entities/message-bad-kind.json", "Message", { ...message, kind: "voice" }, "unknown message kind");
  bad("entities/message-attachments-not-array.json", "Message", { ...message, attachments: fileRefImage }, "wrong type for attachments");
  ok("client/send-v1.json", "ClientFrame", { t: "send", rid: "r40", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "见附件", replyTo: { messageId: ID.msg }, attachments: [fileRefImage], artifactRefs: [artifactRef] });
  ok("client/send-card.json", "ClientFrame", { t: "send", rid: "r41", groupId: ID.general, clientMsgId: ID.cmid, title: "CI", body: "CI 通过", card: cardFull }, "a card message that starts a topic");
  add("client/send-reply-extra-fields.json", {
    kind: "ClientFrame",
    valid: true,
    description: "the client sends only replyTo.messageId; other replyTo fields are dropped",
    value: { t: "send", rid: "r42", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "同意", replyTo: { ...replyTo } },
    normalized: { t: "send", rid: "r42", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "同意", replyTo: { messageId: ID.msg } },
  });
  bad("client/send-too-many-attachments.json", "ClientFrame", { t: "send", rid: "r43", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x", attachments: Array.from({ length: MAX_ATTACHMENTS + 1 }, () => fileRefPdf) }, "limit: 21 attachments");
  bad("client/send-attachment-bad-sha.json", "ClientFrame", { t: "send", rid: "r44", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x", attachments: [{ ...fileRefPdf, sha256: "ABC" }] }, "sha256 must be 64 lowercase hex digits");
  bad("client/send-artifact-ref-version-zero.json", "ClientFrame", { t: "send", rid: "r45", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x", artifactRefs: [{ artifactId: X.art, version: 0 }] }, "artifact version starts at 1");
  bad("client/send-card-invalid.json", "ClientFrame", { t: "send", rid: "r46", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x", card: { ...cardMinimal, blocks: [{ type: "progress", value: -0.1 }] } }, "invalid card inside send");
  ok("client/edit-card.json", "ClientFrame", { t: "edit", rid: "r47", groupId: ID.general, messageId: ID.msg2, body: "CI 失败", card: { ...cardFull, tone: "danger", actions: [] }, attachments: [], artifactRefs: [artifactRef] }, "card author replaces the card and clears attachments");
  bad("client/edit-attachment-bad-mime.json", "ClientFrame", { t: "edit", rid: "r48", groupId: ID.general, messageId: ID.msg2, body: "x", attachments: [{ ...fileRefPdf, mime: "pdf" }] }, "mime must be type/subtype");
  ok("events/message.updated-card.json", "Event", { ...base, actorId: ID.bot, type: "message.updated", data: { messageId: ID.msg2, body: "CI 失败", editedAt: T0 + 9000, streaming: false, card: { ...cardMinimal, tone: "danger" }, attachments: [], artifactRefs: [artifactRef] } }, "card and references replaced");
  ok("events/message.created-v1.json", "Event", { ...base, type: "message.created", data: { message: messageV1 } });
  ok("http/send-result-v1.json", "SendResult", { message: messageCard, topic: topicV1 });

  // -------------------------------------------------------------------------
  // 5. Files and 产出
  // -------------------------------------------------------------------------
  const fileMeta = { id: X.file, groupId: ID.general, topicId: ID.topic, name: fileRefImage.name, size: fileRefImage.size, mime: "image/png", sha256: SHA_A, width: 1920, height: 1080, status: "ready", createdBy: ID.owner, createdAt: T0 };
  const fileMetaPending = { id: X.file2, groupId: ID.general, topicId: null, name: "spec.pdf", size: MAX_FILE_BYTES, mime: "application/pdf", sha256: SHA_B, status: "pending", createdBy: ID.alice, createdAt: T0 };
  ok("entities/file-meta.json", "FileMeta", fileMeta);
  ok("entities/file-meta-pending.json", "FileMeta", fileMetaPending, "not uploaded yet, no topic, size at the 100 MB limit");
  bad("entities/file-meta-bad-status.json", "FileMeta", { ...fileMeta, status: "deleted" }, "unknown file status");
  ok("http/create-file-request.json", "CreateFileRequest", { groupId: ID.general, topicId: ID.topic, name: fileRefImage.name, size: fileRefImage.size, mime: "image/png", sha256: SHA_A, width: 1920, height: 1080 });
  ok("http/create-file-request-minimal.json", "CreateFileRequest", { groupId: ID.general, name: "notes.txt", size: 12, mime: "text/plain", sha256: SHA_B });
  bad("http/create-file-request-too-big.json", "CreateFileRequest", { groupId: ID.general, name: "big.bin", size: MAX_UPLOAD_BYTES + 1, mime: "application/octet-stream", sha256: SHA_B }, "limit: size over 2 GB (1.1; was 100 MB)");
  bad("http/create-file-request-bad-sha.json", "CreateFileRequest", { groupId: ID.general, name: "a", size: 1, mime: "text/plain", sha256: "A".repeat(64) }, "uppercase hex digest");
  bad("http/create-file-request-bad-mime.json", "CreateFileRequest", { groupId: ID.general, name: "a", size: 1, mime: "text", sha256: SHA_B }, "mime without subtype");
  bad("http/create-file-request-empty-name.json", "CreateFileRequest", { groupId: ID.general, name: "", size: 1, mime: "text/plain", sha256: SHA_B }, "empty name");
  ok("http/create-file-response.json", "CreateFileResponse", { file: fileMetaPending, uploadUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${X.file2}/content` });
  ok("http/file-response.json", "FileResponse", fileMeta);

  const artifact = { id: X.art, groupId: ID.general, topicId: ID.topic, title: "发布说明", kind: "html", currentVersion: 2, createdBy: ID.bot, createdAt: T0, updatedAt: T0 + 60_000 };
  const files = [
    { path: "index.html", fileId: X.file, size: 2048, mime: "text/html" },
    { path: "assets/app.css", fileId: X.file2, size: 512, mime: "text/css" },
    { path: "assets/图.png", fileId: X.file3, size: 9000, mime: "image/png" },
  ];
  const version2 = { artifactId: X.art, version: 2, entry: "index.html", files, note: "修正错别字", createdBy: ID.bot, createdAt: T0 + 60_000 };
  const version1 = { artifactId: X.art, version: 1, entry: "index.html", files: [files[0]], createdBy: ID.bot, createdAt: T0 };
  ok("entities/artifact.json", "Artifact", artifact);
  ok("entities/artifact-file-kind.json", "Artifact", { ...artifact, kind: "file", currentVersion: 1 });
  bad("entities/artifact-bad-kind.json", "Artifact", { ...artifact, kind: "pdf" }, "unknown artifact kind");
  ok("entities/artifact-version.json", "ArtifactVersion", version2);
  ok("entities/artifact-version-minimal.json", "ArtifactVersion", version1, "note omitted");
  const inputFiles = files.map(({ path, fileId }) => ({ path, fileId }));
  ok("http/create-artifact-request.json", "CreateArtifactRequest", { groupId: ID.general, topicId: ID.topic, title: "发布说明", kind: "html", entry: "index.html", files: inputFiles, note: "初稿" });
  ok("http/create-artifact-request-file.json", "CreateArtifactRequest", { groupId: ID.general, topicId: ID.topic, title: "数据.xlsx", kind: "file", entry: "数据.xlsx", files: [{ path: "数据.xlsx", fileId: X.file }] }, "a file 产出");
  for (const [name, path] of [["dotdot", "../etc/passwd"], ["absolute", "/index.html"], ["backslash", "a\\b.html"], ["empty-segment", "a//b.html"], ["dot", "./index.html"], ["trailing-slash", "assets/"]]) {
    bad(`http/create-artifact-request-path-${name}.json`, "CreateArtifactRequest", { groupId: ID.general, topicId: ID.topic, title: "x", kind: "html", entry: "index.html", files: [{ path, fileId: X.file }] }, `unsafe path ${path}`);
  }
  bad("http/create-artifact-request-no-files.json", "CreateArtifactRequest", { groupId: ID.general, topicId: ID.topic, title: "x", kind: "html", entry: "index.html", files: [] }, "at least one file");
  ok("http/add-artifact-version-request.json", "AddArtifactVersionRequest", { entry: "index.html", files: inputFiles, note: "修正错别字", title: "发布说明 v2" });
  ok("http/add-artifact-version-request-minimal.json", "AddArtifactVersionRequest", { entry: "index.html", files: [inputFiles[0]] });
  bad("http/add-artifact-version-request-bad-entry.json", "AddArtifactVersionRequest", { entry: "../index.html", files: [inputFiles[0]] }, "unsafe entry path");
  ok("http/artifact-version-response.json", "ArtifactVersionResponse", { artifact, version: version2 });
  ok("http/artifact-response.json", "ArtifactResponse", { artifact, versions: [version2, version1] }, "versions newest first");
  ok("http/artifacts-response.json", "ArtifactsResponse", [artifact]);
  ok("events/artifact.created.json", "Event", { ...base, actorId: ID.bot, type: "artifact.created", data: { artifact: { ...artifact, currentVersion: 1, updatedAt: T0 }, version: version1 } });
  ok("events/artifact.versionAdded.json", "Event", { ...base, actorId: ID.bot, type: "artifact.versionAdded", data: { artifact, version: version2 } });
  bad("events/artifact.created-missing-version.json", "Event", { ...base, type: "artifact.created", data: { artifact } }, "missing version");

  // -------------------------------------------------------------------------
  // 6. Members, preferences, notifications
  // -------------------------------------------------------------------------
  ok("http/update-member-request.json", "UpdateMemberRequest", { name: "小王", role: "前端工程师", level: "owner", deactivated: false }, "owner changes every field");
  ok("http/update-member-request-clear-role.json", "UpdateMemberRequest", { role: null }, "clear the role");
  ok("http/update-member-request-empty.json", "UpdateMemberRequest", {}, "no change");
  ok("http/update-member-request-character.json", "UpdateMemberRequest", { character: "o05", role: "幕僚长" }, "owner sets character and role");
  ok("http/update-member-request-clear-character.json", "UpdateMemberRequest", { character: null }, "clear the character");
  bad("http/update-member-request-bad-character.json", "UpdateMemberRequest", { character: "o99" }, "unknown character id");
  bad("http/update-member-request-bad-level.json", "UpdateMemberRequest", { level: "admin" }, "unknown level");
  bad("http/update-member-request-name-too-long.json", "UpdateMemberRequest", { name: "名".repeat(65) }, "limit: name 65 characters");
  bad("http/update-member-request-empty-name.json", "UpdateMemberRequest", { name: "" }, "limit: empty name");
  ok("http/member-response.json", "MemberResponse", { ...ownerFull, avatarUrl: `https://chat-api-e2e.example.workers.dev/v1/members/${ID.owner}/avatar?v=3` });
  ok("http/update-group-prefs-request.json", "UpdateGroupPrefsRequest", { muted: true, notify: "none" });
  ok("http/update-group-prefs-request-notify.json", "UpdateGroupPrefsRequest", { notify: "all" });
  ok("http/update-group-request.json", "UpdateGroupRequest", { name: "研发 群 🚀" });
  add("http/update-group-request-extra.json", { kind: "UpdateGroupRequest", valid: true, description: "unknown fields are dropped", value: { name: "全员", memberIds: [ID.alice] }, normalized: { name: "全员" } });
  bad("http/update-group-request-empty-name.json", "UpdateGroupRequest", { name: "" }, "limit: empty name");
  bad("http/update-group-request-name-too-long.json", "UpdateGroupRequest", { name: "群".repeat(65) }, "limit: 65 characters");
  bad("http/update-group-request-missing-name.json", "UpdateGroupRequest", {}, "missing name (1.1: and purpose)");
  bad("http/update-group-prefs-request-bad-notify.json", "UpdateGroupPrefsRequest", { notify: "loud" }, "unknown notify level");
  for (const reason of ["mention", "waiting", "result", "message"]) {
    ok(`server/event-notify-${reason}.json`, "ServerFrame", { t: "event", event: { ...base, type: "message.created", data: { message } }, notify: reason });
  }
  bad("server/event-notify-bad-reason.json", "ServerFrame", { t: "event", event: { ...base, type: "message.created", data: { message } }, notify: "always" }, "unknown notify reason");

  // -------------------------------------------------------------------------
  // 7. Search
  // -------------------------------------------------------------------------
  ok("client/search.json", "ClientFrame", { t: "search", rid: "r50", query: "发布", groupId: ID.general, topicId: ID.topic, limit: MAX_SEARCH_LIMIT }, "inside one topic, limit at 50");
  ok("client/search-minimal.json", "ClientFrame", { t: "search", rid: "r51", query: "重连" }, "across all my groups");
  bad("client/search-empty-query.json", "ClientFrame", { t: "search", rid: "r52", query: "" }, "empty query");
  bad("client/search-limit-too-big.json", "ClientFrame", { t: "search", rid: "r53", query: "x", limit: MAX_SEARCH_LIMIT + 1 }, "limit: 51 hits");
  bad("client/search-query-too-long.json", "ClientFrame", { t: "search", rid: "r54", query: "查".repeat(201) }, "limit: query 201 characters");
  ok("server/searchResults.json", "ServerFrame", { t: "searchResults", rid: "r50", hits: [
    { groupId: ID.general, topicId: ID.topic, messageId: ID.msg, authorId: ID.owner, snippet: "…准备发布 V1…", highlights: [{ start: 3, length: 2 }], createdAt: T0 + 5000, seq: 41 },
    { groupId: ID.dm, topicId: X.topic2, messageId: ID.msg2, authorId: ID.alice, snippet: "发布", createdAt: T0, seq: 3 },
  ] });
  ok("server/searchResults-empty.json", "ServerFrame", { t: "searchResults", rid: "r51", hits: [] });
  bad("server/searchResults-missing-hits.json", "ServerFrame", { t: "searchResults", rid: "r51" }, "missing hits");

  // -------------------------------------------------------------------------
  // 8-9. Updates, diagnostics
  // -------------------------------------------------------------------------
  const update = { version: "1.2.10", notes: "## 新功能\n\n- 卡片", size: 48_000_000, sha256: SHA_B, publishedAt: T0, downloadUrl: "https://chat-api-e2e.example.workers.dev/v1/updates/1.2.10/download" };
  ok("http/update-info.json", "UpdateInfo", update);
  ok("http/update-info-windows.json", "UpdateInfo", { ...update, platform: "windows", downloadUrl: `${update.downloadUrl}?platform=windows` }, "the Windows installer");
  ok("http/update-info-macos.json", "UpdateInfo", { ...update, platform: "macos" }, "a server that knows platforms names macos too");
  bad("http/update-info-bad-version.json", "UpdateInfo", { ...update, version: "1.2" }, "version must be MAJOR.MINOR.PATCH");
  bad("http/update-info-leading-zero.json", "UpdateInfo", { ...update, version: "1.02.3" }, "no leading zeros");
  ok("http/diagnostics-response.json", "DiagnosticsResponse", { id: X.diag });

  // -------------------------------------------------------------------------
  // 10. Integrations
  // -------------------------------------------------------------------------
  const hook = { id: X.hook, name: "GitHub", kind: "github", groupId: ID.general, topicId: null, memberId: X.hookBot, createdBy: ID.owner, createdAt: T0, lastUsedAt: T0 + 3_600_000 };
  const hookGeneric = { id: X.hook, name: "部署", kind: "generic", groupId: ID.general, topicId: ID.topic, memberId: X.hookBot, createdBy: ID.owner, createdAt: T0 };
  ok("entities/hook.json", "Hook", hook);
  ok("entities/hook-generic.json", "Hook", hookGeneric, "default topic, never used");
  ok("http/create-hook-request.json", "CreateHookRequest", { name: "部署", groupId: ID.general, topicId: ID.topic, kind: "generic" });
  ok("http/create-hook-request-github.json", "CreateHookRequest", { name: "GitHub", groupId: ID.general, kind: "github" });
  bad("http/create-hook-request-bad-kind.json", "CreateHookRequest", { name: "x", groupId: ID.general, kind: "gitlab" }, "unknown hook kind");
  ok("http/create-hook-response.json", "CreateHookResponse", { hook, url: `https://chat-api-e2e.example.workers.dev/v1/hooks/${X.hook}/github`, secret: "whk_DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD" }, "secret is a fake placeholder");
  ok("http/hooks-response.json", "HooksResponse", [hook, hookGeneric]);
  ok("http/hook-in-request.json", "HookInRequest", { topicKey: "deploy:prod#42", title: "部署 prod #42", body: "开始部署", clientMsgId: "deploy-42-start" });
  ok("http/hook-in-request-card.json", "HookInRequest", { topicId: ID.topic, card: cardMinimal }, "card only, into a topic");
  bad("http/hook-in-request-both-topics.json", "HookInRequest", { topicId: ID.topic, topicKey: "k", body: "x" }, "topicId and topicKey exclude each other");
  bad("http/hook-in-request-empty.json", "HookInRequest", { topicKey: "k" }, "body or card is required");
  bad("http/hook-in-request-empty-body.json", "HookInRequest", { body: "" }, "an empty body without a card");
  bad("http/hook-in-request-bad-key.json", "HookInRequest", { topicKey: "a key", body: "x" }, "topic key with a space");
  ok("http/post-message-request.json", "PostMessageRequest", { topicId: ID.topic, clientMsgId: ID.cmid, body: "构建完成", mentions: [ID.alice], replyTo: { messageId: ID.msg }, attachments: [fileRefPdf], artifactRefs: [artifactRef] });
  ok("http/post-message-request-card.json", "PostMessageRequest", { title: "CI", clientMsgId: ID.cmidUuid, body: "CI 通过", card: cardFull }, "starts a topic");
  bad("http/post-message-request-missing-client-msg-id.json", "PostMessageRequest", { body: "x" }, "missing clientMsgId");
  bad("http/post-message-request-body-too-long.json", "PostMessageRequest", { clientMsgId: ID.cmid, body: "x".repeat(32_769) }, "limit: body 32769 characters");

  // -------------------------------------------------------------------------
  // 11. Audit
  // -------------------------------------------------------------------------
  const auditEntry = { id: X.audit, at: T0, actorId: ID.bot, requestedBy: ID.alice, credential: { kind: "token", id: X.token }, action: "member.update", target: { kind: "member", id: ID.alice }, details: { level: ["member", "owner"], note: null } };
  const auditAdmin = { id: X.audit, at: T0, actorId: null, credential: { kind: "admin" }, action: "release.publish", details: { version: "1.2.10" } };
  ok("entities/audit-entry.json", "AuditEntry", auditEntry);
  ok("entities/audit-entry-admin.json", "AuditEntry", auditAdmin, "admin secret: no actor");
  ok("entities/audit-entry-group.json", "AuditEntry", { id: X.audit, at: T0, actorId: ID.owner, requestedBy: null, credential: { kind: "hook", id: X.hook }, action: "message.create", groupId: ID.general }, "requestedBy explicitly null");
  bad("entities/audit-entry-bad-credential.json", "AuditEntry", { ...auditAdmin, credential: { kind: "password" } }, "unknown credential kind");
  ok("http/audit-response.json", "AuditResponse", { entries: [auditEntry, auditAdmin], hasMore: true });
  ok("http/audit-response-empty.json", "AuditResponse", { entries: [], hasMore: false });
}
