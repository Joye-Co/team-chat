// Pure rules of the settings feature: local time in the member's zone, quiet hours, the next
// digest time, notify filtering and approval policy updates. No I/O, so they are unit-tested.
import {
  type ApprovalPolicy,
  DEFAULT_APPROVAL_POLICY,
  DEFAULT_NOTIFY_SETTINGS,
  type NotifyEvent,
  type NotifySettings,
  type QuietHours,
  type UpdateApprovalPolicyRequest,
  type UpdateNotifySettingsRequest,
} from "@chat/protocol";

const DAY_MS = 86_400_000;
const formatters = new Map<string, Intl.DateTimeFormat>();

/** True when the runtime knows this IANA zone. */
export function isKnownZone(tz: string): boolean {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

function formatter(tz: string): Intl.DateTimeFormat {
  const zone = isKnownZone(tz) ? tz : "UTC";
  let f = formatters.get(zone);
  if (!f) {
    f = new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      hourCycle: "h23",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    formatters.set(zone, f);
  }
  return f;
}

/** The zone's offset from UTC at `at`, in ms (local wall clock = at + offset). */
export function zoneOffsetMs(at: number, tz: string): number {
  const parts = Object.fromEntries(formatter(tz).formatToParts(new Date(at)).map((p) => [p.type, p.value]));
  const wall = Date.UTC(Number(parts.year), Number(parts.month) - 1, Number(parts.day), Number(parts.hour) % 24, Number(parts.minute), Number(parts.second));
  return wall - Math.floor(at / 1000) * 1000;
}

/** "HH:MM" as minutes after midnight. */
export function clockMinutes(hhmm: string): number {
  return Number(hhmm.slice(0, 2)) * 60 + Number(hhmm.slice(3, 5));
}

/** Minutes after local midnight at `at`. */
export function localMinutes(at: number, tz: string): number {
  const wall = at + zoneOffsetMs(at, tz);
  return Math.floor((((wall % DAY_MS) + DAY_MS) % DAY_MS) / 60_000);
}

/** True inside quiet hours. A range that crosses midnight (22:00–08:00) works; start = end is empty. */
export function inQuietHours(at: number, q: QuietHours, tz: string): boolean {
  if (!q.enabled) return false;
  const s = clockMinutes(q.start);
  const e = clockMinutes(q.end);
  if (s === e) return false;
  const now = localMinutes(at, tz);
  return s < e ? now >= s && now < e : now >= s || now < e;
}

/** The first instant after `after` when the local clock in `tz` shows `hhmm`. */
export function nextLocalTime(after: number, hhmm: string, tz: string): number {
  const off = zoneOffsetMs(after, tz);
  const wall = after + off;
  const dayStart = Math.floor(wall / DAY_MS) * DAY_MS;
  let candidate = dayStart + clockMinutes(hhmm) * 60_000;
  if (candidate <= wall) candidate += DAY_MS;
  // Use the offset at the candidate itself, so a daylight saving change in between is honored.
  let at = candidate - zoneOffsetMs(candidate - off, tz);
  if (at <= after) at += DAY_MS;
  return at;
}

/** Stored settings (any older shape) with defaults filled in. */
export function withDefaults(stored: Partial<NotifySettings> | null | undefined): NotifySettings {
  const d = DEFAULT_NOTIFY_SETTINGS;
  if (!stored) return { ...d, events: { ...d.events }, quietHours: { ...d.quietHours } };
  const q = stored.quietHours;
  return {
    events: {
      waiting: stored.events?.waiting ?? d.events.waiting,
      mention: stored.events?.mention ?? d.events.mention,
      result: stored.events?.result ?? d.events.result,
      blocked: stored.events?.blocked ?? d.events.blocked,
    },
    digestAt: stored.digestAt === undefined ? d.digestAt : stored.digestAt,
    quietHours: { enabled: q?.enabled ?? d.quietHours.enabled, start: q?.start ?? d.quietHours.start, end: q?.end ?? d.quietHours.end },
    timeZone: stored.timeZone ?? d.timeZone,
    updatedAt: stored.updatedAt ?? 0,
  };
}

/** Applies a PATCH. */
export function applyNotifyUpdate(s: NotifySettings, u: UpdateNotifySettingsRequest, now: number): NotifySettings {
  return {
    events: { ...s.events, ...(u.events ?? {}) },
    digestAt: u.digestAt === undefined ? s.digestAt : u.digestAt,
    quietHours: { ...s.quietHours, ...(u.quietHours ?? {}) },
    timeZone: u.timeZone ?? s.timeZone,
    updatedAt: now,
  };
}

export const SWITCHABLE: ReadonlySet<string> = new Set<NotifyEvent>(["waiting", "mention", "result", "blocked"]);

/**
 * The `notify` decision under the member's settings. Returns whether the notification stays.
 * Off when the event's switch is off and inside quiet hours (勿扰). An event without
 * `notifyEvent` (a plain `message` notification) follows only quiet hours.
 */
export function keepNotify(s: NotifySettings, notifyEvent: string | undefined, at: number): boolean {
  if (notifyEvent && SWITCHABLE.has(notifyEvent) && !s.events[notifyEvent as NotifyEvent]) return false;
  return !inQuietHours(at, s.quietHours, s.timeZone);
}

// ---------------------------------------------------------------------------
// Approval policies
// ---------------------------------------------------------------------------

export function teamPolicy(stored: Partial<ApprovalPolicy> | null | undefined): ApprovalPolicy {
  const d = DEFAULT_APPROVAL_POLICY;
  if (!stored) return { ...d, askBefore: { ...d.askBefore } };
  const a = stored.askBefore;
  return {
    askBefore: {
      mergeMain: a?.mergeMain ?? d.askBefore.mergeMain,
      deployProd: a?.deployProd ?? d.askBefore.deployProd,
      prodConfig: a?.prodConfig ?? d.askBefore.prodConfig,
      deleteData: a?.deleteData ?? d.askBefore.deleteData,
      externalPublish: a?.externalPublish ?? d.askBefore.externalPublish,
    },
    maxSpendCny: stored.maxSpendCny === undefined ? d.maxSpendCny : stored.maxSpendCny,
    approver: stored.approver ?? d.approver,
    updatedAt: stored.updatedAt ?? 0,
  };
}

/** Applies a PATCH to the team policy. */
export function applyPolicyUpdate(p: ApprovalPolicy, u: UpdateApprovalPolicyRequest, now: number): ApprovalPolicy {
  return {
    askBefore: { ...p.askBefore, ...(u.askBefore ?? {}) },
    maxSpendCny: u.maxSpendCny === undefined ? p.maxSpendCny : u.maxSpendCny,
    approver: u.approver ?? p.approver,
    updatedAt: now,
  };
}

const ACTION_TEXT: Record<keyof ApprovalPolicy["askBefore"], string> = {
  mergeMain: "merge into the main branch",
  deployProd: "deploy or publish to production",
  prodConfig: "change production configuration or secrets",
  deleteData: "delete data or files outside your own scratch work",
  externalPublish: "publish anything outside the team (social media, email, public posts)",
};

/** The policy lines for a run prompt, or undefined when nothing needs approval. */
export function policyPrompt(p: ApprovalPolicy): string | undefined {
  const items = (Object.keys(ACTION_TEXT) as (keyof typeof ACTION_TEXT)[]).filter((k) => p.askBefore[k]).map((k) => ACTION_TEXT[k]);
  if (p.maxSpendCny !== null) items.push(`spend more than ¥${p.maxSpendCny} at once (paid APIs, cloud resources, purchases)`);
  if (items.length === 0) return undefined;
  const who = p.approver === "owners" ? "a team owner" : "the person who asked you";
  return [
    `Team rule: before you do any of these, stop, ask ${who} for approval in the topic and wait for the answer:`,
    ...items.map((x) => `- ${x}`),
  ].join("\n");
}
