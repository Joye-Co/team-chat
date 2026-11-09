// 成员 of 团队管理 (docs/protocol.md "Team roster"): `GET /v1/team/roster` answers TeamRoster, what
// the member list in the directory does not carry: when each member joined and who added them,
// when they were last seen, their signed-in devices, and invites not accepted yet.
import { z } from "zod";
import { Id, Millis } from "./common.ts";

export const RosterDevice = z.object({
  /** The device name given at sign-in, e.g. "Joye 的 MacBook Pro". */
  label: z.string().max(200),
  createdAt: Millis,
  lastUsedAt: Millis.optional(),
});
export type RosterDevice = z.infer<typeof RosterDevice>;

export const RosterMember = z.object({
  memberId: Id,
  joinedAt: Millis,
  /** Who invited or added them; absent for the first owner. */
  addedBy: Id.optional(),
  /** The newest use of any of their tokens. */
  lastSeenAt: Millis.optional(),
  devices: z.array(RosterDevice).max(50),
});
export type RosterMember = z.infer<typeof RosterMember>;

export const PendingInvite = z.object({
  id: Id,
  name: z.string().min(1).max(100),
  kind: z.enum(["human", "bot"]),
  level: z.enum(["owner", "member"]),
  role: z.string().max(100).optional(),
  createdBy: Id,
  createdAt: Millis,
  expiresAt: Millis,
});
export type PendingInvite = z.infer<typeof PendingInvite>;

export const TeamRoster = z.object({
  members: z.array(RosterMember).max(1000),
  invites: z.array(PendingInvite).max(200),
});
export type TeamRoster = z.infer<typeof TeamRoster>;
