// Writes protocol/fixtures/**. Run with `bun run gen:protocol` at the repo root (or
// `bun run gen:fixtures`); `--check` only compares. The fixture files are the contract; this
// script only makes them easier to extend. It owns the whole folder: every fixture is written
// here (or in a fixtures-*.ts it calls), and a file it does not produce is removed, so never add
// one by hand. test/generated.test.ts fails when the folder and this script differ.
import { join } from "node:path";
import { MAX_BODY_CHARS, MAX_NAME_CHARS, MAX_PAGE_LIMIT, MAX_TITLE_CHARS } from "../src/index.ts";
import { addV1Fixtures } from "./fixtures-v1.ts";
import { addBotsFixtures } from "./fixtures-bots.ts";
import { addReactionsFixtures } from "./fixtures-reactions.ts";
import { addV11Fixtures } from "./fixtures-v11.ts";
import { addInteractFixtures } from "./fixtures-v11-interact.ts";
import { addSettingsFixtures } from "./fixtures-settings.ts";
import { addGapFixtures } from "./fixtures-v11-gaps.ts";
import { addFilesFixtures } from "./fixtures-v11-files.ts";
import { addMemberFixtures } from "./fixtures-members.ts";
import { addMemoryFixtures } from "./fixtures-memory.ts";
import { addPushFixtures } from "./fixtures-push.ts";
import { addUsableFixtures } from "./fixtures-usable.ts";
import { addSignInFixtures } from "./fixtures-sign-in.ts";
import { addHomesFixtures } from "./fixtures-homes.ts";
import { addAiServiceFixtures } from "./fixtures-ai-service.ts";
import { addSuperviseFixtures } from "./fixtures-supervise.ts";
import { addAlertsFixtures } from "./fixtures-alerts.ts";
import { addPersonalAiFixtures } from "./fixtures-personal-ai.ts";
import { addHideDeleteFixtures } from "./fixtures-hide-delete.ts";
import { run, type Tree } from "./generated.ts";

type Fixture = {
  kind: string;
  valid: boolean;
  description?: string;
  /** Invalid fixtures: the `ack.error.reason` both sides give (`card.limits`, …). */
  reason?: string;
  value: unknown;
  /** Expected value after parse and serialize, when it differs from `value`. */
  normalized?: unknown;
};

export const fixturesRoot = join(import.meta.dir, "../../../protocol/fixtures");
const out = new Map<string, Fixture>();
const add = (path: string, f: Fixture) => {
  if (out.has(path)) throw new Error(`duplicate fixture ${path}`);
  out.set(path, f);
};
const ok = (path: string, kind: string, value: unknown, description?: string) =>
  add(path, { kind, valid: true, ...(description ? { description } : {}), value });
const bad = (path: string, kind: string, value: unknown, description: string, reason?: string) =>
  add(path, { kind, valid: false, description, ...(reason ? { reason } : {}), value });

// ---------------------------------------------------------------------------
// Sample data
// ---------------------------------------------------------------------------

const T0 = 1_762_128_000_000;
const ID = {
  owner: "01JBX0OWNER0000000000000AA",
  alice: "01JBX0ALICE0000000000000AB",
  bot: "01JBX0BOT000000000000000AC",
  general: "01JBX0GENERAL000000000000A",
  dm: "01JBX0DM00000000000000000B",
  topic: "01JBX0TOPIC0000000000000AD",
  msg: "01JBX0MSG000000000000000AE",
  msg2: "01JBX0MSG000000000000000AF",
  cmid: "01JBX0CMID00000000000000AG",
  cmidUuid: "3b241101-e2bb-4255-8caf-4136c566a962",
};

const ownerFull = {
  id: ID.owner,
  kind: "human",
  name: "Joye",
  role: "产品负责人",
  level: "owner",
  avatarUrl: "https://example.com/a/joye.png",
  deactivated: false,
};
const aliceMinimal = { id: ID.alice, kind: "human", name: "Alice", level: "member" };
const botNulls = {
  id: ID.bot,
  kind: "bot",
  name: "代码助手 🤖",
  role: null,
  level: "member",
  avatarUrl: null,
  deactivated: true,
};

