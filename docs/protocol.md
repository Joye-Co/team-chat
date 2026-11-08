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
  character?: CharacterId | null // the avatar's character from the optional character pack; absent or null = none
  reportsTo?: string           // 1.1: the member it reports to (汇报给); absent = nobody
  weeklyBudgetCents?: number   // 1.1: weekly spending limit in US cents (限额), 0..100,000,000; absent = none
}

CharacterId = "jojo" | "h02" | "h03" | "h04" | "h05" | "h06"
            | "o01" | "o02" | ... | "o20"   // the 26 character pack ids, lower case

Group = {
  id: string
  kind: "group" | "dm"         // a DM is a group of exactly two members
  name: string                 // for DMs the server sets the other member's name per viewer
  memberIds: string[]
  createdBy: string
  createdAt: number
  purpose?: string             // 1.1: the purpose line under the name (1..200; absent = none)
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
  | { type: "group.updated",       data: { name: string, purpose?: string /* 1.1: set when it changed, "" = cleared */ } }
  | { type: "group.memberAdded",   data: { memberId: string } }
  | { type: "group.memberRemoved", data: { memberId: string } }
)
```

V1 adds `card.actioned`, `artifact.created` and `artifact.versionAdded`, and new optional fields
on several entities, frames and events: see [V1 extensions](#v1-extensions). 1.1 adds
`card.answered`, `topic.linked`, `card.retracted` and `artifact.previewReady`: see
[1.1 extensions](#11-extensions).

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
  // without topicId: start a new topic whose root is this message (1.1.1: the 群聊 has
  //   the fixed id mainTopicId(groupId), see "群聊: the group's own chat");
  //   title defaults to the first line of body, cut to 80 characters (an empty body with
  //   replyTo: the quoted message's first line); when no title was sent, the server may
  //   later replace that default once with an AI title (topic.updated with title; needs the
  //   team's AI 服务, see "AI 服务"), unless
  //   someone changed the title first
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
{ t: "ack", rid, ok: false, error: { code: ErrorCode, message: string, reason?, retryAfterMs? /* 1.1 */ } }
{ t: "page", rid, groupId, events: Event[], hasMore: boolean, lastSeq: number }
{ t: "history", rid, groupId, topicId, messages: Message[], hasMore: boolean }
{ t: "directory", members?: Member[], groups?: GroupSummary[], removedGroupIds?: string[] }
{ t: "error", error: { code: ErrorCode, message: string } }   // connection-level problem
{ t: "pong" }

ErrorCode = "unauthorized" | "forbidden" | "not_found" | "invalid" | "conflict"
          | "rate_limited" | "too_large" | "unsupported_protocol" | "internal"
```

- Frame limits per member (all its sockets together): write frames (`send`, `edit`, `delete`,
  `topic.update` and feature writes such as `card.*` and `artifact.*`) about 20 per second with
  bursts of 20; `delta` 100 per second, bursts of 200; reads (`read`, `fetch`, `history`,
  `typing`, `search`) 50 per second, bursts of 100. Over a limit the ack is `rate_limited` with
  `error.retryAfterMs` (1.1): the time until the next frame of that class fits. A rate-limited
  frame gets no write number, so later writes of the socket do not wait for it. Clients pace
  writes below the limit (the bot SDK sends at most 18 per second by default, `writeRate`).

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
  before it broadcasts `message.delta`; only the final `message.updated` is stored as an event.
  The room also keeps the text so far (up to 32,768 characters) in the message, so `history`
  and `fetch` return the current body of a streaming message to members who load it later.
- `hello` is accepted once per socket; a second one gets `invalid`. Events committed while the
  server builds `welcome` are sent right after it (some may be at or below welcome's
  `lastSeq`; clients drop seqs they already have).
- `fetch` and `history` pages also stop at about 4 MiB of JSON (with `hasMore: true`); a page
  always has at least one row.
- A member added to a group starts with everything before it read: its group read position is
  at least the seq of its `group.memberAdded` event.
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
| `POST /v1/invites` | owner | `{ name, kind, level, role?, memberId? }` | `{ code, expiresAt }`; with `memberId`, a sign-in code for that member |
| `POST /v1/bots` | owner | `{ name, role?, character? }` | `{ member, token }` |
| `POST /v1/tokens/revoke` | owner, or the token's member | `{ memberId?, all? }` | `{ revoked }` |
| `GET /v1/groups` | member | | `GroupSummary[]` |
| `POST /v1/groups` | member | `{ name, memberIds }` | `GroupSummary` (creator is added) |
| `POST /v1/groups/:id/members` | group member | `{ memberId }` | `GroupSummary` |
| `DELETE /v1/groups/:id/members/:memberId` | owner, or self | | `{ ok: true }` |
| `POST /v1/dms` | member | `{ memberId }` | `GroupSummary` (find or create) |
| `GET /v1/health` | anyone | | `{ ok: true, version }` |
| `GET /v1/ops/archive` | member | | `{ lastCompleteAt: number \| null, lastRun: { at, complete, failed } \| null }` (daily archive freshness, for the monitor) |

Group changes are also pushed to every affected connection as `directory` frames, and the group
log records `group.memberAdded` / `group.memberRemoved`.

## Tokens and invites

- Token: `tc_` + 43 characters of base64url (32 random bytes). The server stores only the
  SHA-256 hex digest. Tokens do not expire; owners and members can revoke them.
- Invite code: 8 characters from the Crockford alphabet (40 bits), shown as `XXXX-XXXX`, like a
  device code. Stored as a SHA-256 digest of the normalized code (upper case, dashes and spaces
  dropped, I/L read as 1 and O as 0), single use, expires after 7 days. Codes made before this
  format have 12 characters (`XXXX-XXXX-XXXX`) and still redeem. `POST /v1/auth/redeem` allows 10
  attempts per minute per client IP; a right code does not use one up. Over the limit it answers
  429 with the `rate_limited` error.
- Sign-in code for an existing member (`POST /v1/invites` with `memberId`): the same code
  format and rules, but `redeem` gives that member one more token instead of making a new
  member (a new Mac, signing in again after 退出登录, or after reactivation). The member's own
  name, kind and level are used; the request's are ignored. It is refused for a deactivated
  member (`invalid` when made, `unauthorized` when redeemed).
- Bootstrap: the first owner is made with `POST /v1/admin/bootstrap`, protected by the
  `ADMIN_SECRET` Worker secret. It also makes the default group, called "全员" (environments
  bootstrapped before this change called it "General" until it was renamed).
- First redeem of a new member: the member joins every default group and, when it is a human,
  gets a DM with the default bot (the active bot named Jojo, else the oldest active bot), made
  exactly as `POST /v1/dms` makes one (`dm.create` audited by the new member). No bot, no DM; a
  failure here never fails the redeem. A sign-in code of an existing member opens nothing.

# V1 extensions

Everything below is additive: the protocol stays version 1, every new field is optional, and
old clients ignore fields and frames they do not know. Each area has its own Zod module in
`packages/protocol/src/<area>.ts` and its own Rust module in `crates/chat-protocol/src/`.
"Absent means X" below is the default a reader applies; writers may omit a field that has its
default value. Unless a rule says otherwise, a frame that breaks a rule gets `ack` with
`ok: false` and code `invalid` (bad value), `forbidden` (not allowed), `not_found` (unknown id or
not a member of the group) or `too_large`.

## Topics: waitingOn, owner, follow, mute

Module: `topic.ts` / `topic.rs`.

```ts
Topic += {
  waitingOn?: string[]          // member ids the topic waits on; absent = []; at most 20
  ownerId?: string | null       // the one responsible member; absent or null = none
  lastMessageAt?: number        // createdAt of the newest non-deleted message
}
{ t: "topic.update", ..., waitingOn?: string[], ownerId?: string | null }
  // present = replace the whole list / set the owner; null clears the owner
topic.updated data += { waitingOn?: string[], ownerId?: string | null, lastMessageAt?: number }
  // present = changed to this value
{ t: "topic.follow", rid, groupId, topicId, follow: boolean }   // ack.result = TopicState
{ t: "topic.mute", rid, groupId, topicId, muted: boolean }      // ack.result = TopicState
```

Rules:

- Any group member may send `topic.update`. Every id in `waitingOn` and `ownerId` must be a
  current member of the group (`invalid` otherwise). Duplicates in `waitingOn` are removed.
- The server sets `ownerId` to the creator when it creates a topic. A bot that takes the work
  sets itself as owner with `topic.update`.
- The server does not emit `topic.updated` per message. Clients set `lastMessageAt` from each
  `message.created` (its `createdAt`). When the newest message is deleted the server emits
  `topic.updated` with the recomputed `lastMessageAt` (omitted when no message is left).
- Status chips (PRODUCT.md) use `status` together with `waitingOn`.
- `topic.follow` sets my follow state for one topic; the server answers with my `TopicState` and
  sends a `reads` frame to my other connections. Auto-follow: the server sets `following` to true
  when I create the topic, post in it, am mentioned in it, am added to its `waitingOn`, or become
  its owner. An explicit unfollow stays until the next of these.
- `topic.mute` sets my mute state for one topic (话题静音); the server answers with my
  `TopicState` and sends a `reads` frame to my other connections, as for `topic.follow`. In a
  muted topic only mentions of me notify (see Notifications). Muting does not change `following`,
  and auto-follow does not unmute: a mute stays until I unmute.

## Read positions per topic

Module: `reads.ts` / `reads.rs`.

```ts
TopicState = {
  topicId: string
  readSeq: number       // messages of this topic with createdSeq <= readSeq are read
  unread: number        // unread messages in this topic not written by me
  mentions: number      // of those, the ones that mention me
  following?: boolean   // absent = false
  muted?: boolean       // absent = false
}
GroupSummary += { topics?: TopicState[] }
{ t: "read", groupId, seq, topicId?: string }                       // client, no ack
{ t: "reads", groupId, readSeq, unread, mentions, topics: TopicState[] }   // server
```

Rules:

- The effective read position of a topic is `max(group readSeq, topic readSeq)`. Unread counts
  `message.created` events in the topic with `createdSeq` above that position, not written by me,
  not deleted and with `kind` not `system`. `mentions` counts the unread ones that mention me.
- `read` without `topicId` moves the group `readSeq` forward to `seq` (all topics up to `seq`
  are read). With `topicId` it moves only that topic's position forward to `seq`; the group
  `readSeq` does not change. Positions never move back; a `seq` above the group's `lastSeq` is
  cut to `lastSeq`.
- `GroupSummary.unread` and `.mentions` are group totals: the sums over all topics.
  `GroupSummary.readSeq` is the group cursor.
- `GroupSummary.topics` (in `welcome`, `directory` and `GET /v1/groups`) lists every topic with
  `unread > 0`, `mentions > 0`, `following` or `muted`. A topic that is not listed has no
  unread messages, no mentions and is neither followed nor muted.
- After a `read`, `topic.follow` or `topic.mute` changes my state, the server sends `reads` to every connection
  of mine (all devices, the sender too). `topics` holds only the topics that changed; for a
  group-wide read it may be empty, and the client then sets every topic's unread and mentions to
  0 for topics with `lastSeq <= readSeq`. The totals in `reads` are always the new group totals.
- Between `reads` frames, clients count new `message.created` events themselves with the rule
  above. The server is the source of truth on the next `welcome`, `directory` or `reads`.

## Messages: quotes, attachments, cards

Module: `message.ts` / `message.rs`.

```ts
MessageKind = "text" | "card" | "system"
ReplyTo = { messageId: string, authorId: string, excerpt: string }   // excerpt <= 200 code points
FileRef = { fileId, name, size, mime, width?, height?, sha256 }       // name 1..255, sha256 = 64 lowercase hex
ArtifactRef = { artifactId: string, version: number }                // version >= 1

Message += {
  kind?: MessageKind            // absent = "text"
  replyTo?: ReplyTo
  attachments?: FileRef[]       // at most 20; absent = []
  artifactRefs?: ArtifactRef[]  // at most 20; absent = []
  card?: Card                   // present exactly when kind = "card"
}
{ t: "send", ..., replyTo?: { messageId }, attachments?: FileRef[], artifactRefs?: ArtifactRef[], card?: Card }
{ t: "edit", ..., attachments?: FileRef[], artifactRefs?: ArtifactRef[], card?: Card }
  // present = replace; absent = keep
message.updated data += { attachments?, artifactRefs?, card? }   // present = replaced with this value
```

Rules:

- `kind` is not sent by clients. The server sets `card` when `send` has a card, `text`
  otherwise. Only the server makes `system` messages (for example member joins).
- For a card message `body` is the plain-text fallback that old clients, notifications and search
  use. When `body` is empty the server stores the card title as `body`.
- `replyTo`: the client sends only `messageId` (other fields are dropped). The message must be in
  the same group and not deleted. The server fills `authorId` and `excerpt` (the first 200 code
  points of the quoted body, or of the card title).
