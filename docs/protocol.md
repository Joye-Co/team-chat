# Wire protocol (version 1)

This file is the contract between the desktop client, the server and the machine daemon.
`packages/protocol` (Zod) is the executable source of truth; `crates/chat-protocol` (serde)
must accept every file in `protocol/fixtures/` and serialize it back to an equal JSON value.
Change this file, both implementations and the fixtures in the same commit.

## Conventions

- JSON, UTF-8, field names in camelCase. Unknown fields are ignored on read.
- IDs are ULIDs (26 characters, Crockford base32) made by the server. The only exception is
  `clientMsgId`, which the client makes (ULID or UUID v4 string).
- Times are integers: milliseconds since the Unix epoch, UTC.
- Optional fields may be omitted. `null` means "explicitly empty" only where a field says so.
- Limits: message `body` at most 32,768 characters; topic `title` at most 200 characters;
  `fetch`/`history` `limit` at most 500; one WebSocket frame at most 1 MiB.

## Entities

```ts
Member = {
  id: string
  kind: "human" | "bot"
  name: string                 // display name, 1..64 chars
  role?: string | null         // the member's job ("岗位"), shown under the name
  level: "owner" | "member"    // owner = full admin
  avatarUrl?: string | null
  deactivated?: boolean
}

Group = {
  id: string
  kind: "group" | "dm"         // a DM is a group of exactly two members
  name: string                 // for DMs the server sets the other member's name per viewer
  memberIds: string[]
  createdBy: string
  createdAt: number
}

GroupSummary = {
  group: Group
  lastSeq: number              // seq of the newest event in the group log (0 = empty)
  readSeq: number              // this member's read cursor
  unread: number               // message.created events after readSeq not written by me
  mentions: number             // unread messages that mention me
}

TopicStatus = "open" | "in_progress" | "waiting_answer" | "waiting_review" | "blocked" | "done"

Topic = {
  id: string
  groupId: string
  title: string
  status: TopicStatus
  createdBy: string
  createdAt: number
  rootMessageId: string
  lastSeq: number              // seq of the newest event that touched this topic
  messageCount: number
}

Message = {
  id: string
  clientMsgId: string
  groupId: string
  topicId: string
  authorId: string
  body: string                 // Markdown
  mentions: string[]           // member ids, validated against group membership
  createdSeq: number
  createdAt: number
  editedAt?: number | null
  deletedAt?: number | null    // a deleted message keeps its row; body becomes ""
  streaming?: boolean          // true while a bot is still writing it
}
```

## Group event log

Every group has one ordered log. The group room assigns `seq` (1, 2, 3, ... with no gaps) and
stores every event. Clients apply events strictly in `seq` order.

```ts
Event = { seq: number, groupId: string, at: number, actorId: string } & (
  | { type: "topic.created",       data: { topic: Topic } }
  | { type: "topic.updated",       data: { topicId: string, title?: string, status?: TopicStatus } }
  | { type: "message.created",     data: { message: Message } }
  | { type: "message.updated",     data: { messageId: string, body: string, editedAt: number, streaming: boolean } }
  | { type: "message.deleted",     data: { messageId: string, deletedAt: number } }
  | { type: "group.updated",       data: { name: string } }
  | { type: "group.memberAdded",   data: { memberId: string } }
  | { type: "group.memberRemoved", data: { memberId: string } }
)
```

Transient events are delivered once, have no `seq` and are never stored:

```ts
TransientEvent =
  | { type: "typing",        groupId: string, topicId: string, memberId: string, at: number }
  | { type: "presence",      memberId: string, online: boolean, at: number }
  | { type: "message.delta", groupId: string, topicId: string, messageId: string, append: string }
```

## WebSocket

`GET {base}/v1/ws` with header `Authorization: Bearer <token>`. The server rejects a missing or
invalid token with HTTP 401 before the upgrade. One member may have several connections
(several devices); each connection gets every event.

Every frame has a `t` field. Frames that expect an answer carry `rid` (a client-made string);
the server answers with an `ack` (or `page`) that has the same `rid`.

### Client to server

```ts
{ t: "hello", protocol: 1, client: { name: string, version: string }, cursors?: Record<string, number> }
  // first frame on every connection; cursors = last applied seq per group id
{ t: "send", rid, groupId, topicId?: string, title?: string, clientMsgId, body, mentions?: string[], streaming?: boolean }
  // without topicId: start a new topic whose root is this message;
  //   title defaults to the first line of body, cut to 80 characters
  // ack.result = { message: Message, topic?: Topic, duplicate?: boolean }
{ t: "edit", rid, groupId, messageId, body, streaming?: boolean }      // author only
{ t: "delta", groupId, messageId, append }                            // streaming author only, no ack
{ t: "delete", rid, groupId, messageId }                              // author or owner
{ t: "topic.update", rid, groupId, topicId, title?, status? }
{ t: "typing", groupId, topicId }                                     // at most one per 3 s per topic
{ t: "read", groupId, seq }                                           // move my read cursor forward only
{ t: "fetch", rid, groupId, afterSeq: number, limit?: number }        // events with seq > afterSeq
{ t: "history", rid, groupId, topicId, beforeSeq?: number, limit?: number }
  // messages of one topic, newest first, created before beforeSeq
{ t: "ping" }
```

