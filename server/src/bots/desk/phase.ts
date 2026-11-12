// Usable (design-takeover 3.1): the one state people see for a run, from its row. A pure
// function, rules in fixed order; the first that matches wins. `status` and `pausedBy` keep
// their meaning; clients read `phase` (and work it out themselves from those two when it is
// missing or unknown).
import type { RunReport, RunStatus } from "@chat/protocol";
import { endedPhase } from "./report";

/** The run fields the phase depends on (a desk `runs` row). */
export interface PhaseRow {
  status: RunStatus;
  ended_at: number | null;
  paused_by: string | null;
  /** NULL on rows from before take-over: a pause. */
  hold_kind?: string | null;
  away_since: number | null;
  queued_at: number | null;
  /** A queued run waiting for its computer's subscription limit to reset (模 3.4); absent on old rows. */
  limit_until?: number | null;
  machine_id: string | null;
  approval_id: string | null;
}

/** What the phase of an ended run needs besides the row. */
export interface PhaseContext {
  /** The run's latest report (`chat-run report`), or null. */
  report?: RunReport | null;
  /** A later run of the same AI member started in the topic (a waiting report was answered). */
  superseded?: boolean;
}

export interface RunPhase {
  phase: string;
  waitingFor?: string;
}

/** True while a person holds the run (paused or taken over). */
export function isHeld(r: Pick<PhaseRow, "ended_at" | "paused_by">): boolean {
  return r.ended_at === null && r.paused_by !== null;
}

/** The hold of a held run: "takeover", else "pause" (old rows have no kind). */
export function holdKindOf(r: Pick<PhaseRow, "hold_kind">): "pause" | "takeover" {
  return r.hold_kind === "takeover" ? "takeover" : "pause";
}

/** design-takeover 3.1 `phaseOf`. */
export function phaseOf(r: PhaseRow, ctx: PhaseContext = {}): RunPhase {
  // 1. Ended: stopped, needs_reply (a waiting or blocked report, not answered), done or failed.
  if (r.ended_at !== null) return endedPhase(r.status, ctx.report ?? null, ctx.superseded ?? false);
  // 2.1 A person holds it. Its machine going away does not change that (only waitingFor).
  if (r.paused_by !== null) {
    const phase = holdKindOf(r) === "takeover" ? "taken_over" : "paused";
    return r.away_since !== null ? { phase, waitingFor: "offline" } : { phase };
  }
  // 2.2 Waits for a computer: in the queue, its computer went away, or it is being prepared.
  if (r.status === "created" && r.machine_id === null && r.queued_at !== null) {
    return { phase: "waiting_computer", waitingFor: r.limit_until !== null && r.limit_until !== undefined ? "limit" : "queue" };
  }
  if (r.away_since !== null) return { phase: "waiting_computer", waitingFor: "offline" };
  if (r.status === "created" && r.machine_id !== null) return { phase: "waiting_computer", waitingFor: "booting" };
  // 2.3 A step waits for approval.
  if (r.approval_id !== null) return { phase: "waiting_approval" };
  // 2.4 The CLI lives and waits for a person: a question card, or the desk found it stuck.
  if (r.status === "waiting") return { phase: "waiting_person", waitingFor: "question" };
  if (r.status === "blocked") return { phase: "waiting_person", waitingFor: "stuck" };
  // 2.5
  return { phase: "working" };
}

/** Phases a person may pause or take over from (design-takeover 3.2). */
export const HOLDABLE_PHASES = new Set(["working", "waiting_person", "waiting_approval"]);
