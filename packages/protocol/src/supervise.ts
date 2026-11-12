// Usable: what the desk's supervision (盯活) did to a run, and the team's time limit for one run.
// See docs/protocol.md, "Supervision (盯活)" and "Run time limit"; audit rows 盯 1.6, 1.8, 5.5.
// Readers take `action` and `reason` as open strings.
import { z } from "zod";
import { Millis } from "./common.ts";

/**
 * Known `SuperviseAct.action` values: a nudge to a silent run (`nudge`), a reminder to a run that
 * repeats a step (`loop`), a continuation of a run that only promised work (`continue`), an
 * interrupt and retry in the same session (`retry`), a move to another computer (`move`), a wait
 * for a computer that went away (`wait_machine`) and its return (`machine_back`), the time limit
 * (`limit`: told to save and report; `limit_continue`: a new turn in the same session;
 * `limit_stop`: stopped after turns without progress), and the end of the desk's tries
 * (`gave_up`).
 */
export const SUPERVISE_ACTIONS = [
  "nudge",
  "loop",
  "continue",
  "retry",
  "move",
  "wait_machine",
  "machine_back",
  "limit",
  "limit_continue",
  "limit_stop",
  "gave_up",
] as const;
/**
 * Known `SuperviseAct.reason` values: no event for a while (`silent`), the same step with the same
 * output (`same_step`), an answer that only promised work (`promise`), a computer that stopped
 * sending heartbeats (`machine_away`), was removed (`machine_removed`) or no longer has the run
 * (`machine_lost`), the daemon restarted (`daemon_restarted`), the time limit (`time_limit`), and
 * turns at the time limit without progress (`no_progress`).
 */
export const SUPERVISE_REASONS = [
  "silent",
  "same_step",
  "promise",
  "machine_away",
  "machine_removed",
  "machine_lost",
  "daemon_restarted",
  "time_limit",
  "no_progress",
] as const;
/** At most this many acts are kept per run (the latest). */
export const MAX_SUPERVISE_ACTS = 20;

/** One act of the desk's supervision on a run, with its reason. Server-made. */
export const SuperviseAct = z.object({
  at: Millis,
  /** `SUPERVISE_ACTIONS` or a newer value. */
  action: z.string().min(1).max(32),
  /** `SUPERVISE_REASONS` or a newer value. */
  reason: z.string().min(1).max(32),
  /** How long the reason lasted, in whole minutes (silence, time away, time worked), when it has one. */
  minutes: z.int().nonnegative().optional(),
});
export type SuperviseAct = z.infer<typeof SuperviseAct>;

/** Fields added to `BotRun` (usable). Absent on older runs and servers. */
export const runSuperviseFields = {
  /** What the desk did to this run by itself, oldest first (the latest `MAX_SUPERVISE_ACTS`). */
  supervision: z.array(SuperviseAct).max(MAX_SUPERVISE_ACTS).optional(),
};

/** Bounds of `RunSettings.limitMinutes`. */
export const RUN_LIMIT_MIN_MINUTES = 30;
export const RUN_LIMIT_MAX_MINUTES = 24 * 60;
export const DEFAULT_RUN_LIMIT_MINUTES = 120;
export const RunLimitMinutes = z.int().min(RUN_LIMIT_MIN_MINUTES).max(RUN_LIMIT_MAX_MINUTES);

/**
 * `GET /v1/settings/runs` (member): how long an AI member works on one run before it saves its
 * state, reports and goes on in a new turn of the same session. Time spent waiting for an
 * approval, a pause or a computer that went away does not count.
 */
export const RunSettings = z.object({
  limitMinutes: RunLimitMinutes,
  /** 0 = never changed (the default). */
  updatedAt: Millis,
});
export type RunSettings = z.infer<typeof RunSettings>;

/** `PATCH /v1/settings/runs` (admin): only the given fields change. Response: `RunSettings`. */
export const UpdateRunSettingsRequest = z.object({
  limitMinutes: RunLimitMinutes.optional(),
});
export type UpdateRunSettingsRequest = z.infer<typeof UpdateRunSettingsRequest>;