### Server to client

```ts
{ t: "welcome", protocol: 1, me: Member, members: Member[], groups: GroupSummary[], serverTime: number }
{ t: "event", event: Event }
{ t: "transient", event: TransientEvent }
{ t: "ack", rid, ok: true, result?: unknown }
{ t: "ack", rid, ok: false, error: { code: ErrorCode, message: string } }
{ t: "page", rid, groupId, events: Event[], hasMore: boolean, lastSeq: number }
{ t: "history", rid, groupId, topicId, messages: Message[], hasMore: boolean }
{ t: "directory", members?: Member[], groups?: GroupSummary[], removedGroupIds?: string[] }
{ t: "error", error: { code: ErrorCode, message: string } }   // connection-level problem
{ t: "pong" }

ErrorCode = "unauthorized" | "forbidden" | "not_found" | "invalid" | "conflict"
          | "rate_limited" | "too_large" | "unsupported_protocol" | "internal"
```

### Rules

- Order and gaps: a client keeps `lastApplied` per group. An event with `seq <= lastApplied` is
  ignored. An event with `seq > lastApplied + 1` means a gap: the client buffers it and sends
  `fetch` with `afterSeq = lastApplied` until the gap is closed.
- Catch-up: after `welcome`, the client sends `fetch` for every group whose `lastSeq` is greater
  than its own `lastApplied`, page by page while `hasMore` is true.
- Idempotency: `(authorId, clientMsgId)` is unique per group. Sending the same pair again
  returns the original message with `duplicate: true` and creates no event.
- A message is sent only when its `ack` arrives with `ok: true`. Clients show unacked messages
  as pending and resend them with the same `clientMsgId` after a reconnect.
- Streaming: a bot sends `send` with `streaming: true`, then `delta` frames, then `edit` with the
  full final body and `streaming: false`. The room coalesces deltas (every 100 ms or 8 KiB)
  before it broadcasts `message.delta`; only the final `message.updated` is stored.
- Keepalive: the client sends `ping` every 30 s; the server answers `pong` without waking up the
  object. The client reconnects with exponential backoff (0.5 s to 30 s, with jitter).

## HTTP API

All requests send `Authorization: Bearer <token>` except `redeem` and `bootstrap`. Responses are
JSON. Errors use HTTP status codes and the body `{ error: { code, message } }`.

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `POST /v1/admin/bootstrap` | header `X-Admin-Secret` | `{ ownerName }` | `{ token, member, group }`; only while no member exists |
| `POST /v1/auth/redeem` | anyone with an invite code | `{ inviteCode, deviceName }` | `{ token, member }` |
| `GET /v1/me` | member | | `Member` |
| `GET /v1/members` | member | | `Member[]` |
| `POST /v1/invites` | owner | `{ name, kind, level, role? }` | `{ code, expiresAt }` |
| `POST /v1/bots` | owner | `{ name, role? }` | `{ member, token }` |
| `POST /v1/tokens/revoke` | owner, or the token's member | `{ memberId?, all? }` | `{ revoked }` |
| `GET /v1/groups` | member | | `GroupSummary[]` |
| `POST /v1/groups` | member | `{ name, memberIds }` | `GroupSummary` (creator is added) |
| `POST /v1/groups/:id/members` | group member | `{ memberId }` | `GroupSummary` |
| `DELETE /v1/groups/:id/members/:memberId` | owner, or self | | `{ ok: true }` |
| `POST /v1/dms` | member | `{ memberId }` | `GroupSummary` (find or create) |
| `GET /v1/health` | anyone | | `{ ok: true, version }` |

Group changes are also pushed to every affected connection as `directory` frames, and the group
log records `group.memberAdded` / `group.memberRemoved`.

## Tokens and invites

- Token: `tc_` + 43 characters of base64url (32 random bytes). The server stores only the
  SHA-256 hex digest. Tokens do not expire; owners and members can revoke them.
- Invite code: 12 characters from the Crockford alphabet, shown as `XXXX-XXXX-XXXX`. Stored as a
  SHA-256 digest, single use, expires after 7 days.
- Bootstrap: the first owner is made with `POST /v1/admin/bootstrap`, protected by the
  `ADMIN_SECRET` Worker secret. It also makes a group called "General".
