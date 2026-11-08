// 沉淀 (memory): what the team, a group, an AI member and a person keep for later
// (docs/design/agent-harness.zh.md section 7). See docs/protocol.md, "Memory".
import { z } from "zod";
import { Count, Id, Millis } from "./common.ts";

/** Max characters of a memory's text. */
export const MAX_MEMORY_CHARS = 500;
/** Max characters of `when` (when the memory applies). */
export const MAX_MEMORY_WHEN_CHARS = 300;
/** Max characters of a memory's note (evidence, merge or promotion sources, retire reason). */
export const MAX_MEMORY_NOTE_CHARS = 500;

/** `team`, `group:<groupId>`, `member:<botId>` (an AI member) or `person:<memberId>` (a person). */
export const MemoryScope = z.string().max(200).regex(/^(team|(group|member|person):[A-Za-z0-9_-]+)$/, "team, group:<id>, member:<id> or person:<id>");
export type MemoryScope = z.infer<typeof MemoryScope>;

export const MemoryKind = z.enum(["convention", "fact", "decision", "preference", "lesson", "playbook"]);
export type MemoryKind = z.infer<typeof MemoryKind>;

/** pending: written by an AI, waits for the daily consolidation; active: read by members. */
export const MemoryStatus = z.enum(["active", "retired", "pending"]);
export type MemoryStatus = z.infer<typeof MemoryStatus>;

/** Where a memory came from (来龙去脉): message, run or card ids; manual = a person wrote it. */
export const MemorySource = z.object({
  kind: z.enum(["message", "run", "card", "manual"]),
  ids: z.array(Id).max(20),
});
export type MemorySource = z.infer<typeof MemorySource>;

const MemoryText = z.string().trim().min(1).max(MAX_MEMORY_CHARS);
const MemoryWhen = z.string().trim().max(MAX_MEMORY_WHEN_CHARS);

export const MemoryItem = z.object({
  id: Id,
  scope: MemoryScope,
  kind: MemoryKind,
  text: z.string().min(1).max(MAX_MEMORY_CHARS),
  /** When it applies. */
  when: z.string().max(MAX_MEMORY_WHEN_CHARS).optional(),
  source: MemorySource,
  /** Evidence, the items a promotion or merge came from, why it was retired. */
  note: z.string().max(MAX_MEMORY_NOTE_CHARS).optional(),
  authorId: Id,
  /** A person wrote (or last edited) it: the daily consolidation never changes it. */
  human: z.boolean(),
  status: MemoryStatus,
  confidence: z.number().min(0).max(1),
  uses: Count,
  lastUsedAt: Millis.nullable(),
  version: z.int().min(1),
  createdAt: Millis,
  updatedAt: Millis,
});
export type MemoryItem = z.infer<typeof MemoryItem>;

/** `POST /v1/memories`: a person's memory is active at once; an AI member's waits as pending. */
export const CreateMemoryRequest = z.object({
  scope: MemoryScope,
  kind: MemoryKind,
  text: MemoryText,
  when: MemoryWhen.optional(),
  source: MemorySource.optional(),
});
export type CreateMemoryRequest = z.infer<typeof CreateMemoryRequest>;

/** `PATCH /v1/memories/:id`: a new version. `when: null` clears it; `status` retires or reactivates. */
export const UpdateMemoryRequest = z.object({
  kind: MemoryKind.optional(),
  text: MemoryText.optional(),
  when: MemoryWhen.nullable().optional(),
  status: z.enum(["active", "retired"]).optional(),
});
export type UpdateMemoryRequest = z.infer<typeof UpdateMemoryRequest>;

/** `GET /v1/memories?scope=&status=&q=&limit=` */
export const MemoriesResponse = z.object({ memories: z.array(MemoryItem).max(500) });
export type MemoriesResponse = z.infer<typeof MemoriesResponse>;

export const MemoryVersion = z.object({
  memoryId: Id,
  version: z.int().min(1),
  /** The memory as this version left it. */
  item: MemoryItem,
  /** Who made the version; null = the daily consolidation. */
  by: Id.nullable(),
  at: Millis,
});
export type MemoryVersion = z.infer<typeof MemoryVersion>;

/** `GET /v1/memories/:id/versions`, newest first. */
export const MemoryVersionsResponse = z.object({ versions: z.array(MemoryVersion).max(200) });
export type MemoryVersionsResponse = z.infer<typeof MemoryVersionsResponse>;

/** One line of a day's 记忆更新. `flagged`: the consolidation thinks a person's memory is out of date; nothing changed. */
export const MemoryUpdate = z.object({
  id: Id,
  /** The team's local day of the consolidation, `YYYY-MM-DD`. */
  day: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  scope: MemoryScope,
  memoryId: Id,
  change: z.enum(["added", "changed", "retired", "flagged"]),
  before: MemoryItem.nullable(),
  after: MemoryItem.nullable(),
  note: z.string().max(MAX_MEMORY_NOTE_CHARS).optional(),
  /** Set once the line was undone (`POST /v1/memories/updates/:id/undo`). */
  undoneAt: Millis.nullable(),
  at: Millis,
});
export type MemoryUpdate = z.infer<typeof MemoryUpdate>;

/** `GET /v1/memories/updates?scope=&day=`, oldest first. */
export const MemoryUpdatesResponse = z.object({
  /** The day shown: the asked one, or the newest day with updates (null = none yet). */
  day: z.string().regex(/^\d{4}-\d{2}-\d{2}$/).nullable(),
  updates: z.array(MemoryUpdate).max(500),
});
export type MemoryUpdatesResponse = z.infer<typeof MemoryUpdatesResponse>;

/** `POST /v1/machine/runs/:id/memories` (`chat-run memory note`): a pending memory with the run as its source. */
export const MachineMemoryNoteRequest = z.object({
  scope: z.enum(["member", "person", "group"]),
  kind: MemoryKind,
  text: MemoryText,
  when: MemoryWhen.optional(),
});
export type MachineMemoryNoteRequest = z.infer<typeof MachineMemoryNoteRequest>;
