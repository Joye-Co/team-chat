// 接 4.2 / 5.2: the run's report (`chat-run report`, output op `run.report`). The CLI says how its
// work ended: done, blocked (it needs something to go on) or waiting (for a person's answer). The
// latest report counts when the run ends; without one a run that ended normally is done, as
// before. The phases an ended run reads as follow design-takeover 3.1 (rule 1): `needs_reply`
// while a blocked or waiting run is the AI member's latest in its topic, else done, failed or
// stopped.
import { RunReport, type RunStatus } from "@chat/protocol";
import { ApiError } from "../../core/errors";

/** The latest report of a run, as stored in `runs.report`. */
export interface StoredReport extends RunReport {
  at: number;
}

/** The `run.report` body checked at the boundary; throws `invalid` for anything else. */
export function parseReport(body: unknown): RunReport {
  const parsed = RunReport.safeParse(body);
  if (!parsed.success) {
    const issue = parsed.error.issues[0];
    throw new ApiError("invalid", `invalid output: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim(), "report.invalid");
  }
  return parsed.data;
}

/** The stored report, or null (none, or a row that does not parse). */
export function reportOf(r: { report?: string | null }): StoredReport | null {
  if (!r.report) return null;
  try {
    const v = JSON.parse(r.report) as StoredReport;
    return RunReport.safeParse(v).success ? v : null;
  } catch {
    return null;
  }
}

/** A report that keeps the topic open for a person: blocked or waiting. */
export function needsReply(report: RunReport | null): report is RunReport & { state: "blocked" | "waiting" } {
  return report !== null && report.state !== "done";
}

/**
 * The phase of an ended run (design-takeover 3.1, rule 1) and what it waits for. `superseded`:
 * a later run of the same AI member started in the topic, so a waiting report was answered.
 */
export function endedPhase(status: RunStatus, report: RunReport | null, superseded: boolean): { phase: string; waitingFor?: string } {
  if (status === "cancelled") return { phase: "stopped" };
  // The report counts only for a run that ended normally; a failed run failed whatever it said.
  if (status === "done" && needsReply(report) && !superseded) return { phase: "needs_reply", waitingFor: report.state === "waiting" ? "report" : "stuck" };
  return { phase: status === "done" ? "done" : "failed" };
}