- `attachments`: each `fileId` must be a `ready` file of the same group (`invalid` otherwise).
  1.1: `source?: { artifactId, version, t /* seconds >= 0 */ }` says the file is a frame of
  that 产出 version (把这一帧发到话题); the version must be one of the same group (`invalid`).
  The server stores its own record for name, size, mime, width, height and sha256 and ignores the
  client's values for those fields.
- `artifactRefs`: each must name an existing 产出 of the same group and an existing version.
- `edit` with `card` is allowed only on a card message and only for its author. `edit` cannot
  change `kind` or `replyTo`.
- `delete` removes the message's content everywhere the server keeps it: `body`, `card`,
  `cardV2` (1.1), the `inputs` and `editedValues` of its `card.answered` events (1.1),
  `replyTo`, `reactions`, `attachments` and `artifactRefs` of the message and of its stored
  events, and the `excerpt` of every `replyTo` that quotes it (set to `""`, the link stays). A
  file that no other message and no 产出 uses is then deleted: `GET /v1/files/:id` and its
  content answer `not_found`, and its bytes are removed from R2. Clients hide the attachments of
  a deleted message.

## Cards

Module: `card.ts` / `card.rs`.

```ts
Card = {
  v: 1
  title: string                                // 1..200
  tone?: "neutral" | "info" | "success" | "warning" | "danger"   // absent = "neutral"
  status?: { label: string }                   // 1..40
  blocks: CardBlock[]                          // at most 50
  actions?: CardAction[]                       // at most 10, ids unique
}
CardBlock =
  | { type: "text", text }                                     // Markdown, <= 8000
  | { type: "fields", fields: { label, value }[] }             // 1..50; label 1..100, value <= 1000
  | { type: "table", columns: string[], rows: string[][] }     // 1..20 columns (<= 100 each),
                                                               // <= 200 rows, one cell per column (<= 1000)
  | { type: "list", items: { text, status? }[] }               // 1..200; text 1..1000;
                                                               // status: todo | in_progress | done | failed | skipped
  | { type: "progress", value: number, label? }                // value 0..1; label <= 200
  | { type: "code", lang?, text }                              // lang <= 32, text <= 32768
  | { type: "image", fileId, alt? }                            // a ready image file of the group; alt <= 500
  | { type: "divider" }
  | { type: "link", label, target }                            // label 1..200
LinkTarget =
  | { kind: "url", url }                                       // http:// or https:// only, <= 2048
  | { kind: "topic", groupId, topicId }
  | { kind: "artifact", artifactId, version? }                 // absent version = current
CardAction = {
  id: string            // 1..64 of [A-Za-z0-9_.:-]
  label: string         // 1..40
  style?: "primary" | "default" | "danger"     // absent = "default"
  confirm?: string      // 1..200; the client asks this question before it sends the action
  value?: JSON          // any JSON, at most 4 KiB as JSON text
}
{ t: "card.action", rid, groupId, messageId, actionId, value?: JSON }   // ack, no result
card.actioned (stored event) data: { messageId, actionId, value?: JSON }  // actorId = presser
```

Rules:

- A card is at most 64 KiB: the UTF-8 length of its JSON text. Larger cards get `too_large`.
- Readers render an unknown block type as nothing (Rust: `CardBlock::Unknown`) so new block
  types can be added; the server rejects unknown block types on write.
