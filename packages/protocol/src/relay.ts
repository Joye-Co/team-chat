// Usable: several AI members in one topic, chains of hand-overs (转交 / 分出去) and relays
// (按顺序做). See docs/protocol.md, "Participants, chains and relays"; design
// docs/plans/usable/design-relay.zh.md. Readers take every state and reason as an open string.
import { z } from "zod";
import { Count, Id, Millis } from "./common.ts";

/** Max AI members `Topic.participants` lists. */
export const MAX_TOPIC_PARTICIPANTS = 50;
/** A relay has 2..6 steps. */
export const MIN_RELAY_STEPS = 2;
export const MAX_RELAY_STEPS = 6;

/** One AI member that ran in a topic (`Topic.participants`). */
export const TopicParticipant = z.object({
  memberId: Id,
  /** It has a run in the topic that has not ended. */
  working: z.boolean(),
  /** When its latest run in the topic started. */
  lastRunAt: Millis,
});
export type TopicParticipant = z.infer<typeof TopicParticipant>;

/** Known `TopicRelay.state` values; an unknown value reads as stopped. */
export const RELAY_STATES = ["running", "paused", "done", "stopped"] as const;
/** Known `TopicRelayStep.state` values; an unknown value reads as pending. */
export const RELAY_STEP_STATES = ["pending", "queued", "working", "done", "failed", "stopped"] as const;
/** Known `TopicRelay.reason` values; an unknown value reads as "stopped" with no reason shown. */
export const RELAY_REASONS = [
  "member_unavailable",
  "step_failed",
  "step_blocked",
  "step_cancelled",
  "limit",
  "undelivered",
  "not_pushed",
  "requester_left",
] as const;

export const TopicRelayStep = z.object({
  memberId: Id,
  /** What this step is for, as the person wrote it (<= 100). */
  note: z.string().min(1).max(100).optional(),
  /** "pending" | "queued" | "working" | "done" | "failed" | "stopped" (`RELAY_STEP_STATES`). */
  state: z.string().min(1).max(32),
  /** The run of this step, once it started (the run it was handed over to after a 转交). */
  runId: Id.optional(),
});
export type TopicRelayStep = z.infer<typeof TopicRelayStep>;

/** A relay (按顺序做) a person started in a topic. */
export const TopicRelay = z.object({
  id: Id,
  /** The person who wrote the arrow message. */
  startedBy: Id,
  goal: z.string().min(1).max(1000),
  /** Index of the current step, 0-based. */
  current: z.int().nonnegative(),
  /** "running" | "paused" | "done" | "stopped" (`RELAY_STATES`). */
  state: z.string().min(1).max(32),
  /** Why it is paused or stopped (`RELAY_REASONS`). */
  reason: z.string().min(1).max(64).optional(),
  /** The person who stopped it. */
  stoppedBy: Id.optional(),
  updatedAt: Millis,
  steps: z.array(TopicRelayStep).min(MIN_RELAY_STEPS).max(MAX_RELAY_STEPS),
});
export type TopicRelay = z.infer<typeof TopicRelay>;

/** Fields added to `Topic` (usable). */
export const topicRelayFields = {
  /** AI members that ran in the topic; absent = none (or a server before it). */
  participants: z.array(TopicParticipant).max(MAX_TOPIC_PARTICIPANTS).optional(),
  /** The topic's latest relay; absent = none. */
  relay: TopicRelay.optional(),
};

/** Fields added to `topic.updated` data (usable). Present = changed to this value. */
export const topicUpdatedRelayFields = {
  participants: z.array(TopicParticipant).max(MAX_TOPIC_PARTICIPANTS).optional(),
  /** null = the topic has no relay any more. */
  relay: TopicRelay.nullable().optional(),
};

/** Where a run left its code (`run.done.code`, `BotRun.code`). */
export const RunCode = z.object({
  branch: z.string().min(1).max(256),
  /** The commit the branch points at (hex). */
  head: z.string().regex(/^[0-9a-f]{4,64}$/),
  /** Every commit of the branch is on its remote. */
  pushed: z.boolean(),
});
export type RunCode = z.infer<typeof RunCode>;

/** `POST /v1/groups/:id/chains/:chainId/stop` */
export const ChainStopResponse = z.object({ stoppedRuns: Count });
export type ChainStopResponse = z.infer<typeof ChainStopResponse>;
