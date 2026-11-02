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
  character?: CharacterId | null // the avatar's jojo-friends character; absent or null = none
}

CharacterId = "jojo" | "h02" | "h03" | "h04" | "h05" | "h06"
            | "o01" | "o02" | ... | "o20"   // the 26 jojo-friends ids, lower case

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

V1 adds `card.actioned`, `artifact.created` and `artifact.versionAdded`, and new optional fields
on several entities, frames and events: see [V1 extensions](#v1-extensions).

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
| `POST /v1/bots` | owner | `{ name, role?, character? }` | `{ member, token }` |
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
- Invite code: 8 characters from the Crockford alphabet (40 bits), shown as `XXXX-XXXX`, like a
  device code. Stored as a SHA-256 digest of the normalized code (upper case, dashes and spaces
  dropped, I/L read as 1 and O as 0), single use, expires after 7 days. Codes made before this
  format have 12 characters (`XXXX-XXXX-XXXX`) and still redeem. `POST /v1/auth/redeem` allows 10
  attempts per minute per client IP; a right code does not use one up. Over the limit it answers
  429 with the `rate_limited` error.
- Bootstrap: the first owner is made with `POST /v1/admin/bootstrap`, protected by the
  `ADMIN_SECRET` Worker secret. It also makes a group called "General".

# V1 extensions

Everything below is additive: the protocol stays version 1, every new field is optional, and
old clients ignore fields and frames they do not know. Each area has its own Zod module in
`packages/protocol/src/<area>.ts` and its own Rust module in `crates/chat-protocol/src/`.
"Absent means X" below is the default a reader applies; writers may omit a field that has its
default value. Unless a rule says otherwise, a frame that breaks a rule gets `ack` with
`ok: false` and code `invalid` (bad value), `forbidden` (not allowed), `not_found` (unknown id or
not a member of the group) or `too_large`.

## Topics: waitingOn, owner, follow

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

## Read positions per topic

Module: `reads.ts` / `reads.rs`.

```ts
TopicState = {
  topicId: string
  readSeq: number       // messages of this topic with createdSeq <= readSeq are read
  unread: number        // unread messages in this topic not written by me
  mentions: number      // of those, the ones that mention me
  following?: boolean   // absent = false
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
  `unread > 0`, `mentions > 0` or `following`. A topic that is not listed has no unread
  messages, no mentions and is not followed.
- After a `read` or `topic.follow` changes my state, the server sends `reads` to every connection
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
  The server stores its own record for name, size, mime, width, height and sha256 and ignores the
  client's values for those fields.
- `artifactRefs`: each must name an existing 产出 of the same group and an existing version.
- `edit` with `card` is allowed only on a card message and only for its author. `edit` cannot
  change `kind` or `replyTo`.

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
- `card.action`: any group member may press. The message must be a card message that is not
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
| `GET /v1/files/:id` | | `FileMeta` |
| `GET /v1/files/:id/content` | | the bytes; supports `Range` (206) |
| `PUT /v1/members/:id/avatar` | raw image bytes | `Member` (new `avatarUrl`) |
| `GET /v1/members/:id/avatar` | | the image |

```ts
FileMeta = { id, groupId, topicId?: string | null, name, size, mime, sha256, width?, height?,
             status: "pending" | "ready", createdBy, createdAt }
```

Rules:

- `size` is at most 100,000,000 bytes (100 MB; the Workers request body limit, so not 100 MiB).
  `mime` is `type/subtype`. `width`/`height` are pixels, for images only.
- `uploadUrl` is the absolute URL of `PUT /v1/files/:id/content`; send it with the same bearer
  token. Only the file's creator may upload, once (`conflict` when already `ready`). The server
  checks that the body length equals `size` and its SHA-256 equals `sha256` (`invalid`
  otherwise, and the file stays `pending`). A `pending` file is deleted after 24 hours.
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
| `PATCH /v1/members/:id` | owner: every field; self: `name`, `role` | `UpdateMemberRequest { name?, role?: string \| null, level?, deactivated?, character?: CharacterId \| null }` | `Member` |
| `PATCH /v1/groups/:id/prefs` | group member, for self | `UpdateGroupPrefsRequest { muted?, notify? }` | `GroupSummary` |

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
  the 26 jojo-friends ids are accepted (`invalid` otherwise); a new id is a protocol change,
  because clients bundle the images and reject unknown ids. Clients draw the character with the
  member's live status (idle, working, needs-input, success, error, offline); members without
  one keep their initial avatar. `welcome`, `directory`, `GET /v1/members` and `GET /v1/me`
  carry it.
- Prefs are per member. The server pushes the new `GroupSummary` to my other connections as a
  `directory` frame.

## Notifications

Module: `members.ts` / `members.rs`. The server decides; clients only show what it marks.

```ts
NotifyReason = "mention" | "waiting" | "result" | "message"
{ t: "event", event, notify?: NotifyReason }    // present = show a notification
```

The server sets `notify` per receiving member on live `event` frames only (never on `page`).
No notification for an event whose `actorId` is me or when my level is `none`. In a muted group
only rule 1 (`mention`) applies. Otherwise, the first rule that matches wins:

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
| `GET /download?code=<invite code>` | anyone with a valid invite code | | the latest DMG |

```ts
UpdateInfo = { version, notes, size, sha256, publishedAt, downloadUrl }
  // version "MAJOR.MINOR.PATCH" without leading zeros; notes Markdown <= 20,000; downloadUrl absolute
```

Rules: latest = the highest version by numeric comparison. A `PUT` of an existing version is
`conflict`. The server checks the body's SHA-256 against `X-Sha256` (`invalid` otherwise) and
the body is at most 100 MB. `GET /download` accepts an invite code that is not expired (used or
not), is rate limited per IP, answers 404 with a plain-text body for a bad code, and sends
`Content-Disposition: attachment; filename="<slug>-<version>.dmg"`. Logs must redact the `code`
query parameter. Every publish is audited.

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
  `ping`; others answer `{ ok: true }` and are dropped. One topic per pull request with topic key
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
           plan?: RunPlanItem[] /* the latest plan; absent = none */ }
RunPlanState = "done" | "current" | "waiting" | "pending"
RunPlanItem = { text /* 1..200 code points */, state: RunPlanState }
RunPlan = RunPlanItem[]                    // in order, at most 30 items
StartRunRequest = { groupId, topicId, plan?: RunPlan }
SetRunPlanRequest = { plan: RunPlan }      // replaces the whole plan; [] clears it
EndRunRequest = { status: "done" | "failed" | "cancelled", summary? /* <= 280 */ }
run.updated (stored event) data: { run: BotRun }      // actorId = the bot
RunLogEntry = run.event | run.done | run.session frames of machine-protocol.md, as stored
RunEventsResponse = { run: BotRun, entries: RunLogEntry[], hasMore: boolean, lastSeq: number }

SetBotRunnerRequest = { runtime, labels?: string[], repo?: { url } | { path }, baseBranch?,
                        model?, envAllowlist?: string[], enabled? /* absent = true */ }
BotRunner = { botId, runtime, labels, repo?, baseBranch?, model?, envAllowlist, enabled, updatedAt }
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
| `GET /v1/machines` | owner | | `Machine[]` |
| `DELETE /v1/machines/:id` | owner | | `{ ok: true }`; revokes the token, closes the link |
| `GET /v1/runs/:id` | member of the run's group | | `BotRun` |
| `GET /v1/runs/:id/events?afterSeq=&limit=` | member of the run's group | | `RunEventsResponse`; `limit` 1..500, default 200 |
| `GET /v1/runs/:id/session` | owner | | the raw CLI session, gzip (`application/gzip`); 404 until uploaded |
| `POST /v1/runs` | a bot that is not runner-backed, member of the group | `StartRunRequest` | `BotRun`: a new `running` run, or the bot's active run in the topic |
| `PUT /v1/runs/:id/plan` | the run's bot | `SetRunPlanRequest` | `BotRun`; `conflict` when the run ended |
| `POST /v1/runs/:id/end` | the run's bot, runs from `POST /v1/runs` only | `EndRunRequest` | `BotRun`; `conflict` when the run ended |
| `GET /v1/machine/ws` | machine token | | WebSocket upgrade (machine-protocol.md) |
| `PUT /v1/machine/runs/:id/session` | machine token | gzip body | `{ ok: true }` |

Rules:

- Labels are 1..64 of `[A-Za-z0-9_.:-]`, at most 20. A machine's labels are its owner labels plus
  the labels its daemon reports. A machine qualifies for a bot when it has all of the bot's
  labels, reports the bot's `runtime` as available, is connected with a heartbeat in the last
  90 s, has fewer active runs than `maxParallelRuns` and at least 5 GiB free disk (`healthy`).
  Among those the server picks the one with the fewest active runs, then the lowest load.
- Trigger: a `message.created` by a human that mentions a runner-backed bot starts a run in
  that topic, unless the topic already has a responsible bot. The first bot that starts a run
  in a topic becomes its only responsible bot. Messages written by bots never start runs or
  reach a run (no loops).
- While a run of the topic's bot is active (`created`, `running`, `waiting`, `blocked`), every
  later human message in the topic goes to that run as input. A message that reaches the run
  after its last turn ended starts a new run. When no run is active, only a new mention of the
  bot starts a run.
- The bot answers once with a short acknowledgement, streams progress into one streaming
  message, and posts the result when the run ends. It sets the topic's `status`:
  `in_progress` while it works, `waiting_answer` with `waitingOn` = the requester while the run
  asks a question (reply 同意 or 拒绝 to an approval), `done` on success, `blocked` with one
  short note when the run fails, its machine stays away past the lease, or no event comes for
  10 minutes. A blocked run that sends events again goes back to `running`.
- `run.updated` is stored in the group log when a run is created and on every status change, so
  clients show runs from events. The process view reads the run log with
  `GET /v1/runs/:id/events`, paged by `seq` (gaps are normal: the daemon may drop deltas).
- The raw session (the CLI's own file, gzip) is stored in R2 at `sessions/<runId>.jsonl.gz`.
  Only owners can download it.

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