- Text is Markdown only in `text` blocks; every other string is plain text.
- `card.action`: any group member may press (a runner bot's approval card acts only on a press by
  the run's requester or an owner, see "Bot runs"). The message must be a card message that is not
  deleted, and `actionId` must exist in its current card (`not_found` otherwise). `value` in the
  frame is ignored in V1: the event carries the action's own `value`. At most one press per
  member per message per second (`rate_limited`).
- The card's author (usually a bot) gets the `card.actioned` event like every member and reacts,
  usually by an `edit` with a new card (for example without the actions, or with a new status).

## Reactions

Module: `reactions.ts` / `reactions.rs`.

```ts
Emoji = string                  // 1..16 code points, no whitespace (one emoji with its modifiers)
Reaction = { emoji: Emoji, memberIds: string[] }   // memberIds oldest first, at least one
Message += { reactions?: Reaction[] }              // at most 50 emoji; absent = []
{ t: "react", rid, groupId, messageId, emoji, on: boolean }   // ack.result = { changed: boolean }
reaction.changed (stored event) data: { messageId, emoji, on }   // actorId = who reacted
```

Rules:

- Any group member may react to a message of the group that is not deleted (`not_found`
  otherwise). A member has at most one reaction per emoji per message: `on: true` adds it,
  `on: false` removes it. When nothing changes the ack has `changed: false` and no event is stored.
- A new emoji is appended to `reactions`; a member is appended to the emoji's `memberIds`. An
  emoji whose last member leaves is removed. A message has at most 50 emoji (`invalid` for the
  51st).
- `Message.reactions` is the current state in `history`, `send` results and archives; the
  `message.created` event carries the state at creation (none). Clients apply
  `reaction.changed` events in `seq` order to keep the list current. A client that does not know
  the event skips it.

## Files

Module: `files.ts` / `files.rs`. All endpoints need a member token; the member must be in the
file's group.

| Method and path | Body | Response |
| --- | --- | --- |
| `POST /v1/files` | `CreateFileRequest { groupId, topicId?, name, size, mime, sha256, width?, height? }` | `CreateFileResponse { file: FileMeta, uploadUrl }` |
| `PUT /v1/files/:id/content` | raw bytes | `FileMeta` with `status: "ready"` |
| `PUT /v1/files/:id/parts/:n` (1.1) | raw bytes of part `n` | `UploadPartResponse { part, size }` |
| `POST /v1/files/:id/complete` (1.1) | | `FileMeta` with `status: "ready"` |
| `GET /v1/files/:id` | | `FileMeta` |
| `GET /v1/files/:id/content` | | the bytes; supports `Range` (206) |
| `PUT /v1/members/:id/avatar` | raw image bytes | `Member` (new `avatarUrl`) |
| `GET /v1/members/:id/avatar` | | the image |

```ts
FileMeta = { id, groupId, topicId?: string | null, name, size, mime, sha256, width?, height?,
             status: "pending" | "ready", createdBy, createdAt }
```

Rules:

- `size` is at most 2,000,000,000 bytes (`MAX_UPLOAD_BYTES`, 2 GB; 1.0 servers: 100 MB). A larger
  file is refused at `POST /v1/files` (`invalid`), before any byte is sent. One `PUT` of the
  content carries at most 100,000,000 bytes (`MAX_FILE_BYTES`, the Workers request body limit, so
  not 100 MiB). `mime` is `type/subtype`. `width`/`height` are pixels, for images only.
- 1.1 multipart: for a file over `MAX_FILE_BYTES`, `CreateFileResponse.multipart = { partBytes,
  parts, partUrl, completeUrl }` (`partBytes` = 52,428,800, `UPLOAD_PART_BYTES`). The creator
  sends `PUT <partUrl>/<n>` for every `n` in 1..`parts`, each exactly `partBytes` bytes (the last
  part the rest), in any order, again after a failure (the newest copy of a part wins). Then
  `POST <completeUrl>`: `invalid` with `parts missing: <k> of <parts>` until every part is in; the
  server joins the parts and checks the SHA-256 of the whole file; on a mismatch it drops the
  parts, the file stays `pending` and the client sends every part again. `PUT …/content` on such
  a file is `too_large`, and parts on a small file are `invalid`. A `pending` multipart file is
  also deleted after 24 hours.
- `uploadUrl` is the absolute URL of `PUT /v1/files/:id/content`; send it with the same bearer
  token. Only the file's creator may upload, once (`conflict` when already `ready`). The server
  checks that the body length equals `size` and its SHA-256 equals `sha256` (`invalid`
  otherwise, and the file stays `pending`). A failed attempt never removes the object of another
  attempt that finished. A `pending` file is deleted after 24 hours.
- Limits per member: 500 files and 5 GiB (declared sizes) per hour (`rate_limited`), and at most
  100 GiB of files that are not deleted (`too_large`).
- Downloads send `Content-Type` from `mime`, `X-Content-Type-Options: nosniff`, and
  `Content-Disposition: attachment` except for `image/png`, `image/jpeg`, `image/gif` and
  `image/webp`. Files are never served as `text/html` inline from the API origin.
- Avatars: self or an owner; `image/png`, `image/jpeg` or `image/webp`, at most 1 MiB. The server
  sets `avatarUrl` to `/v1/members/:id/avatar?v=<n>` (absolute) and pushes a `directory` frame with
  the member. Reading an avatar needs a member token.

## 产出 (artifacts)

Module: `files.ts` / `files.rs`. A 产出 is made from uploaded files (`POST /v1/files` first).

```ts
Artifact = { id, groupId, topicId, title, kind: "html" | "file", currentVersion, createdBy, createdAt, updatedAt }
ArtifactVersion = { artifactId, version, entry, files: { path, fileId, size, mime }[], note?, createdBy, createdAt }
```

| Method and path | Body | Response |
| --- | --- | --- |
| `POST /v1/artifacts` | `CreateArtifactRequest { groupId, topicId, title, kind, entry, files: { path, fileId }[], note? }` | `ArtifactVersionResponse { artifact, version }` (version 1) |
| `POST /v1/artifacts/:id/versions` | `AddArtifactVersionRequest { entry, files, note?, title? }` | `ArtifactVersionResponse` |
| `GET /v1/artifacts?groupId=&topicId=` | | `Artifact[]` (topicId optional), newest `updatedAt` first |
| `GET /v1/artifacts/:id` | | `ArtifactResponse { artifact, versions }`, newest version first |
| `GET /v1/artifacts/:id/versions/:v/files/*path` | | the file bytes |

Rules:

- Any member of the group may create a 产出 or add a version. `title` 1..200; `note` <= 2000.
- `files`: 1..500 entries; each `fileId` is a `ready` file of the same group; paths are unique,
  1..255 characters, relative: split by `/`, no empty, `.` or `..` segment, no `\` or NUL.
  `entry` must be one of the paths. For kind `file` there is exactly one file and `entry` is
  its path. For kind `html` the `entry` file has mime `text/html`.
- `size` and `mime` in a version come from the file records.
- The group log records `artifact.created { artifact, version }` and
  `artifact.versionAdded { artifact, version }` (`actorId` = who made it), so clients update
  the side panel from events.
- File responses for an `html` 产出 send `Content-Security-Policy: sandbox; default-src 'none';
  img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline';
  font-src 'self' data:` and `nosniff`. The desktop window loads them through its own scheme with
  no network (see crates/artifact-view); the CSP protects browsers that open the URL.

## Members and preferences

Module: `members.ts` / `members.rs`.

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `PATCH /v1/members/:id` | owner: every field; self: `name`, `role` | `UpdateMemberRequest { name?, role?: string \| null, level?, deactivated?, character?: CharacterId \| null, reportsTo?: string \| null, weeklyBudgetCents?: number \| null }` | `Member` |
| `PATCH /v1/groups/:id/prefs` | group member, for self | `UpdateGroupPrefsRequest { muted?, notify? }` | `GroupSummary` |
| `PATCH /v1/groups/:id` | group member, or an owner | `UpdateGroupRequest { name?, purpose? }` (at least one) | `GroupSummary` |

```ts
NotifyLevel = "all" | "mentions" | "none"
GroupSummary += { muted?: boolean, notify?: NotifyLevel }
  // absent: muted false; notify "all" for a DM, "mentions" for a group
```

Rules:

- `role` is at most 64 characters; `null` clears it. The server answers `conflict` when the
  change would leave no active owner. Deactivating a member revokes its tokens and closes its
  connections. Every change is audited and pushed to every connection as `directory` with the
  changed `members`.
- `character` is owner only (`forbidden` for a member, also on self); `null` clears it. Only
  the 26 character pack ids are accepted (`invalid` otherwise); a new id is a protocol change,
  because clients bundle the images and reject unknown ids. Clients draw the character with the
  member's live status (idle, working, needs-input, success, error, offline); members without
  one keep their initial avatar. `welcome`, `directory`, `GET /v1/members` and `GET /v1/me`
  carry it.
- 1.1 `reportsTo` and `weeklyBudgetCents` are owner only (`forbidden` for a member, also on
  self); `null` clears them. `reportsTo` must name another existing member (`invalid`
  otherwise). `weeklyBudgetCents` is whole US cents, 0..100,000,000 (`MAX_WEEKLY_BUDGET_CENTS`).
  Clients show it next to the member's cost this week (the sum of `usage.costUsd` of its runs);
  the server stores it and does not stop runs past it in 1.1. Both are audited with
  `member.update` and pushed like the other fields.
- Rename (`PATCH /v1/groups/:id`): any member of the group, or an owner who is not in it, may
  rename a group. A DM cannot be renamed (`forbidden`, message "a DM cannot be renamed"). The
  server trims the name and turns every run of whitespace (also full-width spaces and line
  breaks) into one space, then needs 1..64 characters (`invalid` otherwise). `POST /v1/groups`
  stores names by the same rule. A new name updates D1, is audited (`group.rename`) and
  appends `group.updated { name }` to the group log, so every client's cache and sidebar
  update; the same name again (after normalizing) appends nothing. An owner outside the group
  gets a `GroupSummary` with `readSeq`, `unread` and `mentions` 0.
- Whitespace is Unicode White_Space (`normalizeGroupText` / `normalize_group_text`: U+0009–000D,
  U+0020, U+0085, U+00A0, U+1680, U+2000–200A, U+2028, U+2029, U+202F, U+205F, U+3000), the same
  on both sides.
- 1.1 purpose line: `purpose` is normalized like a name and may be 0..200 characters; `""`
  clears it. Same rights as a rename; audited as `group.purpose` (`{chars}`); the event carries
  `name` and the new `purpose`. A rename alone sends `group.updated { name }`.
- Order: the room appends the event first; D1 then takes the room's name and purpose only when
  the seq of the room's newest `group.updated` event is newer than the one D1 holds
  (`groups.update_seq`). Concurrent changes end with D1 equal to the newest event of the log; a
  retry after a lost D1 write writes D1 again without a new event.
- Prefs are per member. The server pushes the new `GroupSummary` to my other connections as a
  `directory` frame.

## Notifications

Module: `members.ts` / `members.rs`. The server decides; clients only show what it marks.

```ts
NotifyReason = "mention" | "waiting" | "result" | "message"
{ t: "event", event, notify?: NotifyReason }    // present = show a notification
```

The server sets `notify` per receiving member on live `event` frames only (never on `page`).
No notification for an event whose `actorId` is me or when my level is `none`. In a muted group,
and in a topic I muted (`topic.mute`), only rule 1 (`mention`) applies. Otherwise, the first rule
that matches wins:

1. `mention`: `message.created` that mentions me.
2. `waiting`: `topic.updated` whose `waitingOn` now includes me and did not before.
3. `result`: in a topic I follow, `topic.updated` to status `done`, or `artifact.created` /
   `artifact.versionAdded`.
4. `message`: `message.created` (kind not `system`) when my level is `all` (the DM default).

Streaming messages notify once, on `message.created`. A client may still skip showing a
notification while the topic is open and the window is focused. New reasons may be added later;
clients treat an unknown reason as `message` (Rust readers get a parse error on an unknown value,
so the server adds a reason only together with a client release).

## Search

Module: `search.ts` / `search.rs`.

```ts
{ t: "search", rid, query, groupId?, topicId?, limit? }
  // query 1..200; topicId needs groupId; limit 1..50, default 20
{ t: "searchResults", rid, hits: SearchHit[] }   // best match first
SearchHit = { groupId, topicId, messageId, authorId, snippet, highlights?: { start, length }[], createdAt, seq }
```

Rules: only groups I am a member of; without `groupId` all of mine. Deleted messages are not
found. `snippet` is plain text (<= 300 code points) around the match; `highlights` are code-point
ranges in it. `seq` is the message's `createdSeq`. Errors come as an `ack` with `ok: false` and
the same `rid`. Search matches Chinese two-character words (packages/search).

## Updates and downloads

Module: `updates.ts` / `updates.rs`.

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `GET /v1/updates/latest` | member | | `UpdateInfo`; 404 when no release exists |
| `GET /v1/updates/:version/download` | member | | the DMG (`Range` supported) |
| `PUT /v1/admin/releases/:version` | header `X-Admin-Secret` | raw DMG; headers `X-Sha256` (hex), `X-Release-Notes` (base64 of UTF-8 Markdown) | `UpdateInfo` |
| `POST /v1/admin/releases/:version/withdraw` | header `X-Admin-Secret` | `{ restore? }` | `{ version, withdrawn, latest: UpdateInfo \| null }`; a withdrawn release is not `latest` and not downloadable |
| `GET /download` | anyone | | a small page that reads the code from the fragment (`/download#code=<invite code>`) and posts it |
| `POST /download` | anyone with a valid invite code | form `code=<invite code>` | the latest DMG |
| `GET /download?code=<invite code>` | anyone with a valid invite code | | the latest DMG (links made before the fragment form) |

```ts
UpdateInfo = { version, notes, size, sha256, publishedAt, downloadUrl }
  // version "MAJOR.MINOR.PATCH" without leading zeros; notes Markdown <= 20,000; downloadUrl absolute
```

Rules: latest = the highest version by numeric comparison. A `PUT` of an existing version is
`conflict`. The server checks the body's SHA-256 against `X-Sha256` (`invalid` otherwise) and
the body is at most 100 MB. `GET /download` accepts an invite code that is not expired (used or
not), is rate limited per IP, answers 404 with a plain-text body for a bad code, and sends
`Content-Disposition: attachment; filename="<slug>-<version>.dmg"`. Invite links put the code in
the URL fragment, which browsers never send, so no request log (the app's or the Workers
invocation log) records it. Logs must redact the `code` query parameter of old links. Every
publish is audited.

## Diagnostics

Module: `diagnostics.ts` / `diagnostics.rs`.

`POST /v1/diagnostics` (member): body is gzip of UTF-8 text, `Content-Type: application/gzip`,
at most 5 MiB compressed (`too_large`). Response `DiagnosticsResponse { id }`. The client removes
tokens, invite codes and secrets before upload. `GET /v1/diagnostics/:id` (owner) returns the
gzip body. At most 10 uploads per member per hour.

## Integrations

Module: `hooks.ts` / `hooks.rs`.

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `POST /v1/hooks` | owner | `CreateHookRequest { name, groupId, topicId?, kind: "generic" \| "github" }` | `CreateHookResponse { hook, url, secret }` |
| `GET /v1/hooks` | owner | | `Hook[]` |
| `DELETE /v1/hooks/:id` | owner | | `{ ok: true }` |
| `POST /v1/hooks/:id/in` | header `X-Hook-Secret` | `HookInRequest` | `SendResult` |
| `POST /v1/hooks/:id/github` | header `X-Hub-Signature-256` | GitHub webhook JSON | `{ ok: true }` |
| `POST /v1/groups/:id/messages` | group member | `PostMessageRequest` | `SendResult` |

```ts
Hook = { id, name, kind, groupId, topicId?: string | null, memberId, createdBy, createdAt, lastUsedAt?: number | null }
HookInRequest = { topicId?, topicKey?, title?, body?, card?, clientMsgId? }
  // topicId and topicKey exclude each other; body (non-empty) or card is required
  // topicKey: 1..128 of [A-Za-z0-9_.:#/-]
PostMessageRequest = send frame without t, rid and groupId
  // { topicId?, title?, clientMsgId, body, mentions?, streaming?, replyTo?, attachments?, artifactRefs?, card? }
```

Rules:

- Creating a hook makes a bot member (`kind: "bot"`, name = hook name, level `member`) that is
  added to the group; `Hook.memberId` is that member and its posts carry that `authorId`.
  Deleting the hook deactivates that member. `url` is the absolute URL of `/in` (generic) or
  `/github` (github). A generic hook rejects `/github` and the reverse (`not_found`).
- `secret` is `whk_` + 43 base64url characters, shown only in the create response. Because GitHub
  signs with the raw secret, the server stores it encrypted with a Worker secret, not as a digest.
  Comparisons are constant-time. A wrong secret or signature is `unauthorized`.
- `/in` topic choice: `topicId` (a topic of the hook's group); else `topicKey` (the first post
  with a new key starts a topic titled `title`, later posts go to that topic; keys are per hook);
  else the hook's `topicId`; else a new topic. `clientMsgId` dedupes per hook as for `send`.
- `/github` handles `pull_request`, `pull_request_review`, `check_suite`, `workflow_run` and
  `ping`. `check_run` is not a CI source (its checks are in `check_suite` and `workflow_run`); it
  and every other event answer `{ ok: true }` and are dropped. One topic per pull request with topic key
  `github:<owner>/<repo>#<number>`; CI results go to the topic of the pull request of their head
  commit, else to `github:<owner>/<repo>@<branch>`. Posts are card messages.
- `POST /v1/groups/:id/messages` runs exactly the `send` path (same checks, idempotency and
  events) for bots and scripts. Hook and HTTP posts are rate limited per credential.

## Audit

Module: `audit.ts` / `audit.rs`.

`GET /v1/audit?limit=&before=` (owner): `AuditResponse { entries, hasMore }`, newest first.
`limit` 1..200 (default 50); `before` is an entry id (entries older than it).

```ts
AuditEntry = {
  id, at
  actorId: string | null         // the member whose credential made the write; null = admin secret
  requestedBy?: string | null    // the member who asked, when a bot acted for someone
  credential: { kind: "token" | "admin" | "hook" | "invite", id?: string }   // record id, never the secret
  action: string                 // e.g. member.update, hook.create, release.publish
  target?: { kind: string, id: string }
  groupId?: string
  details?: Record<string, JSON> // what changed, e.g. { level: ["member", "owner"] }; secrets removed
}
```

Every HTTP write and every admin action is audited. Message writes over WebSocket are in the group
log and are not repeated in the audit log.

## Bot runs

Modules: `bots.ts` and `machine.ts` / `bots.rs` and `machine.rs`. A runner-backed bot is a bot
member whose work runs as a coding CLI on a team machine (the machine daemon, see
[machine-protocol.md](machine-protocol.md)). Its front desk on the server (BotDesk, one per
bot) turns @mentions into runs and run progress into topic messages.

```ts
RunStatus = "created" | "running" | "waiting" | "blocked" | "done" | "failed" | "cancelled"
BotRun = { id, botId, groupId, topicId, machineId?, status: RunStatus, requestedBy?,
           createdAt, startedAt?, endedAt?, updatedAt, summary? /* <= 280 */,
           plan?: RunPlanItem[] /* the latest plan; absent = none */,
           queuedAt?, queuePosition? /* 1.1, 排队 */ }
RunPlanState = "done" | "current" | "waiting" | "pending"
RunPlanItem = { text /* 1..200 code points */, state: RunPlanState,
                startedAt? /* 1.1, ms: became current */, endedAt? /* 1.1, ms: became done */,
                estimateMs? /* 1.1: the bot's estimate */ }
RunPlan = RunPlanItem[]                    // in order, at most 30 items
StartRunRequest = { groupId, topicId, plan?: RunPlan }
SetRunPlanRequest = { plan: RunPlan }      // replaces the whole plan; [] clears it
EndRunRequest = { status: "done" | "failed" | "cancelled", summary? /* <= 280 */ }
run.updated (stored event) data: { run: BotRun }      // actorId = the bot
RunLogEntry = run.event | run.done | run.session frames of machine-protocol.md, as stored
RunEventsResponse = { run: BotRun, entries: RunLogEntry[], hasMore: boolean, lastSeq: number }

SetBotRunnerRequest = { runtime, labels?: string[], repo?: { url } | { path }, baseBranch?,
                        model?, effort?: string | null /* 思考强度; absent keeps, null clears */, envAllowlist?: string[], enabled? /* absent = true */,
                        machineId?: Id | null /* 1.2.2: absent keeps, null clears */ }
BotRunner = { botId, runtime, labels, repo?, baseBranch?, model?, effort?, envAllowlist, enabled, updatedAt,
              machineId? /* 1.2.2 */ }
Machine = { id, name, labels, createdBy, createdAt, revoked?, connected, healthy,
            lastHeartbeatAt?, daemonVersion?, os?, arch?, maxParallelRuns?, health?: Health,
            clis?: CliInfo[], runningRuns: string[] }
CreateMachineRequest = { name, labels? }      CreateMachineResponse = { machine, token }
```

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `PUT /v1/bots/:id/runner` | owner | `SetBotRunnerRequest` | `BotRunner` |
| `GET /v1/bots/:id/runner` | owner | | `BotRunner`; 404 when the bot is not runner-backed |
| `DELETE /v1/bots/:id/runner` | owner | | `{ ok: true }` |
| `POST /v1/machines` | owner | `CreateMachineRequest` | `CreateMachineResponse` |
| `GET /v1/machines` | member | | `Machine[]`; members other than owners get only `id`, `name`, `createdBy`, `createdAt`, `revoked`, `connected` and `healthy` (`labels` and `runningRuns` empty) |
| `DELETE /v1/machines/:id` | owner | | `{ ok: true }`; revokes the token, closes the link |
| `GET /v1/runs/:id` | member of the run's group | | `BotRun` |
| `GET /v1/runs/:id/events?afterSeq=&limit=` | member of the run's group | | `RunEventsResponse`; `limit` 1..500, default 200 |
| `GET /v1/runs/:id/session` | owner | | the raw CLI session, gzip (`application/gzip`); 404 until uploaded |
| `GET /v1/runs/:id/context` | owner | | `RunContext` ("Run context"); 404 for a run made before it |
| `POST /v1/runs` | a bot that is not runner-backed, member of the group | `StartRunRequest` | `BotRun`: a new `running` run, or the bot's active run in the topic |
| `PUT /v1/runs/:id/plan` | the run's bot | `SetRunPlanRequest` | `BotRun`; `conflict` when the run ended |
| `POST /v1/runs/:id/cancel` | a person in the run's group | | `BotRun` with `status: "cancelled"`; `conflict` when the run ended; audited |
| `POST /v1/runs/:id/end` | the run's bot, runs from `POST /v1/runs` only | `EndRunRequest` | `BotRun`; `conflict` when the run ended |
| `GET /v1/machine/ws` | machine token | | WebSocket upgrade (machine-protocol.md) |
| `PUT /v1/machine/runs/:id/session` | machine token | gzip body | `{ ok: true }` |
| `GET /v1/machine/runs/:id/topic/messages`, `…/topic/messages/:messageId`, `…/search`, `…/files/:fileId` | machine token | | the run's topic context (machine-protocol.md, "Topic context") |

Rules:

- `model` and `effort` go to the CLI as `run.assign.model` and `run.assign.effort` (effort only to
  a machine with feature `effort`). Settings offer the models and efforts the machines report in
  `Machine.clis[].models` / `efforts` (machine-protocol.md, "Models and effort"); `effort` is
  `[a-z][a-z0-9_-]{0,31}`.
- Labels are 1..64 of `[A-Za-z0-9_.:-]`, at most 20. A machine shows its owner labels plus the
  labels its daemon reports, but only owner labels route runs. A machine qualifies for a bot when it has all of the bot's
  labels, reports the bot's `runtime` as available, is connected with a heartbeat in the last
  90 s, has fewer active runs than `maxParallelRuns` and at least 5 GiB free disk (`healthy`).
  Among those the server picks the one with the fewest active runs, then the lowest load.
  1.2.2: a runner with `machineId` runs only on that machine (its labels do not count; the other
  conditions do). `PUT` without `machineId` keeps the stored one, `null` clears it, and an
  unknown or revoked machine is `not_found`. Revoking the machine clears it.
- Trigger: a `message.created` by a human that mentions a runner-backed bot starts a run in
  that topic, unless the topic already has a responsible bot. The first bot that starts a run
  in a topic becomes its only responsible bot. Messages written by bots never start runs or
  reach a run (no loops). In a DM with a runner-backed bot every human message counts as a
  mention of it. A deactivated bot, or one whose runner is disabled, gets no more messages; on
  deactivation its active runs are cancelled and its topics released.
- While a run of the topic's bot is active (`created`, `running`, `waiting`, `blocked`), every
  later human message in the topic goes to that run as input. A message that reaches the run
  after its last turn ended starts a new run. When no run is active, only a new mention of the
  bot starts a run.
- The bot acknowledges the message that started the run with one reaction (👀; before 1.1, a
  short text line). It posts nothing while it works: progress lives in the run (`plan`, the run
  log; before 1.1, one streaming message). When the run ends it posts one result message and
  sets `BotRun.resultMessageId` to it (1.1): on success a card message whose `cardV2` has
  intent `done` (see "Run fields, pause and resume"), else a short note. It sets the topic's `status`:
  `in_progress` while it works, `waiting_answer` with `waitingOn` = the requester while the run
  asks a question (reply 同意 or 拒绝 to an approval, or press the card's buttons; only the
  requester or an owner can answer, every answer is audited as `run.approval`; when the question
  cannot be posted the answer is no), `done` on success, `blocked` with one
  short note when the run fails, its machine stays away past the lease, or no event comes for
  10 minutes, after the desk's own tries (see "Supervision") (the daemon's own `status: stalled` report marks it blocked at once and does not
  count as progress). A blocked run that sends events again goes back to `running`. A run that
  never reaches a machine ends as `failed` after 2 minutes.
