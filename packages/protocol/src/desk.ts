// 1.1 desk notes (首页): a bot member's one-line priority, set by the bot and kept in the group
// log so every client syncs it (GAPS SF-3). See docs/protocol.md, "Desk notes".
import { z } from "zod";
import { eventBase, Id, Millis, Seq } from "./common.ts";

/** Max code points of a desk note. */
export const MAX_DESK_NOTE_CHARS = 140;
const NoteText = z.string().max(4 * MAX_DESK_NOTE_CHARS);

/** `PUT /v1/desk-note` (a bot member of the group): sets its note; `""` clears it. */
export const PutDeskNoteRequest = z.object({ groupId: Id, text: NoteText });
export type PutDeskNoteRequest = z.infer<typeof PutDeskNoteRequest>;
/** Response of `PUT /v1/desk-note`: the stored event's seq (null = same text, nothing stored). */
export const PutDeskNoteResponse = z.object({ seq: Seq.nullable() });
export type PutDeskNoteResponse = z.infer<typeof PutDeskNoteResponse>;

/** Stored event; `actorId` = the bot. `text` is trimmed, 0..140 code points ("" = cleared). */
export const DeskNotedEvent = z.object({
  ...eventBase,
  type: z.literal("desk.noted"),
  data: z.object({ text: z.string().max(4 * MAX_DESK_NOTE_CHARS) }),
});

export const DeskNote = z.object({ memberId: Id, text: z.string().min(1).max(4 * MAX_DESK_NOTE_CHARS), at: Millis, seq: Seq });
export type DeskNote = z.infer<typeof DeskNote>;
/** `GET /v1/groups/:id/desk-notes`: the newest note per bot member, cleared ones left out. */
export const DeskNotesResponse = z.object({ notes: z.array(DeskNote) });
export type DeskNotesResponse = z.infer<typeof DeskNotesResponse>;
