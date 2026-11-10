// 活动记录 of 团队管理 (docs/protocol.md "Activity"): `GET /v1/activity` answers ActivityResponse.
import { z } from "zod";
import { Id, Millis } from "./common.ts";

/** What an entry is about; the page filters by it. */
export const ACTIVITY_KINDS = ["member", "level", "invite", "key", "approval", "budget", "login", "machine", "run"] as const;
export const ActivityKind = z.enum(ACTIVITY_KINDS);
export type ActivityKind = z.infer<typeof ActivityKind>;

/** `?before=<id>&limit=&actor=<memberId>&kind=a,b&machine=<machineId>` */
export const ActivityQuery = z.object({
  before: z.coerce.number().int().positive().optional(),
  limit: z.coerce.number().int().min(1).max(200).default(100),
  actor: Id.optional(),
  kind: z
    .string()
    .transform((s) => s.split(",").filter(Boolean))
    .pipe(z.array(ActivityKind).min(1).max(ACTIVITY_KINDS.length))
    .optional(),
  machine: Id.optional(),
});

export const ActivityEntry = z.object({
  id: z.int().positive(),
  at: Millis,
  /** One of ACTIVITY_KINDS; clients show an unknown kind as "member". */
  kind: z.string().min(1).max(32),
  /** The audit action, e.g. "member.update", "machine.secret.set". */
  action: z.string().min(1).max(64),
  actorId: Id.optional(),
  /** The member, machine, invite or run the action was on. */
  target: z.string().max(200).optional(),
  machineId: Id.optional(),
  /** The audit details: names, levels, budgets, never a secret. */
  details: z.record(z.string(), z.unknown()).optional(),
});
export type ActivityEntry = z.infer<typeof ActivityEntry>;

export const ActivityResponse = z.object({ entries: z.array(ActivityEntry).max(200), hasMore: z.boolean() });
export type ActivityResponse = z.infer<typeof ActivityResponse>;