- 1.1 queue (排队): when every machine that could take a run is busy (`maxParallelRuns`), the run
  waits instead of failing: it stays `created` with no `machineId` and gets `queuedAt` and
  `queuePosition` (1-based among the bot's queued runs, oldest first), stored with `run.updated`.
  When a run of the bot ends, and every 15 s, the oldest queued run gets a free machine; the
  runs behind it get a `run.updated` with their new place. A queued run is not failed by the
  2-minute rule; it fails with the "no free machine" note after 60 minutes. With no machine
  that could ever take it (labels, runtime, health), the run fails at once as before. Readers:
  "created with no `machineId`" and `queuedAt` = 排队 at `queuePosition`.
- Input left over when a run ends starts a follow-up run, at most 5 in a row; then the bot
  posts one note. A person in the run's group stops a run with `POST /v1/runs/:id/cancel`: the
  machine gets `run.cancel`, the run ends as `cancelled` at once, the bot posts one note and the
  topic goes back to `open`.
- A run that started before 1.1 keeps its streaming progress message until it ends. When the
  run's result is also the last text of that message, the message leaves it out (the result is
  posted once).
- `run.updated` is stored in the group log when a run is created and on every status change, so
  clients show runs from events. The process view reads the run log with
  `GET /v1/runs/:id/events`, paged by `seq` (gaps are normal: the daemon may drop deltas).
- The raw session (the CLI's own file, gzip) is stored in R2 at `sessions/<runId>.jsonl.gz`.
  Only owners can download it.

### Supervision (盯活)

BotDesk watches every run on a machine and steps in by itself before it tells people
(docs/design/agent-harness.zh.md 3.4; rules in `server/src/agent/supervise.ts`). Each run counts
the desk's interventions, and a restarted or continued run carries the count. After 3 the desk
stops trying, and the topic gets one note.

- **Silent 5 minutes**: one `run.input` to the CLI (`inputId` `desk-nudge-…`) asking where it
  stands. It is not a member's message, so it never starts a follow-up run. Any event clears it.
- **Silent 10 minutes, the daemon's `stalled` report, or the machine gone** (no heartbeat for
  2 minutes, or `runLost` past the lease): the run is cancelled without a message (`cancelled`,
  summary `restarted: …`), and its request starts again with the same work order on a healthy
  machine. The topic hears nothing. Out of tries, the old behavior applies: `blocked` with one
  note ("我自己试了几次…还是没有进展").
- **The same step 4 times in a row** (equal `tool.call` titles): one reminder (`desk-loop-…`).
- **A promise without work**: a run that succeeds with a short result such as "我来看看" and called
  no tool gets a follow-up run that continues its session ("do the work now"). At most 3 in a row.
- **Progress questions**: a short "进展怎么样了" / "好了吗" while a run works is answered by the
  bot from the run's state (minutes, plan, last step). The CLI is not interrupted, and the message
  counts as a taken input.

### Front desk

With an AI service set (设置 › 通用 › AI 服务), a runner-backed bot's front desk reads each new
human request before it becomes a run (docs/design/agent-harness.zh.md 3.1-3.2). It reads the
topic (the first message and the newest lines, up to 12,000 characters, as in "Run context")
and makes one forced tool call `decide`, which answers with one of four actions:

- `reply`: the bot answers in the topic at once (conversation, a question the topic answers).
  No run, no machine.
- `dispatch`: a run as before. Its prompt carries the work order (`WorkBrief`: goal, done-when
  checks, notes) after the request.
- `ask`: the bot posts one question and becomes the topic's responsible bot. For an hour the
  requester's next message in the topic counts as addressed to the bot without an @; the front
  desk then decides again with the answer in the topic.
- `skip`: nothing is posted.

The 👀 reaction goes on the request while the front desk reads it. A decision is stored before
it is acted on, so a retried delivery never posts twice. Any doubt becomes a `dispatch`: an
invalid answer, `reply` or `ask` without text, confidence under 0.6, or a provider error
(`fallback` says which). Without an AI service, and for card answers, live events, retries and
side chats, requests become runs as before. A message to an active run never reaches the front
desk.

```ts
WorkBrief = { goal, doneWhen: string[] /* <= 10 */, notes? }
FrontDeskDecision = { messageId, topicId, authorId, action: "reply" | "dispatch" | "ask" | "skip",
                      reason, confidence, reply?, brief?: WorkBrief, fallback?, model?, ms,
                      inputTokens, outputTokens, replyMessageId?, at }
FrontDeskDecisionsResponse = { decisions: FrontDeskDecision[] }   // newest first, at most 20
```

| Method and path | Who | Response |
| --- | --- | --- |
| `GET /v1/bots/:id/decisions?topicId=` | owner | `FrontDeskDecisionsResponse` |

### Run context

What a run's prompt carries from its topic. The server builds the prompt; it is not a client
contract, but its shape is fixed here so the developer view and the daemon notes agree.

- **Lines, not rows.** Each message becomes one compact line, and nothing is dropped because it
  is not text:

  ```
  [#42 m 01J… · 张三 · 11-08 14:02] the body (cut at 600 characters; the rest by id)
    ↪ quotes m 01J… (王五: the excerpt)
    [file f 01J… · 设计稿.png · image/png · 1280×720]
    [产出 a 01J… v3]
    [card · 选部署时间 · 2 answers: 李四 选了「周五晚上」]
  ```

  `#42` is the message's seq. A system message (`kind: "system"`) is one line in brackets.
- **Budget, not a count.** The topic's first message is always there. Then the newest messages,
  newest first, until 24,000 characters; the lines go in oldest first. When messages are left out
  between them, one line says how many and, for a machine with feature `context`, how to read them
  (`chat-run topic history --before <seq>`).
- **Resume sends only what is new.** Each run keeps the highest seq its prompt covered. A run that
  continues the session of the topic's last run (`resumeSessionId`) gets only messages after that
  seq that are not the bot's own, then the request; at most the same budget.
- **群聊.** A run continues the bot's last session in the 群聊 only when its request quotes a
  message of that bot or is a follow-up of its run (input left over). A new request starts a new
  session with the 群聊's lines as above.
- **What it carried.** Every run keeps its prompt and a manifest; owners read them with
  `GET /v1/runs/:id/context`:

  ```ts
  RunContext = { prompt: string, manifest: RunContextManifest }
  RunContextManifest = { resumed: boolean, chars: number, firstSeq?: number, lastSeq?: number,
                         messages: number /* lines carried */, omitted: number /* messages left out */,
                         files: number, cards: number, refs: number }
  ```

### Run plans

A run plan is the bot's ordered to-do list for one run, so members see what it does next:
`done` (finished), `current` (works on it now), `waiting` (waits for a member, such as a
review) and `pending` (later). Only the run's owner sets it: the run's bot with its own token
(`PUT /v1/runs/:id/plan`, or `plan` on `POST /v1/runs`), or the machine that holds the run with
a `plan` run event (machine-protocol.md; the daemon maps Claude Code's TodoWrite and Codex plan
updates to it). Any other caller gets `forbidden`, an unknown run `not_found`, an ended run
`conflict`.

- Every update replaces the whole plan. An empty list clears it: the run then has no `plan`.
- The server keeps only the latest plan with the run and stores a `run.updated` event with the
  whole run (`plan` included, `updatedAt` bumped) on every change, so clients sync it like the
  status. A plan equal to the current one stores nothing. Several plan events in one daemon
  batch store one `run.updated` with the last plan.
- A plan survives the end of the run, as the record of what was done.
- External bots (SDK) make their own runs: `POST /v1/runs` starts a `running` run of the calling
  bot in a topic of a group it is in (or returns its active run there, so a retry makes no
  second run), and `POST /v1/runs/:id/end` ends it. These runs have no machine, no run log and
  no stall detection; they do not change the topic status or its responsible member (the bot
  sets those itself with `topic.update`). A runner-backed bot gets `conflict` on
  `POST /v1/runs`: its runs come from mentions.

# 1.1 extensions

Everything here is additive, as in V1: new optional fields, new frame and event types. No value
is added to a released closed enum (`RunStatus`, `RunPlanState`, `TopicStatus`, `CardTone`,
`ActionStyle`, `CardListStatus`, `LinkTarget.kind`, `NotifyReason`): a 1.0.x Rust reader fails on
an unknown value. New Rust enums of 1.1 that may grow read unknown values as `Unknown` instead.

A 1.0.x client skips the new event types (`LogEntry::Unknown`) and drops the new fields when it
caches a message or a run. The 1.1 client's cache migration `v11-1` therefore fetches every group
log once from the start and re-applies what 1.0.x lost (see "Client cache recovery").

Accepted limitation: a 1.0.x client shows its edited mark on a card message after a state update
that the server makes (an answer, a retract, an expiry of a card ask; a ref whose source was
deleted). 1.0.1 shows the mark for every `message.updated`, and such an update carries
`editedAt` (see "Edit marks" under Cards v2). The message content does not change. A 1.1 client
does not show the mark. The server sends these updates to 1.0.x connections unchanged.

## Cards v2

Module: `card2.ts` / `card2.rs`. A v2 card travels in `Message.cardV2`. The existing `card` field
keeps holding a v1 card, so 1.0.x clients still render and press the message. **Never put
`v: 2` in `card`**: 1.0.x rejects the whole message (and a `history` page that holds it).

