// Usable (design-takeover 3.2, 3.3): who may pause, take over and hand back a run, and the
// timing of a hold. Pure helpers; BotDesk (takeover.ts, hand.ts) applies them.
import { MACHINE_FEATURE_TAKEOVER, MACHINE_FEATURE_TAKEOVER_ISOLATED } from "@chat/protocol";
import { ANONYMOUS_ID } from "../../cards2/privacy";
import type { MachineView } from "../dispatch";

/** Reminders to the person holding a run: after 2 hours (holder), after 24 hours (holder, requester, admins). */
export const HOLD_REMIND_MS = [2 * 3_600_000, 24 * 3_600_000] as const;
/** A person's command runs at most 10 minutes; without a result after this it no longer blocks the next. */
export const HAND_COMMAND_MAX_MS = 11 * 60_000;
/** A run that fails this soon after a hand-back goes on in its session instead (3.3). */
export const HANDBACK_RETRY_MS = 2 * 60_000;

/** A person acting on a run's hold. `level: "owner"` is an admin (管理员, the owner included). */
export interface HoldActor {
  id: string;
  kind: "human" | "bot";
  level: "owner" | "member";
}

/** The run's machine as the rules need it. Undefined: the machine is gone (read as a team machine). */
export type HoldMachine = Pick<MachineView, "ownerId" | "info"> | undefined;

function features(m: HoldMachine): string[] {
  return m?.info?.features ?? [];
}

/** `BotRun.takeover`: how a person can take over a run on this machine (3.6). */
export function takeoverOf(m: HoldMachine): "commands" | "isolated" | "none" {
  if (!features(m).includes(MACHINE_FEATURE_TAKEOVER)) return "none";
  return features(m).includes(MACHINE_FEATURE_TAKEOVER_ISOLATED) ? "isolated" : "commands";
}

/** The person the run works for (`requestedFor`, else `requestedBy`); null for a private card answer. */
export function requesterOf(r: { requested_by: string }): string | null {
  return r.requested_by && r.requested_by !== ANONYMOUS_ID ? r.requested_by : null;
}

/** On a personal machine its owner; on a team machine an admin (design-roles 3.6). */
function machineManager(a: HoldActor, m: HoldMachine): boolean {
  return m?.ownerId ? a.id === m.ownerId : a.level === "owner";
}

/** Pause (3.2): the requester; on a personal machine also its owner, on a team machine also an admin. */
export function mayPause(a: HoldActor, requester: string | null, m: HoldMachine): boolean {
  return a.kind === "human" && (a.id === requester || machineManager(a, m));
}

/**
 * Take over (3.2, "谁能接管"): never without feature `takeover`; on a personal machine only its
 * owner; on a team machine only the requester, and only where the machine reports
 * `takeover-isolated`. Admins do not take over the runs of others.
 */
export function mayTakeOver(a: HoldActor, requester: string | null, m: HoldMachine): boolean {
  if (a.kind !== "human" || m === undefined) return false;
  const how = takeoverOf(m);
  if (how === "none") return false;
  if (m.ownerId) return a.id === m.ownerId;
  return how === "isolated" && a.id === requester;
}

/**
 * Resume a pause or hand back a take-over (3.2): the person holding it. A pause also the
 * requester and, by the machine rule, its owner or an admin. A take-over on a team machine also
 * an admin, for the person holding it.
 */
export function mayResume(a: HoldActor, hold: { by: string; kind: "pause" | "takeover" }, requester: string | null, m: HoldMachine): boolean {
  if (a.kind !== "human") return false;
  if (a.id === hold.by) return true;
  if (hold.kind === "pause") return a.id === requester || machineManager(a, m);
  return !m?.ownerId && a.level === "owner";
}

/** Topic lines and reminders of a hold (people see these; product words only). */
export const HOLD_TEXT = {
  /** The person holding it left the team or the 群. */
  left: (who: string) => `${who} 已离开，自动交还`,
  /** The daemon restarted while a person held the run. */
  restarted: (computer: string) => `${computer} 重启过，接管已结束`,
  /** The CLI failed right after a hand-back and goes on from where it was. */
  retried: (member: string) => `${member} 交还后出错，已从上次的进度重新开始`,
  /** 2 hours. */
  remind: (member: string) => `${member} 还停着，等你交还`,
  /** 24 hours. */
  remindAll: (who: string, member: string, hours: number, kind: "pause" | "takeover") =>
    `${who} ${kind === "takeover" ? "接管" : "暂停"} ${member} 已经 ${hours} 小时`,
};

/** What the CLI reads when its hold ends without the person (English, like every prompt). */
export const HOLD_CLI_TEXT = {
  left: "[team-chat] The person who held your computer left the team; the hold ended automatically. Check the work folder before you continue.",
  restarted: "the computer restarted while a person held it",
  handback: "failed right after a person handed it back",
};