const general = {
  id: ID.general,
  kind: "group",
  name: "General",
  memberIds: [ID.owner, ID.alice, ID.bot],
  createdBy: ID.owner,
  createdAt: T0,
};
const dm = {
  id: ID.dm,
  kind: "dm",
  name: "Alice",
  memberIds: [ID.owner, ID.alice],
  createdBy: ID.alice,
  createdAt: T0 + 1000,
};
const generalSummary = { group: general, lastSeq: 42, readSeq: 40, unread: 2, mentions: 1 };
const dmSummary = { group: dm, lastSeq: 0, readSeq: 0, unread: 0, mentions: 0 };

const topic = {
  id: ID.topic,
  groupId: ID.general,
  title: "发布 V1：检查清单 ✅",
  status: "in_progress",
  createdBy: ID.owner,
  createdAt: T0,
  rootMessageId: ID.msg,
  lastSeq: 42,
  messageCount: 7,
};

const message = {
  id: ID.msg,
  clientMsgId: ID.cmid,
  groupId: ID.general,
  topicId: ID.topic,
  authorId: ID.owner,
  body: "大家好 👋\n\n- [ ] 写测试\n- [x] **合并** `main`\n\n@Alice 请看一下",
  mentions: [ID.alice],
  createdSeq: 41,
  createdAt: T0 + 5000,
};
const messageFull = {
  ...message,
  id: ID.msg2,
  clientMsgId: ID.cmidUuid,
  authorId: ID.bot,
  body: "Working on it…",
  mentions: [],
  createdSeq: 42,
  editedAt: T0 + 9000,
  deletedAt: null,
  streaming: true,
};
const messageDeleted = {
  ...message,
  body: "",
  mentions: [],
  editedAt: null,
  deletedAt: T0 + 10_000,
};

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

ok("entities/member-full.json", "Member", ownerFull, "every field set, Chinese role");
ok("entities/member-minimal.json", "Member", aliceMinimal, "optional fields omitted");
ok("entities/member-bot-nulls.json", "Member", botNulls, "bot with explicit nulls and emoji name");
ok("entities/member-name-max.json", "Member", { ...aliceMinimal, name: "名".repeat(MAX_NAME_CHARS) }, "name at 64 characters");
ok("entities/member-bot-character.json", "Member", { ...botNulls, role: "主助手", level: "owner", character: "jojo" }, "bot with a character id");
ok("entities/member-character-null.json", "Member", { ...aliceMinimal, character: null }, "character null = no character");
bad("entities/member-bad-character.json", "Member", { ...aliceMinimal, character: "o21" }, "character is not one of the 26 known ids");
bad("entities/member-character-uppercase.json", "Member", { ...aliceMinimal, character: "JOJO" }, "character ids are lower case");
ok("entities/group.json", "Group", general);
ok("entities/group-dm.json", "Group", dm);
ok("entities/group-summary.json", "GroupSummary", generalSummary);
ok("entities/topic.json", "Topic", topic);
for (const status of ["open", "waiting_answer", "waiting_review", "blocked", "done"]) {
  ok(`entities/topic-status-${status}.json`, "Topic", { ...topic, status });
}
ok("entities/message.json", "Message", message, "optional fields omitted, Markdown with Chinese and emoji");
ok("entities/message-full.json", "Message", messageFull, "UUID clientMsgId, streaming, deletedAt null");
ok("entities/message-deleted.json", "Message", messageDeleted, "deleted message keeps its row, body is empty");
ok("entities/message-body-max.json", "Message", { ...message, body: "a".repeat(MAX_BODY_CHARS) }, "body at 32768 characters");
add("entities/member-unknown-field.json", {
  kind: "Member",
  valid: true,
  description: "unknown fields are ignored on read and dropped on write",
  value: { ...aliceMinimal, futureField: { nested: [1, 2, 3] }, theme: "dark" },
  normalized: aliceMinimal,
});
bad("entities/member-missing-level.json", "Member", { id: ID.alice, kind: "human", name: "Alice" }, "missing required field level");
bad("entities/member-bad-kind.json", "Member", { ...aliceMinimal, kind: "robot" }, "unknown enum value");
bad("entities/topic-bad-status.json", "Topic", { ...topic, status: "closed" }, "unknown topic status");
bad("entities/message-mentions-not-array.json", "Message", { ...message, mentions: ID.alice }, "wrong type for mentions");
bad("entities/message-seq-string.json", "Message", { ...message, createdSeq: "41" }, "number sent as string");
bad("entities/message-seq-float.json", "Message", { ...message, createdSeq: 41.5 }, "seq must be an integer");
bad("entities/group-null-name.json", "Group", { ...general, name: null }, "null where not allowed");

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