```ts
CardV2 = {
  v: 2
  rev: number                       // 1..4294967295; every edit sends a higher rev
  intent: "ask" | "done" | "working" | "info"
  status?: "executing" | "failed"   // 1.1: the author's work state (蓝 处理中, 红 失败); absent = follow intent
  title: string                     // 1..200
  fallback: string                  // <= 4000, plain text: old clients, notifications, search
  ask?: {
    from: string[]                  // 1..20: member ids, "requester" (topic creator), "owners"
    expiresAt?: number              // no answers after this time
    record?: boolean                // an answer is a group decision (群里的决定)
    irreversible?: { verb /* 1..40 */, target /* 1..200 */, consequence /* 1..500 */,
                     typeToConfirm? /* 1..100 */ }
  }
  blocks: CardV2Block[]             // at most 50, group children included; ids unique in the card
  actions?: CardV2Action[]          // at most 10, ids unique
  data?: JSON                       // bot state, <= 16 KiB
}
// Every block has id: 1..64 of [A-Za-z0-9_.:-].
CardV2Block =
  | v1 block types with an id: text, fields, table, list, progress, code, image, divider, link
  | { type: "decision", id, options: { id, label /* 1..200 */, detail? /* <= 1000 */ }[] /* 2..10 */,
      multiple?: boolean, default?: string[] /* option ids */ }
  | { type: "input", id, kind: "text" | "textarea" | "number" | "select" | "date" | "switch" | "chips",
      label /* 1..200 */, placeholder? /* <= 200 */, required?, options?: { id, label }[] /* <= 50 */,
      min?, max?, maxLength? /* 1..32768 */, default?: JSON /* <= 4 KiB */ }
  | { type: "draft", id, label?, text /* Markdown <= 32768 */, editable? }
  | { type: "plan", id, runId?, items?: RunPlanItem[] }      // runId: show that run's live plan
  | { type: "metric", id, label /* 1..100 */, value: number, unit? /* <= 16 */, delta?: number,
      better?: "up" | "down", note? /* <= 200 */ }
  | { type: "chart", id, kind: "line" | "bar" | "area" | "spark", label?, unit?,
      x?: string[] /* <= 200, each <= 40 */, series: { name, values: number[] /* <= 200 */ }[] /* 1..5 */ }
  | { type: "diff", id, path? /* <= 1024 */, text /* unified diff <= 32768 */, additions?, deletions? }
  | { type: "evidence", id, label /* 1..200 */, state?: "pass" | "fail" | "info", detail? /* <= 1000 */,
      fileId?, url?, artifact?: ArtifactRef,
      tests?: { passed, failed, skipped? } /* 1.1: counts from the run's output; read these, not the label */,
      recording?: { fileId, durationMs, posterFileId? } /* 1.1: 录屏, files of the same group */ }
  | { type: "ref", id, label /* 1..200 */, target: RefTarget }
  | { type: "group", id, label?, blocks: CardV2Block[] /* 1..20, no group inside */ }
RefTarget =
  | { kind: "pr", url, repo?, number?, state? }   // state: plain text, e.g. "open", "merged"
  | { kind: "artifact", artifactId, version? }
  | { kind: "topic", groupId, topicId }
  | { kind: "member", memberId }
  | { kind: "url", url }                          // http or https, <= 2048
CardV2Action = { id, label /* 1..40 */, style?: "primary" | "default" | "danger",
                 kind?: "submit" | "cancel" | "link" /* absent = submit */,
                 target?: RefTarget /* exactly for link */, confirm?: { title /* 1..200 */, body? /* <= 1000 */ },
                 role?: string /* 1.1, 1..32, open text */ }
// role "approve" (ACTION_ROLE_APPROVE): the action of a review ask that approves the change
// (merge). Clients show 合并中 after that answer only while the PR link's `merge` has no
// `error`; they never guess the approve action from its id or label. Unknown roles mean nothing.

Message += { cardV2?: CardV2 }
{ t: "send", ..., cardV2?: CardV2 }            // makes a card message
{ t: "edit", ..., cardV2?: CardV2 }            // card messages only; rev must be higher
message.updated data += { cardV2? }            // present = replaced

{ t: "card.answer", rid, groupId, messageId, rev, actionId,
  inputs?: Record<blockId, JSON>, editedValues?: Record<blockId, string> }   // ack, no result
card.answered (stored event) data: { messageId, rev, actionId, inputs?, editedValues? }  // actorId = answerer
```

Every `rev` (the card, `card.answer`, `card.answered`, `GroupDecision` and the run input
`answer`) is an integer from 1 to 4294967295 (2^32 - 1): clients store it as an unsigned
32-bit integer. The server rejects a higher value (`invalid`).

Rules:

- A send or edit with `cardV2` and no `card` gets a `card` that the server derives with
  `cardV2ToV1` (packages/protocol `card2.ts`). A client may send its own `card` too; the server
  then keeps it. `kind` becomes `card`; an empty `body` becomes `fallback`, or the title when
  `fallback` is empty.
- Derived v1 card: `title` = title; `tone` = `warning` (ask), `success` (done), `info` (working),
  `neutral` (info); blocks = a `text` block with `fallback` (when not empty), then each block
  mapped: v1 types as they are (without `id`), `group` = its children, `decision` = a `list` of
  the option labels, `draft` = a `text` block, `plan` = a `list` (done = done, current =
  in_progress, other = todo), `metric` = `fields` (label, value and unit), `diff` = `code` with
  `lang: "diff"`, `evidence` = a `list` item (pass = done, fail = failed), `ref` = a `link`
  (`pr` and `url` as a url link; `member` dropped); `input` and `chart` are dropped; at most 50
  blocks. Actions: the submit and cancel actions (id, label, style, `confirm.title`) only when
  the card has no `input`, `draft` or `decision` block; `link` actions are dropped.
- A v1 `card.action` press on a message that has a `cardV2` counts as a `card.answer` with
  that action, the current rev and no inputs.
- `card.answer`: the message must be a card message with a `cardV2` that is not deleted, `rev`
  must be its current rev (`conflict`: the card changed), `actionId` must be one of its
  submit or cancel actions (`not_found`), every key of `inputs` and `editedValues` must be a
  block id of the card (`invalid`), and `inputs` with `editedValues` are at most 64 KiB as JSON
  (`too_large`). When the card has `ask`: only a member that `ask.from` names may answer
  (`forbidden`; `requester` is the topic's creator, so a bot that knows a run's requester lists
  that member's id; `owners` is every owner); after `ask.expiresAt` the
  answer gets `conflict`; the first answer to a rev wins and later ones get `conflict`. At most
  one answer per member per message per second (`rate_limited`).
- The card's author edits the card after an answer (for example with a higher rev and no
  actions). Clients show a card as answered when a `card.answered` for its current rev exists,
  as expired after `ask.expiresAt`, and as superseded when a later rev exists.
- Answers reach a runner bot's run as `run.input.answer` (machine-protocol.md) and an SDK bot
  as the `card.answered` event. A runner bot gets only answers by people to its own cards: the
  topic's active run takes the answer as input; with no active run, the answer starts a run in
  the card's topic like a mention, with the answer as the request.
- Group decisions (群里的决定): every `card.answered` of a card whose `ask.record` was true at
  the answered rev, with the question (title), the choice (action, chosen option labels and
  inputs), who and when. Clients may derive them from their cache; the server also lists them
  (below), because a later rev of the card may drop `ask`.

| Method and path | Who | Response |
| --- | --- | --- |
| `GET /v1/groups/:id/decisions?beforeSeq=&limit=` | group member | `DecisionsResponse { decisions: GroupDecision[], hasMore, recorded?: RecordedDecision[] }`, newest first; `limit` 1..200, default 50; `beforeSeq` = only answers with a lower seq; `recorded` (1.1) holds up to `limit` decisions recorded by hand, and `hasMore` is true when either list has more |
| `POST /v1/groups/:id/decisions` | group member | 1.1, 记一条: `RecordDecisionRequest { text /* 1..1000, not blank, trimmed */, topicId? }` → `RecordDecisionResponse { decision: RecordedDecision }`; `not_found` for an unknown topic |

```ts
GroupDecision = { seq /* of card.answered */, messageId, topicId, rev, title, actionId,
                  actionLabel, choices?: string[], inputs?, editedValues?, answeredBy, answeredAt }
RecordedDecision = { id, seq /* of decision.recorded */, topicId: string | null, text, recordedBy, recordedAt }
event decision.recorded (stored, 1.1) data: { decision: RecordedDecision }   // actorId = recordedBy
```

A decision recorded by hand is in the group log, so the archive and a restore keep it; 1.0.x
clients skip the event type.

A deleted card's decisions are not listed, nor the decisions of a private card (a non-public
poll, an anonymous questionnaire; see "Private answers").
- Readers are lenient: unknown block types, intents, kinds and styles read as unknown (Rust
  `Unknown`, rendered as nothing or the safe default; 1.1: an unknown block keeps its `type`,
  `id` and `fallbackText` in `CardV2::unknown`, and the client shows the fallback box with it), and a `cardV2` that does not parse is
  dropped while the v1 `card` stays. The server rejects all of these on write.

## Side chats

Module: `topic.ts` / `topic.rs`. A side chat (单独问) is a private conversation with a bot about
a topic. It lives in the asker's DM with the bot, so only the two of them read it, and it
reuses DM sync, history and storage.

```ts
TopicContext = { groupId, topicId, uptoSeq }   // the topic, read up to this seq
Topic += { contextOf?: TopicContext }          // set on a side chat topic
{ t: "send", ..., context?: TopicContext }     // in a DM with a bot only
topic.updated data += { contextOf? }           // the context moved on
AsideOrigin = { groupId /* the DM */, topicId /* the side chat */, messageId /* the answer */ }
{ t: "send", ..., fromAside?: AsideOrigin }     // 1.1: post a side chat answer to its topic
Message += { fromAside?: AsideOrigin }
```

Rules:

- A `send` with `context` and no `topicId` in a DM with a bot starts a side chat: the new
  topic gets `contextOf`. The sender and the bot must both be members of `context.groupId`
  and the topic must exist there (`forbidden` / `not_found`). A later send in that topic may
  carry `context` with the same `groupId` and `topicId` and a higher `uptoSeq`; the server then
  stores `topic.updated { contextOf }`. Any other use of `context` gets `invalid`.
- The bot answers with a read-only run (`BotRun.kind = "aside"`) whose prompt holds the source
  topic up to `uptoSeq` (its last 60 messages; the asker must still be a member of the source
  group) and the side chat so far. The run goes only to a machine whose daemon reports feature
  `aside`; with none it ends `failed` with one note. It sets no status on the side chat topic.
  Its answers stay in the DM. Nothing is posted to the source topic: a
  "post to topic" button on the client sends a normal message by the person.
- 1.1: that message carries `fromAside`. The server checks that the sender is a member of
  `fromAside.groupId`, that its `topicId` is a side chat whose `contextOf` is the send's group
  and topic (`invalid`, reason `aside.bad_origin`; a send without `topicId` too), and that the
  answer message is in it (`not_found`). It stores `fromAside` on the message, so every client
  shows the answer as posted after a restart. A second message with the same
  `fromAside.messageId` in the topic is refused (`conflict`, reason `aside.already_posted`);
  a resend with the same `clientMsgId` is a duplicate as usual.
- Clients list the side chats of a topic by `contextOf` and hide side chat topics from the DM's
  normal topic list.

## 群聊: the group's own chat

Module: `topic.ts` / `topic.rs`. 1.1.1. Every group and DM has one chat timeline, the 群聊, where
members talk without starting a 话题. It is a topic with a fixed id, so clients open it, read its
history and send into it before it exists.

```ts
mainTopicId(groupId) = "main-" + groupId     // MAIN_TOPIC_PREFIX = "main-"
Topic += { main?: boolean }                   // true on the 群聊; absent = false
```

Rules:

