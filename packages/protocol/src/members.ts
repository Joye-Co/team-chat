// V1 member changes, group notification preferences and the server's notify decision.
// See docs/protocol.md, "Members and preferences" and "Notifications".
import { z } from "zod";
import { CharacterId } from "./characters.ts";
import { MemberLevel, Name } from "./common.ts";

/** `PATCH /v1/members/:id`. Owner: every field. Self: `name` and `role` only. */
export const UpdateMemberRequest = z.object({
  name: Name.optional(),
  /** null clears the role. */
  role: z.string().max(64).nullable().optional(),
  level: MemberLevel.optional(),
  deactivated: z.boolean().optional(),
  /** Owner only. null clears the character. */
  character: CharacterId.nullable().optional(),
});
export type UpdateMemberRequest = z.infer<typeof UpdateMemberRequest>;

export const NotifyLevel = z.enum(["all", "mentions", "none"]);
export type NotifyLevel = z.infer<typeof NotifyLevel>;

/** Fields added to `GroupSummary`. Absent: muted false; notify "all" for a DM, "mentions" for a group. */
export const groupSummaryPrefFields = {
  muted: z.boolean().optional(),
  notify: NotifyLevel.optional(),
};

/** `PATCH /v1/groups/:id/prefs`. Response: `GroupSummary`. */
export const UpdateGroupPrefsRequest = z.object({
  muted: z.boolean().optional(),
  notify: NotifyLevel.optional(),
});
export type UpdateGroupPrefsRequest = z.infer<typeof UpdateGroupPrefsRequest>;

/** Why an event should notify the receiving member. */
export const NotifyReason = z.enum(["mention", "waiting", "result", "message"]);
export type NotifyReason = z.infer<typeof NotifyReason>;

/** Field added to the `event` server frame: present = show a notification. */
export const eventFrameFields = {
  notify: NotifyReason.optional(),
};
