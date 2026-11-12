// V1 member changes, group notification preferences and the server's notify decision.
// See docs/protocol.md, "Members and preferences" and "Notifications".
import { z } from "zod";
import { CharacterId } from "./characters.ts";
import { Id, MemberLevel, Name } from "./common.ts";

/** Largest weekly spending limit (US cents): 1,000,000 USD. */
export const MAX_WEEKLY_BUDGET_CENTS = 100_000_000;

/** `PATCH /v1/members/:id`. Owner: every field. Self: `name` and `role` only. */
export const UpdateMemberRequest = z.object({
  name: Name.optional(),
  /** null clears the role. */
  role: z.string().max(64).nullable().optional(),
  level: MemberLevel.optional(),
  deactivated: z.boolean().optional(),
  /** Owner only. null clears the character. */
  character: CharacterId.nullable().optional(),
  /** 1.1, owner only: another member's id; null clears it. */
  reportsTo: Id.nullable().optional(),
  /** 1.1, owner only: the weekly spending limit in US cents; null clears it. */
  weeklyBudgetCents: z.int().nonnegative().max(MAX_WEEKLY_BUDGET_CENTS).nullable().optional(),
  /** Usable, 所有者 only: true makes a person a 所有者 (and `level` "owner"); false keeps them an admin. */
  teamOwner: z.boolean().optional(),
});
export type UpdateMemberRequest = z.infer<typeof UpdateMemberRequest>;

export const NotifyLevel = z.enum(["all", "mentions", "none"]);
export type NotifyLevel = z.infer<typeof NotifyLevel>;

/** Fields added to `GroupSummary`. Absent: muted false; notify "all" for a DM, "mentions" for a group. */
export const groupSummaryPrefFields = {
  muted: z.boolean().optional(),
  notify: NotifyLevel.optional(),
  /** I hid this 群 or 私聊 until a new message arrives. Absent means false. */
  hidden: z.boolean().optional(),
};

/** `PATCH /v1/groups/:id/prefs`. Response: `GroupSummary`. */
export const UpdateGroupPrefsRequest = z.object({
  muted: z.boolean().optional(),
  notify: NotifyLevel.optional(),
  /** Hide or show this 群 or 私聊 for me (until a new message arrives). */
  hidden: z.boolean().optional(),
});
export type UpdateGroupPrefsRequest = z.infer<typeof UpdateGroupPrefsRequest>;

/**
 * The whitespace both sides collapse in group names and purposes: Unicode White_Space, the
 * set of Rust `char::is_whitespace` (JS `\s` adds U+FEFF and leaves out U+0085).
 */
export const GROUP_TEXT_SPACE = /[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/gu;
/** Trims and collapses every run of `GROUP_TEXT_SPACE` to one space (Rust `normalize_group_text`). */
export function normalizeGroupText(raw: string): string {
  return raw.replace(GROUP_TEXT_SPACE, " ").replace(/^ | $/g, "");
}
/** Max code points of a group's purpose line (1.1). */
export const MAX_GROUP_PURPOSE_CHARS = 200;

/**
 * `PATCH /v1/groups/:id`: renames a group (not a DM) and/or sets its purpose line (1.1). The
 * server normalizes both with `normalizeGroupText`, then checks the name (1..64) and the purpose
 * (0..200; empty clears it). At least one of them. Response: `GroupSummary`.
 */
export const UpdateGroupRequest = z
  .object({ name: Name.optional(), purpose: z.string().max(4 * MAX_GROUP_PURPOSE_CHARS).optional() })
  .refine((r) => r.name !== undefined || r.purpose !== undefined, { message: "name or purpose is required" });
export type UpdateGroupRequest = z.infer<typeof UpdateGroupRequest>;

/** Why an event should notify the receiving member. */
export const NotifyReason = z.enum(["mention", "waiting", "result", "message"]);
export type NotifyReason = z.infer<typeof NotifyReason>;

/** Field added to the `event` server frame: present = show a notification. */
export const eventFrameFields = {
  notify: NotifyReason.optional(),
};
