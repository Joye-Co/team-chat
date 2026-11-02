// Alert state machine for one target. One alert when a target goes down (after `threshold`
// failed checks in a row), one when it comes back. Nothing in between: no flooding.

export type Status = "unknown" | "ok" | "failing";

export interface TargetState {
  status: Status;
  /** Failed checks in a row. */
  fails: number;
  /** When the current status began (first failed check for "failing"). */
  since: number;
  /** A down alert was raised and no recovery alert yet. */
  alerted: boolean;
  lastError: string | null;
}

export interface CheckResult {
  target: string;
  /** "backup" for the archive freshness check; absent for the service check. */
  check?: "backup";
  at: number;
  ok: boolean;
  healthMs: number | null;
  rttMs: number | null;
  /** Short reason, e.g. "health: 503" or "round trip: timeout". Never contains a token. */
  error: string | null;
}

export type Alert =
  | { kind: "down"; target: string; check?: "backup"; at: number; since: number; fails: number; error: string }
  | { kind: "up"; target: string; check?: "backup"; at: number; downSince: number; error: string | null };

export const INITIAL: TargetState = { status: "unknown", fails: 0, since: 0, alerted: false, lastError: null };

export function step(prev: TargetState, r: CheckResult, threshold: number): { next: TargetState; alert: Alert | null } {
  if (r.ok) {
    const next: TargetState = {
      status: "ok",
      fails: 0,
      since: prev.status === "ok" ? prev.since : r.at,
      alerted: false,
      lastError: null,
    };
    const alert: Alert | null = prev.alerted
      ? { kind: "up", target: r.target, ...checkOf(r), at: r.at, downSince: prev.since, error: prev.lastError }
      : null;
    return { next, alert };
  }
  const fails = prev.fails + 1;
  const since = prev.status === "failing" ? prev.since : r.at;
  const error = r.error ?? "unknown error";
  const raise = !prev.alerted && fails >= threshold;
  return {
    next: { status: "failing", fails, since, alerted: prev.alerted || raise, lastError: error },
    alert: raise ? { kind: "down", target: r.target, ...checkOf(r), at: r.at, since, fails, error } : null,
  };
}

function checkOf(r: CheckResult): { check?: "backup" } {
  return r.check ? { check: r.check } : {};
}

/** Alert state key: one state per target and check. */
export function stateKey(r: { target: string; check?: "backup" }): string {
  return r.check ? `${r.target}:${r.check}` : r.target;
}