- A `send` with `topicId = mainTopicId(groupId)` posts into the 群聊. When it does not exist yet,
  the server creates it in the same commit: `topic.created` (with `main: true`, status `open`,
  `ownerId` null, the title the group's name, or `""` in a DM, and this message as its root),
  then `message.created`; `ack.result.topic` carries the new topic. Any other id with the
  `main-` prefix gets `not_found`. A `send` without `topicId` still starts a new topic, so old
  clients keep working; they show the 群聊 as one more topic.
- 开个话题 (open a topic from a 群聊 message) is a `send` without `topicId` whose `replyTo` names
  that message: the new topic's root quotes it. No new frame.
- `history`, `typing` and `read` with the 群聊 id work before it exists (an empty page; nothing
  to read).
- The 群聊 is a chat, not a piece of work: `topic.update` on it changes nothing and answers the
  topic as it is (bots that report a run's status keep working); a card ask in it does not set
  its status or `waitingOn`; posting or being mentioned in it does not auto-follow it.
- Bots: a message in the 群聊 goes only to the bot it mentions (in a DM with a bot, to that bot).
  No bot becomes the 群聊's responsible member, so each @mention can go to a different bot; a
  bot with an active run there takes a later mention as a follow-up of that run. A card press in
  the 群聊 goes to the card's author when that bot has run there.

## Desk notes

Module: `desk.ts` / `desk.rs` (1.1). A desk note is a bot member's one-line priority for the
首页 desk ("今天先把 1.1 发出去"). It is kept in the group log, so every client syncs it.

```ts
PUT /v1/desk-note  PutDeskNoteRequest { groupId, text /* trimmed: 0..140 code points, "" clears */ }
                   -> PutDeskNoteResponse { seq: number | null /* null: same text, nothing stored */ }
desk.noted (stored event) data: { text }       // actorId = the bot
GET /v1/groups/:id/desk-notes -> DeskNotesResponse { notes: { memberId, text, at, seq }[] /* newest first */ }
```

Rules: only a bot member of the group sets its own note (`forbidden` for people, `not_found`
outside the group). A runner-backed bot sets it from its run with `chat-run desk note --text …`
(the `desk.note` output op, machine-protocol.md), in the run's group. The text is normalized like a group name. Readers keep the newest
`desk.noted` per bot and group; `""` removes it. 1.0.x clients skip the event.

## Run fields, pause and resume

Modules: `bots.ts`, `machine.ts` / `bots.rs`, `machine.rs`.

```ts
BotRun += {
  usage?: { inputTokens, outputTokens, costUsd? }   // from run.done
  durationMs?: number                               // from run.done
  resultMessageId?: string                          // the result message (usually a Card v2, intent done)
  acceptance?: AcceptanceItem[]                     // at most 20; replaced as a whole
  pausedBy?: string                                 // present while paused (接管): who
  kind?: string                                     // "aside" = side chat run; absent = normal
  retryOf?: string                                  // the run this one retries
}
AcceptanceItem = { text /* 1..500 */, state: "pending" | "pass" | "fail",
                   evidence?: { text? /* <= 1000 */, fileId?, url?, artifact?: ArtifactRef } }
SetRunAcceptanceRequest = { items: AcceptanceItem[] /* <= 20; [] clears */ }
ResumeRunRequest = { note?: string /* <= 2000 */ }
Machine += { features?: string[], takeoverUrl?: string }
```

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `POST /v1/runs/:id/pause` | the run's requester or an owner | | `BotRun` with `pausedBy`; `conflict` when the run ended, is paused, or its machine lacks feature `pause`; audited as `run.pause` |
| `POST /v1/runs/:id/resume` | the run's requester or an owner | `ResumeRunRequest` | `BotRun` without `pausedBy`; `conflict` when not paused; audited as `run.resume` |
| `PUT /v1/runs/:id/acceptance` | the run's bot | `SetRunAcceptanceRequest` | `BotRun`; `conflict` when the run ended; an equal list stores nothing |
| `POST /v1/runs/:id/retry` | the run's requester or an owner (people) | | the new `BotRun` with `retryOf`: the same request (author, text, follow-ups) as a new run of the same runner member in the same topic; `conflict` when the run is still active, was started with `POST /v1/runs`, the topic has an active run, or the member cannot take runs; audited as `run.retry` |

Rules:

- The done card of a run (output contract): `intent: "done"`, `title` = the result's first
  line, `fallback` and `body` = the result, `data.runId` = the run, and blocks in this order: a
  `text` block `result`; an `evidence` block `tests` when the result reports test counts
  (`12 passed`, `3 failed`, `12 pass`, `2 skipped`), with the counts in `tests`; one `evidence` block `acceptance-N` per acceptance item
  (`pending` = `info`); one `ref` block `pr-N` per pull request (the topic's `topic.linked`
  links, then GitHub pull request URLs in the result; at most 5); one `ref` block `artifact-N`
  per 产出 the run made (versions with its `runId`, or by the bot in the topic since the run
  started; the newest version each; at most 10). A GitHub pull request URL in the result that
  the topic has no link for yet becomes a `topic.linked` (state `open`, no checks) before the
  card is posted; the GitHub adapter later replaces it with the real state. A card that does not fit the Card v2 limits
  falls back to a text message, and so does a result with no `tests`, acceptance, pull request
  or 产出 block (a plain answer is a text message, not a card). Its v1 `card` is the derived one without the fallback text
  block and the `result` block (the message body holds the result, and 1.0.x draws the body).
  Side chat runs answer with a text message.
- `usage` and `durationMs` are set when the run ends, from `run.done` (also when `run.done`
  comes after a member stopped the run). `acceptance` comes from
  the run event `acceptance` (runner-backed bots) or `PUT /v1/runs/:id/acceptance` (the run's
  bot) and is stored in `run.updated` like the plan. `kind` and the
  closed `RunStatus` are separate: a paused run keeps its status (`running`) and has `pausedBy`.
- Pause stops the CLI's process group on the machine (machine-protocol.md `run.pause`); the
  stall detection and `maxDurationMs` stop until resume. `Machine.takeoverUrl` (owners and run
  requesters only) is a screen sharing address the client may open while paused.
- `Machine.features` (owners only) lists what the daemon supports; see machine-protocol.md.
- Clients compute the desk, active runs across groups, the live terminal (from the run log's
  `tool.call`/`tool.result`) and member stats from their cache; no extra endpoint.

## 产出 events

Module: `files.ts` / `files.rs`.

```ts
ArtifactVersion += { runId?: string, events?: ArtifactEventDecl[] /* <= 20 */, media?: MediaInfo, dataRev?: number,
                     preview?: ArtifactPreview }
ArtifactPreview = { title? /* 1..200 */, summary? /* 1..500 */,
                    metrics?: { label /* 1..40 */, value /* 1..40 */, delta? /* 1..40 */ }[] /* <= 6 */ }
ArtifactEventDecl = { name /* 1..128; writes: [a-z][A-Za-z0-9_.-]{0,63}, unique */, label? /* <= 200 */,
                      kind?: "emit" | "ask" /* default emit */, schema?: TcSchema }
CreateArtifactRequest += { events?, runId?, media?: MediaInfo, data?: JSON /* <= 256 KiB */, preview? }
AddArtifactVersionRequest += { events?, runId?, media?, data?, preview? }
{ t: "artifact.event", rid, groupId, artifactId, version, clientEventId?, name, kind?, payload?: JSON /* <= 16 KiB */ }
   // ack.result (1.1 server) = ArtifactEventAck { eventId, duplicate?, expect: Expect }
```

Page events are **not stored** in the group log (they are transient and audited; the 1.1
pre-release type `artifact.evented` is gone). The bridge frames, transients and data routes are
in [Interaction loop](#interaction-loop-tccard21).

The group room keeps a **page log** of each 产出 (页面里的回应) for 180 days, so every computer of
every member sees it after a restart:

```ts
GET /v1/artifacts/:id/events?before=<eventId>&limit=<1..200, default 100>
  -> ArtifactEventsResponse { events: ArtifactPageEvent[] /* newest first */, hasMore }
ArtifactPageEvent = { eventId, artifactId, version, name, kind: "emit" | "ask", payload?: JSON,
                      actorId, at, replied?: true }
```

Any member of the 产出's group may read it (`not_found` otherwise). `payload` is missing on
events stored before the page log kept payloads.

Rules:

- `preview` (any author) is the text and figures of the version's preview card. Readers use it
  first and scrape the HTML only when a version has none.
- `events` and `runId` are accepted from bot members only; `runId` must be a run of the caller
  in the 产出's topic (else it is dropped). Other callers' values are dropped.
- `TcSchema` is a subset of JSON Schema: `type properties required enum items maxItems minimum
  maximum maxLength pattern (<= 200) description`, depth <= 4, no `$ref`. The server checks a
  `pattern` in linear time (no backtracking), so it refuses one with a backreference (`\1`,
  `\k<…>`) or lookaround (`(?=`, `(?!`, `(?<=`, `(?<!`) when the version is made (`invalid`,
  reason `artifact.bad_schema`). A string checked by a `pattern` has at most 4096 characters.
- Paths under `__teamchat/` are reserved for the page host: create and add version refuse them
  (`invalid`).
- `artifact.event`: a human member of the group; the version exists (`not_found`), its creator is
  a bot member of the group that is not deactivated (`forbidden`, reason `artifact.no_bridge`),
  and it declares `name` with that `kind` (`invalid`, reason `artifact.undeclared_event`); the
  payload fits the schema. At most 5 events per member per second. The event goes to the version's
  creator: transient `artifact.event` (SDK bots) or `run.interaction` / `run.input` (runner bots).
- The desktop gives a page `emit`/`ask` only when its version was made by a bot of the group and
  declares events of that kind. Page events never approve anything: a bot that needs a decision
  posts a Card v2 ask.

## Topic links

Module: `links.ts` / `links.rs`.

```ts
TopicLink = { kind: "pr", repo /* owner/name */, number, url, title /* <= 500 */,
              state /* plain text: open | draft | merged | closed */,
              checks: { name /* 1..200 */, conclusion /* 1..32, "pending" while it runs */ }[] /* <= 100 */,
              additions?, deletions?, changedFiles?,
              merge?: { queuedAt, error? /* 1..200 */ } /* 1.1 */ }
topic.linked (stored event) data: { topicId, link: TopicLink }   // actorId = the integration's bot
```

Rules:

- The GitHub integration stores `topic.linked` for the topic a pull request names
  (`Topic: <id>` in its body) whenever its state, checks or size change. A link with the same
  `kind` and `url` replaces the older one; clients keep the newest per key.
- Readers skip a `kind` they do not know (Rust `TopicLink::Unknown`).
- 1.1 `merge`: the pull request waits to merge. GitHub `pull_request` actions `enqueued` (merge
  queue) and `auto_merge_enabled` set `{ queuedAt }`; `dequeued` and `auto_merge_disabled` keep
  `queuedAt` and set `error` (GitHub's `reason` as plain text, `MERGE_CONFLICT` → `merge
  conflict`, else "merge stopped"). A merged or closed pull request has no `merge`. Clients show
  合并中 only while `merge` is set without `error` and `state` is `open`.

## Client cache recovery

1.0.x clients advanced past the 1.1 event types and dropped 1.1 fields from cached messages,
topics, runs and 产出 versions. The 1.1 desktop cache step `v11-1` records, per group, the seq it
had applied. After the next `welcome` the client fetches each such group's log from seq 0 up to
that seq once, in pages, and re-applies only what is idempotent: `cardV2` of messages, `contextOf`
of topics, newer runs, 产出 versions, and the 1.1 events. Read counts and message counts do not
change. When a group is done its record is removed.

## Interaction loop (tc.card/2.1)

Modules: `card2.ts` / `card2.rs` (cards, events, patches, state, parts), `message.ts` /
`message.rs` (refs), `files.ts` / `files.rs` (产出 bridge), `media.ts` / `media.rs`,
`diagram.ts` / `diagram.rs`. Everything is additive; no closed enum gains a value
(`ErrorCode`, `CardTone`, `TopicStatus`, ... stay as they are).

### Features and catalogs

```ts
hello += { features?: string[] /* "interact/1", "refs/1" */, supportedCatalogs?: string[] /* "tc.card/2.0", "tc.card/2.1" */ }
welcome += { features?: string[] /* "interact/1", "refs/1", "diagram/1" */ }
ErrorBody += { reason?: string /* 1..64, e.g. "card.already_answered" */ }
```

- The new transient types go only to connections whose `hello.features` has `interact/1`.
- The gateway keeps `hello.supportedCatalogs` per connection (the `tc.card/2.x` entries). A live
  `message.created` / `message.updated` event whose `cardV2` catalog (absent = `tc.card/2.0`) is
  in that list comes without the derived v1 `card`; other connections, `history` and `fetch`
  pages keep it.
- A 1.1 client hides an affordance (events, refs, diagrams) the server does not list.
- Errors keep their `ErrorCode`; `reason` names the rule: `card.style_property`,
  `card.unknown_catalog`, `card.unknown_block`, `card.limits`, `card.depth`,
  `card.decision_surfaces`, `card.secret_deferred`, `card.undeclared_event`, `card.block_gone`,
  `card.stale_rev`, `card.superseded`, `card.already_answered`, `card.expired`,
  `card.retract_closed`, `card.rev_limit`, `interact.bot_emitter`, `interact.no_responder`,
  `interact.bad_cause`, `artifact.no_bridge`, `artifact.undeclared_event`, `artifact.stale_data`,
  `artifact.reply_closed`.
- A card that breaks a count or range limit of its shape (blocks, children, options, series,
  points, rows, columns, embed height, rating max, poll voters) gets `card.limits`, the same on
  the server and in the Rust pre-check (`CardV2::problem`). A limit inside a union field (poll
  `voters`: `"group"` or 1..200 member ids) also names `card.limits`. An unknown block type gets `card.unknown_block`;
  an input of kind `secret` gets `card.secret_deferred`. An invalid card fixture names the
  expected reason in its `reason` field, and both fixture tests check it.
- Every `fileId` a card names (`image`, `imageChoice` options, `compare` previews, `evidence`
  and its `posterFileId`) must be a ready file of the same group, and every 产出 (`embed`,
  `evidence.artifact`, `compare` previews, `ref`/`link` blocks and `link` actions with an
  `artifact` target) a 产出 (version) of the same group. Else `invalid`, `card.limits`, with
  `file not in group` or `产出 not in group` in the message; checked on send, edit and patch.

### Card v2.1

```ts
CardV2 += { catalog?: "tc.card/2.0" | "tc.card/2.1" /* absent = 2.0 */, summary? /* <= 280 */ }
CardAsk += { id? /* round, default "a1" */, blocking? /* default intent === "ask" */, promise? /* <= 200 */ }
CardV2Action += { kind: "reject" | "press" (new values of the 1.1 enum), inputs?: blockId[] }
BlockBase (every block) += { emphasis?: "high" | "low", events?: CardEventName[] /* <= 16 */,
                             deliver?: "immediate" | "onSubmit", emitBy?: EmitBy, fallbackText? /* <= 300 */ }
EmitBy = "ask" | "requester" | "members" | "owners" | { memberIds: string[] }
```

Blocks stay one flat list; containers name their children by id (`children`), and a child has
exactly one parent. Containers: `group` (2.0 inline `blocks`, or 2.1 `children` + `title`,
`summary`, `collapsible`, `collapsed`), `stack`, `row {weights?: (1|2|3)[]}`, `compare {question?,
options[2..4] {id, label, pros?, cons?, preview?, recommended?, children?}, note?}`, `tabs
{tabs[2..5] {id, label, children}, selected?}`, `questionnaire {children, submitLabel?,
anonymous?}`. New leaves (catalog 2.1, `fallbackText` required): `rank`, `rating` (max 5),
`scale`, `confirm`, `imageChoice`, `matrix`, `slots`, `poll`, `buttons`, `diagram`, `embed`.
The two containers that ask, `compare` and `questionnaire`, need `fallbackText` too (1.1.0). A
block's `fallbackText` is at most 300 characters; the card's `fallback` stays at most 4000 (a
released 2.0 field: readers show it whole in old clients, notifications and search).
CARD21's `choice` is the 2.0 `decision` block (wire name kept) with `label?`, `required?`,
`help?`, `minSelect?`, `maxSelect?`, `allowOther?`, option `recommended?`. `input` gains kind
`slider` (`step`, `unit`); `secret` is refused. `table` gains `title?`, `rowKeys?` (one key per
row: the only row-key mechanism), `select?: "single" | "multiple"`, `sortable?`. `chart` gains
`series[].id?` and `summary?`. `metric` gains `tone?: "good" | "bad" | "neutral"`.

Write rules (`cardV2Problem` / `CardV2::problem`, run on send, edit and after a patch): known
catalog; 2.1 types only in catalog 2.1; <= 80 blocks; unique ids (`root` and `$…` reserved);
children exist, one parent, no cycle; a container has depth <= 2; no `row` in a `row`, no `tabs`
inside `tabs`/`row`/`compare`, no `compare` inside `row`/`compare`, `questionnaire` children are
leaves; counts (row 2–3, stack/group 1–12, questionnaire 2–5, compare 2–4, tabs 2–5); <= 12
input blocks; <= 20 blocks with `events`; <= 2 `emphasis: "high"`; <= 2 diagrams; `deliver` only
on input blocks, `immediate` only on single choices, `confirm`, `compare`, `rating`, `scale` and
single-line inputs; at most one primary action; action `inputs` name input blocks; one decision
surface per ask. Style keys (`style class className css html color colour background bg font
fontSize fontWeight padding margin gap width height border radius shadow icon`) are refused at any
depth of a block or action, except `style` of actions and inline buttons and `height` of `embed`.
Readers keep unknown event names and an unknown `diagram.syntax` (show the source as code).

### Events, answers and card state

```ts
{ t: "card.event", rid, groupId, messageId, rev, clientEventId, blockId? | actionId?, event: CardEventName,
  value?: JSON /* <= 4 KiB */, inputs?: Record<blockId, JSON> /* <= 16 KiB, action.press only */, part?: PartRef,
  contentHash?: string /* 1.1: sha256 hex of the draft text the answer approves */ }
{ t: "card.answer", ..., clientEventId?, contentHash? }        // short form of card.event action.press
{ t: "card.answer.retract", rid, groupId, messageId, answerId }   // short form of card.event answer.retract
ack.result = CardEventAck { eventId, stored, answerId?, duplicate?, expect: Expect }
Expect = { by: memberId, mode: "live" | "run" | "starting" | "offline" | "none", etaMs? }

card.answered (stored) data += { answerId?, askId?, eventId?, event?, blockId?, value? }   // actionId absent for an immediate commit
card.retracted (stored) data: { messageId, answerId, askId?, eventId? }                     // actorId = the answerer
Message += { cardState?: CardState, cause?: Cause }       message.updated data += { cardState?, cause? }
CardState = { ask?: { id, status: "open"|"answered"|"expired"|"closed"|"superseded", from: memberId[], blocking,
              expiresAt?, answeredBy?, answeredAt? }, answers: CardAnswer[] /* <= 500 */,
              tally?, tallyBy?, emitters?, lastCause?: Cause, answerCount? /* 1.1, private cards */ }
Cause = { eventId, actorId, at, event, blockId?, actionId? }
transient card.event: { groupId, topicId, messageId, eventId, rev, blockId?, actionId?, event, value?, inputs?, part?, actorId, at }
```

Event catalog: `choice.change choice.submit input.change input.submit chart.select chart.clear
metric.press table.rowSelect table.sort button.press draft.edit compare.pick tab.open action.press
answer.retract`. `chart.select` sends `part {kind: "point"|"bar", keys: {series, x}}` next to its
`value {series, x, y, index}`; `metric.press` sends `part {kind: "metric"}`. Other events, and
other part kinds, have no `part` (`invalid`). A live event goes to
the card's author only (transient `card.event`, audit action `card.event`); answers are stored.
Members never write `cardState` or `cause`.

Draft answers: an answer that submits `draft` blocks (a footer action whose inputs name them,
or a 2.0 submit) stores `contentHash` in its `CardAnswer`: the sha256 (lowercase hex of the
UTF-8 bytes) of the draft texts it approves, in card order, each the edited text when edited,
joined with U+0000 (`draftContentText`). A client may send the same `contentHash`; when it
differs from the server's, the answer gets `conflict`, `card.superseded` (the draft changed).

Edit marks: a `message.updated` that the server makes for a state change (an answer, a
retract or an expiry of a card ask; a ref whose source was deleted) has no `cardV2` and carries
`cardState` or `blockRefs`. Its `editedAt` is the stored `editedAt`, or `createdAt` for a message
that was never edited. It is not an edit: a 1.1 client keeps its stored `editedAt` (no edited
mark), and so does a restore. (1.0.x clients copy `editedAt` and show the mark.)

Topic status with blocking asks: while a blocking ask in the topic is open, the topic is
`waiting_answer` and `waitingOn` is the union of the open asks' `from` and the people of a
`waiting_answer` set by `topic.update` (a run's approval). A `topic.update` in that time (for
example a run that ends with `done`) is kept and comes back when the last blocking ask closes;
its ack and `topic.updated` show the combined status. When the last blocking ask closes, the topic
gets that status (or the one it had when the first ask opened; `open` becomes `in_progress`). A
non-blocking ask never changes the topic status.

Private answers: a card with a `poll` whose `public` is not true, or with a `questionnaire` with
`anonymous: true`, is private from its first such rev on. The server keeps every answer; each
member gets its own view, live, in `fetch` pages, in `history` and in acks. The answerer sees
their own answers in full. Every other answer, `card.answered`, `card.retracted` and the state
`message.updated` they cause carry `actorId: "anonymous"` (not a member id), and
`cardState.ask.answeredBy`, `tallyBy` and the voter in `lastCause`/`cause` are left out. The
author of an anonymous questionnaire gets the values of `card.answered` and the other answers
in `cardState.answers` (with `actorId: "anonymous"`). The author of a counts-only poll gets only
the aggregate: `card.answered` without the values, and the `tally` (a runner author's run gets
`run.answer` with `tally` and no value). Other members get neither (a poll's `tally` is for
everyone). Every viewer of a private card gets `cardState.answerCount`, the number of live
answers of the current ask round (the aggregate in place of hidden answers). The derived v1
status of an answered private card is "已回答".

The card's author never learns who used a private card. An SDK author gets the live
`card.event` transient with `actorId: "anonymous"`. A runner author's run gets `run.answer`
and `run.interaction` (and their text forms) with `actor: { id: "anonymous", name: "anonymous" }`
and no level; a run that such an answer or event starts has `requestedBy: "anonymous"`, mentions
nobody in its approvals and waits on nobody (owners approve). Only the group room and the
owners' audit keep the real actor. Group decisions: a private card records none, and
`GET /v1/groups/:id/decisions` leaves out the decisions of a card that became private (a
decision names `answeredBy`). `ask.record` stays valid on a private card.

### Patches and resolves

```ts
{ t: "card.patch", rid, groupId, messageId, baseRev, ops: PatchOp[] /* 1..50 */, cause?: eventId, clientPatchId? }
PatchOp = { op: "set", id, props } | { op: "replace", id, block } | { op: "insert", block, parent?, after? | before? | index? }
        | { op: "remove", id } | { op: "data", path, value? } | { op: "card", props: { title?, summary?, intent?, status?: "executing" | "failed" | null, fallback?, ask?: CardAsk | null, actions? } }
ack.result = CardPatchAck { message, rev, duplicate? }
send += { causeEventId?, blockRefs? }      edit += { baseRev?, causeEventId? }
{ t: "interaction.resolve", rid, groupId, eventId, result: "ignored" | "failed", note? /* <= 120 */ }
transient interaction.resolved: { groupId, eventId, result: "ignored"|"failed"|"patched"|"posted"|"replied", messageId?, note? }
```

A cause (`card.patch.cause`, `send`/`edit` `causeEventId`) and `interaction.resolve` name an
event of the last 30 minutes that the sender received: a card event on one of its cards, or a
page event of one of its 产出 versions (`Cause.event` is the page event's name; the card's
`messageId` is not checked for a page event). `interaction.resolve` is accepted once per event
(`conflict` after that). `artifact.data` with `cause` and `artifact.reply` also send
`interaction.resolved {result: "replied"}` to the emitter and mark the event resolved.

### 产出 bridge

```ts
{ t: "artifact.data", rid, groupId, artifactId, version, data /* <= 256 KiB */, baseDataRev?, cause?, clientPushId? }   // ack.result { dataRev }
{ t: "artifact.reply", rid, groupId, eventId, ok, payload? /* <= 64 KiB, ok: true */, error? { code: "declined"|"failed", message, reason? } }
transient artifact.event: { groupId, topicId, artifactId, version, eventId, name, kind, payload?, actorId, at }   // to the version's creator
transient artifact.data:  { groupId, artifactId, version, dataRev, data? /* inline when <= 64 KiB */, cause? }    // to every member
transient artifact.reply: { groupId, eventId, ok, payload?, error? }                                               // to the asker only
GET /v1/artifacts/:id/versions/:v/data  -> ArtifactDataResponse { data: JSON | null, dataRev, updatedAt }
PUT /v1/artifacts/:id/versions/:v/data  PutArtifactDataRequest { data, baseDataRev?, cause? } -> ArtifactDataAck { dataRev }
```

A page `ask` that no run of the version's runner member will answer (the run ended and none is
open in the topic) gets `artifact.reply` from that member at once: `ok: false`, `error { code:
"declined", reason: "run.ended" }`, so the page's promise settles.

Audit actions: `card.event`, `artifact.event` (payload: first 1 KiB, redacted), `artifact.data`
(`{version, dataRev, bytes}`, no content).

### Media

```ts
MediaInfo = { kind: "html"|"markdown"|"image"|"imageSet"|"video"|"audio"|"pdf"|"code"|"table"|"file", mime, bytes,
              width?, height?, duration?, pages?, rows?, cols?, lines?, language?, posterFileId?,
              frames? { t, fileId, label? }[] /* <= 12 */, chapters? { t, title }[] /* <= 20 */, peaksFileId?,
              transcriptFileId?, images? { fileId, width, height, thumbFileId }[] /* <= 30 */,
              focus? { fromLine, toLine }, preview: "ready" | "pending" | "failed" }
ArtifactVersion += { media? }      FileRef (message attachments) += { media? }
artifact.previewReady (stored) data: { artifactId, version, media }
PUT /v1/artifacts/:id/versions/:v/media  PutArtifactMediaRequest { media: MediaInfo } -> PutArtifactMediaResponse { version }   // stores artifact.previewReady; SDK putArtifactMedia
```

The version's creator may set `media` at any time; another group member only while the stored
`preview` is not `ready` (the receiver fallback). Every file id in `media` must be a ready file
of the group.

The sender's client computes `media`; the server checks the limits (`mediaProblem`: 2 GB, 4 h,
images 50 MB / 100 MP, PDF 500 MB / 2000 pages, HTML 20 MB, Markdown and code 5 MB, tables
200 MB) and never decodes media. It also checks the files that `media` names
(`mediaFileProblem`): a poster at most 200 KB and 1280 px, each frame at most 200 KB and 320 px,
a transcript at most 2 MB (`too_large`).

### References to a part (引用提问)

```ts
PartRef = { kind: PartKind /* 1..16 of [a-z], open set */, keys?: Record</* [a-z]{1,16} */ string, string /* <= 256 */ | int >= 0> /* <= 4 */ }
BlockRefInput = { messageId? | artifact? { artifactId, version } /* exactly one */, sourceGroupId? /* 1.1 */, cardRev?, blockId, part?, label /* 1..120, no control characters */, value? /* <= 2048 bytes */ }
BlockRef = BlockRefInput & { sourceAuthorId?, sourceTopicId, deleted?: true /* label "", no value */ }
Message += { blockRefs?: BlockRef[] /* <= 5 */ }    send / PostMessageRequest += { blockRefs?: BlockRefInput[] /* 1..5 */ }
message.updated data += { blockRefs? }
```

`sourceGroupId` (1.1): a side chat (单独问) in a DM quotes a message or 产出 of the group it is
about. It is allowed only in a DM, the sender must be a member of that group (`forbidden`
otherwise), and that group's room checks the source as it checks its own refs; the stored ref
keeps `sourceGroupId`. Absent (or this group): the source is in this group. A delete in the
source group does not clear the ref of the side chat (the asker keeps the quote line).

`blockId` is a Card v2 block id, `card`, `page` (a 产出 page), `v1:<0..49>` or `md:<0..199>`.
Table keys: `row` = `rowKeys[i]` when present, else the 0-based row index; `col` = the 0-based
column index. The server writes one quote line `> ↳ {label}` per ref in front of the body (then
an empty line), so 1.0.x readers see the reference. Refs travel to runner CLIs inside the prompt
and `run.input.text` (machine protocol unchanged for refs).

### Diagrams

Card v2 block `diagram {syntax: "mermaid", source /* <= 16384 bytes */, title?, caption?, alt?,
fallbackText}`; Markdown fences with info string `mermaid`. `checkDiagramSource` /
`check_diagram_source` (shared cases in `protocol/checks/diagram-checks.json`): first keyword one
of `flowchart graph sequenceDiagram stateDiagram stateDiagram-v2 classDiagram erDiagram gantt
timeline`; no front matter and none of `%%{ click href callback call javascript: <script
<iframe <foreignObject` (any case). The derived v1 card shows the source as a `mermaid` code
block.

# Settings

Additive, as in 1.1. Module: `settings.ts` / `settings.rs`; server feature `server/src/settings/`.
The settings window (设置: panes 账号, 通知, 成员) reads and writes these; clients keep only
sound, badge and window choices locally. Per-group levels (set from the 群's own menu) stay
`PATCH /v1/groups/:id/prefs` (see Notifications).

## Notification settings

```ts
ClockTime = "HH:MM"                     // 24 h, in the member's timeZone
NotifyEvent = "waiting" | "mention" | "result" | "blocked"
  // 需要我拍板的事, 有人 @我, 我交代的事做完了, 成员卡住或出错; open: Rust reads unknown as Unknown
NotifySettings = {
  events: Record<NotifyEvent, boolean>  // 提醒我; default all true
  digestAt: ClockTime | null            // 每日摘要; default null (off)
  quietHours: { enabled, start: ClockTime, end: ClockTime }
                                        // 勿扰; default off, 22:00–08:00
  timeZone: string                      // IANA name, default "Asia/Shanghai"
  updatedAt: number                     // 0 = never changed
}
UpdateNotifySettingsRequest = { events?: Partial<…>, digestAt?: ClockTime | null,
                                quietHours?: Partial<…>, timeZone? }
{ t: "event", event, notify?, notifyEvent?: NotifyEvent }
{ type: "digest", at, since, counts: Record<NotifyEvent, number>,
  items: { groupId, topicId, event: NotifyEvent }[] /* <= 20, newest first */ }   // transient
```

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `GET /v1/me/notify-settings` | member, own | | `NotifySettings` |
| `PATCH /v1/me/notify-settings` | member, own | `UpdateNotifySettingsRequest` | `NotifySettings`; an unknown `timeZone` is `invalid`; audited as `settings.notify` (names of the changed fields only) |

Rules:

- The settings live in the member's gateway, which delivers every event frame to the member
  and applies them there, for every connection (old clients too). The room's decision of
  "Notifications" stays the first step and its `notify` values do not change; the room adds
  `notifyEvent` for `mention`, `waiting` and `result`, and one new case with reason `message`
  (so 1.0.x and 1.1 clients show a plain notification): `blocked` when a topic I follow gets
  status `blocked` (group and topic not muted, level not `none`, not my own change).
- The gateway removes `notify` and `notifyEvent` when the event's switch is off and inside quiet
  hours. A `notify` without `notifyEvent` follows only quiet hours. Quiet hours that cross
  midnight work; equal start and end mean none. The event itself is always delivered.
- Digest: with `digestAt` set, the gateway logs each switched-on `notifyEvent` (also those quiet
  hours held back) and at `digestAt` local time sends one `digest` transient to the
  connections whose hello has feature `settings/1`. When no connection is open, nothing is sent
  and the next digest covers the time since the last one sent. The log keeps 7 days.

## Approval rules for AI members

```ts
ApprovalActions = { mergeMain, deployProd, prodConfig, deleteData, externalPublish: boolean }
  // 合并到 main, 发布到生产, 改生产环境配置, 删除数据或文件, 对外发布（社媒、邮件）; default all true
ApprovalPolicy = { askBefore: ApprovalActions,
                   maxSpendCny: number | null,   // 单次花费超过, whole yuan 1..1000000; null = 不限; default 50
                   approver: "owners" | "requester", // who approves; default "requester"; open: Rust reads unknown as Unknown
                   updatedAt: number }           // 0 = never changed
UpdateApprovalPolicyRequest = { askBefore?: Partial<ApprovalActions>, maxSpendCny?: number | null, approver? }
```

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `GET /v1/settings/approvals` | member | | `ApprovalPolicy` (the team's rules, 默认规则) |
| `PATCH /v1/settings/approvals` | owner | `UpdateApprovalPolicyRequest` | `ApprovalPolicy`; audited as `settings.approvals` |

Rules:

- D1 `approval_policies` (migration 0007) holds one row, scope `*`. Other scopes are reserved.
- BotDesk honors it for every AI member: a runner run's prompt gets one "Team rule" paragraph
  that lists the checked actions and the spend limit and tells the CLI to ask in the topic and
  wait. With `approver: "owners"`, only owners answer the run's approvals (card or 同意/拒绝),
  and the approval card mentions and waits on the active human owners in the group instead of
  the requester. With `"requester"` the released rule stays: the requester or an owner.
- The server cannot tell which action an approval is for, so `approver` is one value for all
  actions.

## AI 服务 (the team's model provider)

The server makes a few model calls of its own (today: the AI title of a new 话题). An owner
picks who answers them and pays with the team's own API key, in 设置 › 通用 › AI 服务. One
deployment is one team, so there is one config.

```ts
AiProvider = "anthropic" | "deepseek" | "openai-compatible"   // open: Rust reads unknown as Unknown
AiServiceConfig = { provider: AiProvider | null,   // null = not set up
                    model: string,                 // "" when not set up, <= 128
                    baseUrl: string | null,        // openai-compatible only; https, <= 256
                    keySet: boolean,
                    keyHint: string | null,        // the key's last 4 characters; never the key
                    updatedAt: number }            // 0 = never changed
UpdateAiServiceRequest = { provider, model /* 1..128 printable, no spaces */,
                           baseUrl?: string | null /* required for openai-compatible */,
                           apiKey?: string /* 8..512 printable; omitted = keep the saved key */ }
TestAiServiceRequest = UpdateAiServiceRequest
AiServiceTestResult = { ok: boolean, error?: string /* the provider's, <= 500 */, latencyMs: number }
```

| Method and path | Who | Body | Response |
| --- | --- | --- | --- |
| `GET /v1/settings/ai` | owner | | `AiServiceConfig` |
| `PUT /v1/settings/ai` | owner | `UpdateAiServiceRequest` | `AiServiceConfig`; audited as `settings.ai` (provider, model, base URL and whether the key changed; never the key) |
| `DELETE /v1/settings/ai` | owner | | `AiServiceConfig` (not set up); audited as `settings.ai` |
| `POST /v1/settings/ai/test` | owner | `TestAiServiceRequest` | `AiServiceTestResult`: one tiny call with these values, nothing saved |

Rules:

- D1 `ai_service` (migration 0009) holds one row, scope `*`. The key is encrypted at rest with
  AES-GCM under the Worker secret `HOOK_SECRET_KEY` (the same secret as integration hook
  secrets), with the scope as additional data. Without that secret, `PUT` and `test` are
  `internal`.
- `PUT` without `apiKey` keeps the saved key only when the provider is the saved one; otherwise
  it is `invalid`. The first `PUT` needs `apiKey`. `test` without `apiKey` uses the saved key
  under the same rule.
- Calls: `anthropic` through the Messages API; `deepseek` (`https://api.deepseek.com`) and
  `openai-compatible` (`baseUrl`) through `POST <base>/chat/completions` with
  `Authorization: Bearer <key>`. A provider error (status, message) is `ok: false` in `test`,
  never an HTTP error of ours. Each call times out after 20 s.
- Suggested models: `deepseek-chat` for DeepSeek, `claude-haiku-5-5` for Anthropic.
- Not set up: a topic keeps its first line as its title.

## Devices

```ts
Device = { id /* token record id, never the token */, name /* deviceName at sign in, <= 128 */,
           createdAt, lastUsedAt: number | null, current: boolean }
DevicesResponse = { devices: Device[] }   // newest use first, at most 100
```

| Method and path | Who | Response |
| --- | --- | --- |
| `GET /v1/me/devices` | member | `DevicesResponse`: my live tokens; `current` = this request's token |
| `DELETE /v1/me/devices/:id` | member, own tokens only | `{ revoked }`; revokes the token and closes its sockets (audited as `token.revoke`); another member's, a revoked or an unknown id is `not_found` |

`lastUsedAt` is updated at most once a minute per token. The name is what the client sent at
sign in; clients pick the device symbol from it.

## Email sign-in

A member who added a sign-in email can sign in on any Mac with a 6-digit code sent to it; the
first sign-in is still an invite code. Each member has at most one sign-in email, and one email
belongs to at most one member (compared in lower case).

```ts
EmailAddress = string            // trimmed, lower case, name@host.tld, <= 254
EmailCodeRequest = { email }
EmailCodeResponse = { sent: true, expiresAt }
EmailSignInRequest = { email, code /* 6 digits */, deviceName /* 1..128 */ }
VerifyEmailRequest = { email, code }
MyEmailResponse = { email: EmailAddress | null }
```

| Method and path | Who | Response |
| --- | --- | --- |
| `POST /v1/auth/email/code` | anyone | `EmailCodeResponse`; sends a sign-in code when the email belongs to an active member, and answers the same either way (no account lookup) |
| `POST /v1/auth/email/sign-in` | anyone | `RedeemResponse`: one more token for that member (audited as `email.sign_in`); a wrong, used or expired code is `unauthorized` |
| `GET /v1/me/email` | member | `MyEmailResponse` |
| `POST /v1/me/email/code` | member | `EmailCodeResponse`; sends a code to the new address. An address another member uses is `conflict` |
| `POST /v1/me/email` | member | `MyEmailResponse`; confirms the code and makes the address my sign-in email (audited as `email.set`) |
| `DELETE /v1/me/email` | member | `MyEmailResponse` with `null` (audited as `email.remove`) |

Rules:

- A code is 6 random digits, stored as a SHA-256 digest, single use, and expires after 10
  minutes. A new code for the same address and purpose replaces the old one. After 5 wrong tries
  a code stops working.
- Sending allows 5 codes per address per hour and 20 per client IP per hour; checking allows 10
  tries per client IP per minute. Over a limit the answer is 429 `rate_limited`.
- The Worker sends mail through the `EMAIL` binding (Cloudflare Email Service) from `EMAIL_FROM`.
  Without both, the code routes answer 400 `invalid` with reason `email.not_configured`, except
  when `ENVIRONMENT` is `local` or `e2e`: there the code goes to the `email_outbox` D1 table
  instead (read by tests with `GET /v1/admin/email-outbox?email=…` and the `x-admin-secret`
  header), and is never sent or logged.