const base = { seq: 42, groupId: ID.general, at: T0 + 5000, actorId: ID.owner };
const events = {
  "topic.created": { ...base, type: "topic.created", data: { topic: { ...topic, status: "open", messageCount: 1 } } },
  "topic.updated": { ...base, type: "topic.updated", data: { topicId: ID.topic, title: "新标题", status: "done" } },
  "message.created": { ...base, type: "message.created", data: { message } },
  "message.updated": {
    ...base,
    actorId: ID.bot,
    type: "message.updated",
    data: { messageId: ID.msg2, body: "Done ✅", editedAt: T0 + 9000, streaming: false },
  },
  "message.deleted": { ...base, type: "message.deleted", data: { messageId: ID.msg, deletedAt: T0 + 10_000 } },
  "group.updated": { ...base, type: "group.updated", data: { name: "研发组 🚀" } },
  "group.memberAdded": { ...base, type: "group.memberAdded", data: { memberId: ID.bot } },
  "group.memberRemoved": { ...base, type: "group.memberRemoved", data: { memberId: ID.alice } },
};
for (const [type, value] of Object.entries(events)) ok(`events/${type}.json`, "Event", value);
ok("events/topic.updated-title-only.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, title: "t".repeat(MAX_TITLE_CHARS) } }, "status omitted, title at 200 characters");
ok("events/topic.updated-status-only.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, status: "blocked" } }, "title omitted");
ok("events/message.created-full.json", "Event", { ...base, seq: 1, type: "message.created", data: { message: messageFull } }, "first event in a log");
bad("events/unknown-type.json", "Event", { ...base, type: "message.pinned", data: { messageId: ID.msg } }, "unknown event type");
bad("events/missing-seq.json", "Event", { groupId: ID.general, at: T0, actorId: ID.owner, type: "group.updated", data: { name: "x" } }, "missing shared field seq");
bad("events/missing-data.json", "Event", { ...base, type: "message.deleted" }, "missing data");
bad("events/message.updated-missing-streaming.json", "Event", { ...base, type: "message.updated", data: { messageId: ID.msg, body: "x", editedAt: T0 } }, "missing required data field");

const transients = {
  typing: { type: "typing", groupId: ID.general, topicId: ID.topic, memberId: ID.alice, at: T0 },
  presence: { type: "presence", memberId: ID.bot, online: true, at: T0 },
  "message.delta": { type: "message.delta", groupId: ID.general, topicId: ID.topic, messageId: ID.msg2, append: "正在生成… 🧠" },
};
for (const [type, value] of Object.entries(transients)) ok(`transient/${type}.json`, "TransientEvent", value);
ok("transient/presence-offline.json", "TransientEvent", { ...transients.presence, online: false });
bad("transient/unknown-type.json", "TransientEvent", { type: "reaction", memberId: ID.bot, at: T0 }, "unknown transient type");
bad("transient/presence-online-string.json", "TransientEvent", { ...transients.presence, online: "yes" }, "wrong type for online");

// ---------------------------------------------------------------------------
// Client frames
// ---------------------------------------------------------------------------

const c = "client";
ok(`${c}/hello.json`, "ClientFrame", { t: "hello", protocol: 1, client: { name: "desktop", version: "0.1.0" }, cursors: { [ID.general]: 42, [ID.dm]: 0 } });
ok(`${c}/hello-no-cursors.json`, "ClientFrame", { t: "hello", protocol: 1, client: { name: "daemon", version: "0.1.0" } }, "cursors omitted");
ok(`${c}/send-new-topic.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, clientMsgId: ID.cmid, body: "新话题的第一条消息 🎉" }, "no topicId: starts a topic, title defaults");
ok(`${c}/send-new-topic-title.json`, "ClientFrame", { t: "send", rid: "r2", groupId: ID.general, title: "标".repeat(MAX_TITLE_CHARS), clientMsgId: ID.cmidUuid, body: "body", mentions: [ID.alice, ID.bot] }, "title at 200 characters");
ok(`${c}/send-reply-streaming.json`, "ClientFrame", { t: "send", rid: "r3", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "", mentions: [], streaming: true }, "bot starts a streaming reply with an empty body");
ok(`${c}/send-body-max.json`, "ClientFrame", { t: "send", rid: "r4", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x".repeat(MAX_BODY_CHARS) }, "body at 32768 characters");
ok(`${c}/edit.json`, "ClientFrame", { t: "edit", rid: "r5", groupId: ID.general, messageId: ID.msg2, body: "最终答案 ✅", streaming: false });
ok(`${c}/edit-minimal.json`, "ClientFrame", { t: "edit", rid: "r6", groupId: ID.general, messageId: ID.msg, body: "fixed typo" });
ok(`${c}/delta.json`, "ClientFrame", { t: "delta", groupId: ID.general, messageId: ID.msg2, append: "下一段 🧩" });
ok(`${c}/delete.json`, "ClientFrame", { t: "delete", rid: "r7", groupId: ID.general, messageId: ID.msg });
ok(`${c}/topic.update.json`, "ClientFrame", { t: "topic.update", rid: "r8", groupId: ID.general, topicId: ID.topic, title: "已解决", status: "done" });
ok(`${c}/topic.update-status-only.json`, "ClientFrame", { t: "topic.update", rid: "r9", groupId: ID.general, topicId: ID.topic, status: "waiting_review" });
ok(`${c}/typing.json`, "ClientFrame", { t: "typing", groupId: ID.general, topicId: ID.topic });
ok(`${c}/read.json`, "ClientFrame", { t: "read", groupId: ID.general, seq: 42 });
ok(`${c}/fetch.json`, "ClientFrame", { t: "fetch", rid: "r10", groupId: ID.general, afterSeq: 0, limit: MAX_PAGE_LIMIT }, "limit at 500");
ok(`${c}/fetch-no-limit.json`, "ClientFrame", { t: "fetch", rid: "r11", groupId: ID.general, afterSeq: 40 });
ok(`${c}/history.json`, "ClientFrame", { t: "history", rid: "r12", groupId: ID.general, topicId: ID.topic, beforeSeq: 42, limit: 50 });
ok(`${c}/history-minimal.json`, "ClientFrame", { t: "history", rid: "r13", groupId: ID.general, topicId: ID.topic });
ok(`${c}/ping.json`, "ClientFrame", { t: "ping" });
add(`${c}/send-unknown-field.json`, {
  kind: "ClientFrame",
  valid: true,
  description: "unknown fields are ignored on read and dropped on write",
  value: { t: "send", rid: "r14", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "hi", labels: [], priority: 3 },
  normalized: { t: "send", rid: "r14", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "hi" },
});
bad(`${c}/unknown-t.json`, "ClientFrame", { t: "subscribe", groupId: ID.general }, "unknown frame type");
bad(`${c}/missing-t.json`, "ClientFrame", { rid: "r1", groupId: ID.general }, "missing t");
bad(`${c}/send-missing-client-msg-id.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, body: "hi" }, "missing required field clientMsgId");
bad(`${c}/send-body-number.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, clientMsgId: ID.cmid, body: 42 }, "wrong type for body");
bad(`${c}/send-topic-id-null.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, topicId: null, clientMsgId: ID.cmid, body: "hi" }, "null where only omission is allowed");
bad(`${c}/read-seq-negative.json`, "ClientFrame", { t: "read", groupId: ID.general, seq: -1 }, "negative seq");
bad(`${c}/hello-missing-client.json`, "ClientFrame", { t: "hello", protocol: 1 }, "missing client");
bad(`${c}/topic.update-bad-status.json`, "ClientFrame", { t: "topic.update", rid: "r1", groupId: ID.general, topicId: ID.topic, status: "archived" }, "unknown topic status");
bad(`${c}/fetch-after-seq-string.json`, "ClientFrame", { t: "fetch", rid: "r1", groupId: ID.general, afterSeq: "0" }, "wrong type for afterSeq");
// Limit violations: Rust rejects these with chat_protocol::validate_client_frame.
bad(`${c}/send-body-too-long.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, topicId: ID.topic, clientMsgId: ID.cmid, body: "x".repeat(MAX_BODY_CHARS + 1) }, "limit: body 32769 characters");
bad(`${c}/send-title-too-long.json`, "ClientFrame", { t: "send", rid: "r1", groupId: ID.general, title: "t".repeat(MAX_TITLE_CHARS + 1), clientMsgId: ID.cmid, body: "hi" }, "limit: title 201 characters");
bad(`${c}/edit-body-too-long.json`, "ClientFrame", { t: "edit", rid: "r1", groupId: ID.general, messageId: ID.msg, body: "😀".repeat(MAX_BODY_CHARS) + "x" }, "limit: body 32769 code points of emoji");
bad(`${c}/topic.update-title-too-long.json`, "ClientFrame", { t: "topic.update", rid: "r1", groupId: ID.general, topicId: ID.topic, title: "t".repeat(MAX_TITLE_CHARS + 1) }, "limit: title 201 characters");
bad(`${c}/fetch-limit-too-big.json`, "ClientFrame", { t: "fetch", rid: "r1", groupId: ID.general, afterSeq: 0, limit: MAX_PAGE_LIMIT + 1 }, "limit: page limit 501");
bad(`${c}/history-limit-zero.json`, "ClientFrame", { t: "history", rid: "r1", groupId: ID.general, topicId: ID.topic, limit: 0 }, "limit: page limit 0");
ok(`${c}/edit-body-emoji-max.json`, "ClientFrame", { t: "edit", rid: "r15", groupId: ID.general, messageId: ID.msg, body: "😀".repeat(MAX_BODY_CHARS) }, "body at 32768 code points of emoji: lengths count Unicode code points, not UTF-16 units");

// ---------------------------------------------------------------------------
// Server frames
// ---------------------------------------------------------------------------

const s = "server";
ok(`${s}/welcome.json`, "ServerFrame", { t: "welcome", protocol: 1, me: ownerFull, members: [ownerFull, aliceMinimal, botNulls], groups: [generalSummary, dmSummary], serverTime: T0 + 60_000 });
ok(`${s}/welcome-empty.json`, "ServerFrame", { t: "welcome", protocol: 1, me: aliceMinimal, members: [aliceMinimal], groups: [], serverTime: T0 }, "empty lists");
ok(`${s}/event.json`, "ServerFrame", { t: "event", event: events["message.created"] });
ok(`${s}/event-member-added.json`, "ServerFrame", { t: "event", event: events["group.memberAdded"] });
ok(`${s}/transient-typing.json`, "ServerFrame", { t: "transient", event: transients.typing });
ok(`${s}/transient-delta.json`, "ServerFrame", { t: "transient", event: transients["message.delta"] });
ok(`${s}/ack-ok-send.json`, "ServerFrame", { t: "ack", rid: "r1", ok: true, result: { message, topic } }, "ack of a send that started a topic");
ok(`${s}/ack-ok-duplicate.json`, "ServerFrame", { t: "ack", rid: "r2", ok: true, result: { message, duplicate: true } });
ok(`${s}/ack-ok-no-result.json`, "ServerFrame", { t: "ack", rid: "r3", ok: true }, "result omitted");
ok(`${s}/ack-error.json`, "ServerFrame", { t: "ack", rid: "r4", ok: false, error: { code: "forbidden", message: "只有作者可以编辑" } });
for (const code of ["unauthorized", "not_found", "invalid", "conflict", "rate_limited", "too_large", "unsupported_protocol", "internal"]) {
  ok(`${s}/ack-error-${code}.json`, "ServerFrame", { t: "ack", rid: `r-${code}`, ok: false, error: { code, message: code } });
}
ok(`${s}/page.json`, "ServerFrame", { t: "page", rid: "r10", groupId: ID.general, events: Object.values(events).map((e, i) => ({ ...e, seq: i + 1 })), hasMore: true, lastSeq: 42 });
ok(`${s}/page-empty.json`, "ServerFrame", { t: "page", rid: "r11", groupId: ID.dm, events: [], hasMore: false, lastSeq: 0 });
ok(`${s}/history.json`, "ServerFrame", { t: "history", rid: "r12", groupId: ID.general, topicId: ID.topic, messages: [messageFull, message, messageDeleted], hasMore: false });
ok(`${s}/directory.json`, "ServerFrame", { t: "directory", members: [ownerFull, botNulls], groups: [dmSummary], removedGroupIds: [ID.general] });
ok(`${s}/directory-members-only.json`, "ServerFrame", { t: "directory", members: [aliceMinimal] }, "other fields omitted");
ok(`${s}/directory-empty.json`, "ServerFrame", { t: "directory" });
ok(`${s}/error.json`, "ServerFrame", { t: "error", error: { code: "unsupported_protocol", message: "protocol 2 is not supported" } });
ok(`${s}/pong.json`, "ServerFrame", { t: "pong" });
add(`${s}/pong-unknown-field.json`, {
  kind: "ServerFrame",
  valid: true,
  description: "unknown fields are ignored on read and dropped on write",
  value: { t: "pong", serverTime: T0 },
  normalized: { t: "pong" },
});
bad(`${s}/unknown-t.json`, "ServerFrame", { t: "kick", reason: "bye" }, "unknown frame type");
bad(`${s}/ack-error-missing-error.json`, "ServerFrame", { t: "ack", rid: "r1", ok: false }, "ok false without error");
bad(`${s}/ack-ok-string.json`, "ServerFrame", { t: "ack", rid: "r1", ok: "true" }, "wrong type for ok");
bad(`${s}/ack-bad-error-code.json`, "ServerFrame", { t: "ack", rid: "r1", ok: false, error: { code: "teapot", message: "x" } }, "unknown error code");
bad(`${s}/page-missing-has-more.json`, "ServerFrame", { t: "page", rid: "r1", groupId: ID.general, events: [], lastSeq: 0 }, "missing hasMore");
bad(`${s}/event-bad-event.json`, "ServerFrame", { t: "event", event: { ...base, type: "nope", data: {} } }, "unknown nested event type");
bad(`${s}/welcome-members-not-array.json`, "ServerFrame", { t: "welcome", protocol: 1, me: aliceMinimal, members: aliceMinimal, groups: [], serverTime: T0 }, "wrong type for members");

// ---------------------------------------------------------------------------
// Payloads and HTTP bodies
// ---------------------------------------------------------------------------

ok("entities/error-body.json", "ErrorBody", { code: "rate_limited", message: "慢一点 🐢" });
ok("http/send-result.json", "SendResult", { message, topic, duplicate: false });
ok("http/error-response.json", "ErrorResponse", { error: { code: "unauthorized", message: "missing or invalid token" } });
ok("http/bootstrap-request.json", "BootstrapRequest", { ownerName: "Joye 乔" });
ok("http/bootstrap-response.json", "BootstrapResponse", { token: "tc_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", member: ownerFull, group: general }, "token is a fake placeholder");
ok("http/redeem-request.json", "RedeemRequest", { inviteCode: "ABCD-EFGH", deviceName: "Alice 的 MacBook Pro" });
ok("http/redeem-response.json", "RedeemResponse", { token: "tc_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB", member: aliceMinimal }, "token is a fake placeholder");
ok("http/me-response.json", "MeResponse", ownerFull);
ok("http/members-response.json", "MembersResponse", [ownerFull, aliceMinimal, botNulls]);
ok("http/members-response-empty.json", "MembersResponse", []);
ok("http/create-invite-request.json", "CreateInviteRequest", { name: "小王", kind: "human", level: "member", role: "后端工程师" });
ok("http/create-invite-request-minimal.json", "CreateInviteRequest", { name: "Ops", kind: "bot", level: "owner" });
ok("http/create-invite-request-expires.json", "CreateInviteRequest", { name: "Lina", kind: "human", level: "member", expiresInHours: 36 }, "a code that works for 36 hours");
bad("http/create-invite-request-expires-too-long.json", "CreateInviteRequest", { name: "Lina", kind: "human", level: "member", expiresInHours: 169 }, "at most a week");
ok("http/create-invite-request-member.json", "CreateInviteRequest", { name: "小王", kind: "human", level: "member", memberId: "01HZX5Q0Y7R0V9C8B7N6M5K4J3" }, "sign-in code for an existing member");
ok("http/create-invite-response.json", "CreateInviteResponse", { code: "WXYZ-2345", expiresAt: T0 + 7 * 86_400_000 }, "code is a fake placeholder");
ok("http/create-bot-request.json", "CreateBotRequest", { name: "Reviewer 🔍", role: "代码审查" });
ok("http/create-bot-request-minimal.json", "CreateBotRequest", { name: "builder" });
ok("http/create-bot-request-character.json", "CreateBotRequest", { name: "Tata", role: "产品负责人", character: "o03" }, "bot with a character");
bad("http/create-bot-request-bad-character.json", "CreateBotRequest", { name: "Tata", character: "tata" }, "unknown character id");
bad("http/create-bot-request-character-null.json", "CreateBotRequest", { name: "Tata", character: null }, "character may be omitted, not null");
ok("http/create-bot-response.json", "CreateBotResponse", { member: botNulls, token: "tc_CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC" }, "token is a fake placeholder");
ok("http/revoke-tokens-request.json", "RevokeTokensRequest", { memberId: ID.bot, all: true });
ok("http/revoke-tokens-request-empty.json", "RevokeTokensRequest", {}, "revoke the calling token");
ok("http/revoke-tokens-response.json", "RevokeTokensResponse", { revoked: 2 });
ok("http/groups-response.json", "GroupsResponse", [generalSummary, dmSummary]);
ok("http/create-group-request.json", "CreateGroupRequest", { name: "前端小组", memberIds: [ID.alice, ID.bot] });
ok("http/add-group-member-request.json", "AddGroupMemberRequest", { memberId: ID.bot });
ok("http/create-dm-request.json", "CreateDmRequest", { memberId: ID.alice });
ok("http/group-summary-response.json", "GroupSummaryResponse", dmSummary);
ok("http/ok-response.json", "OkResponse", { ok: true });
ok("http/health-response.json", "HealthResponse", { ok: true, version: "0.1.0" });
bad("http/bootstrap-request-missing-name.json", "BootstrapRequest", {}, "missing ownerName");
bad("http/create-invite-request-bad-level.json", "CreateInviteRequest", { name: "x", kind: "human", level: "admin" }, "unknown level");
bad("http/create-group-request-ids-not-array.json", "CreateGroupRequest", { name: "x", memberIds: "abc" }, "wrong type for memberIds");
bad("http/ok-response-false.json", "OkResponse", { ok: false }, "ok must be true");
bad("http/error-response-missing-message.json", "ErrorResponse", { error: { code: "internal" } }, "missing message");

// ---------------------------------------------------------------------------
// V1 extensions
// ---------------------------------------------------------------------------

addV1Fixtures({ ok, bad, add, T0, ID, topic, message, generalSummary, dmSummary, ownerFull, aliceMinimal, base });
addBotsFixtures({ ok, bad, T0, ID, base });
addReactionsFixtures({ ok, bad, ID, base, message });
addV11Fixtures({ ok, bad, add, T0, ID, base, message, topic });
addInteractFixtures({ ok, bad, T0, ID, base, message });
addSettingsFixtures({ ok, bad, T0, ID, base });
addGapFixtures({ ok, bad, T0, ID, base, message });
addFilesFixtures({ ok, bad, T0, ID, base, message });
addMemberFixtures({ ok, bad, T0, ID, base, message });
addMemoryFixtures({ ok, bad, T0, ID });
addPushFixtures({ ok, bad, T0, ID });
addUsableFixtures({ ok, bad, T0, ID, base, topic });
addSignInFixtures({ ok, bad, ID });
addHomesFixtures({ ok, ID });
addAiServiceFixtures({ ok, bad });
addSuperviseFixtures({ ok, bad, T0, ID });
addAlertsFixtures({ ok, T0, ID });
addPersonalAiFixtures({ ok, bad, T0, ID });
addHideDeleteFixtures({ ok, bad, T0, ID, base, generalSummary, botMember: botNulls });

// ---------------------------------------------------------------------------

export const fixtures: Tree = new Map([...out].map(([path, f]) => [path, `${JSON.stringify(f, null, 2)}\n`]));

if (import.meta.main) run("fixtures", fixturesRoot, fixtures);
