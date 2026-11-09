// Pure rules of the settings feature: local time in the member's zone, quiet hours, the next
// digest time, notify filtering and approval policy updates. No I/O, so they are unit-tested.
import {
  type ApprovalApprovers,
  type ApprovalPatterns,
  type ApprovalPolicy,
  type Approver,
  DEFAULT_APPROVAL_POLICY,
  DEFAULT_GATE_PATTERNS,
  GATE_KINDS,
  type GateKind,
  type RunGate,
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
    ...perKind(stored.patterns, stored.approvers),
  };
}

/** `patterns` and `approvers` with only the known kinds that have a value; empty ones left out. */
function perKind(patterns: ApprovalPatterns | undefined, approvers: ApprovalApprovers | undefined): Pick<ApprovalPolicy, "patterns" | "approvers"> {
  const out: Pick<ApprovalPolicy, "patterns" | "approvers"> = {};
  const p: ApprovalPatterns = {};
  const a: ApprovalApprovers = {};
  for (const k of GATE_KINDS) {
    const list = patterns?.[k];
    if (Array.isArray(list) && list.length > 0) p[k] = list.filter((x) => typeof x === "string" && x !== "");
    const who = approvers?.[k];
    if (who === "owners" || who === "requester") a[k] = who;
  }
  if (Object.keys(p).length > 0) out.patterns = p;
  if (Object.keys(a).length > 0) out.approvers = a;
  return out;
}

/** Applies a PATCH to the team policy. Patterns of a kind given replace that kind's (`[]` clears). */
export function applyPolicyUpdate(p: ApprovalPolicy, u: UpdateApprovalPolicyRequest, now: number): ApprovalPolicy {
  const { patterns: _p, approvers: _a, ...rest } = p;
  return {
    ...rest,
    askBefore: { ...p.askBefore, ...(u.askBefore ?? {}) },
    maxSpendCny: u.maxSpendCny === undefined ? p.maxSpendCny : u.maxSpendCny,
    approver: u.approver ?? p.approver,
    updatedAt: now,
    ...perKind({ ...p.patterns, ...u.patterns }, { ...p.approvers, ...u.approvers }),
  };
}

/**
 * A team pattern the server takes: it compiles in JavaScript and stays in the subset Rust's
 * `regex` shares (no lookaround, no backreferences, no possessive or atomic groups). Returns
 * the reason when it does not.
 */
export function patternProblem(p: string): string | null {
  if (/\(\?<?[=!]|\(\?>|\\[1-9]|\\k<|[*+?}]\+/.test(p)) return "lookaround, backreferences and possessive forms are not supported";
  try {
    new RegExp(p, "i");
  } catch {
    return "not a valid regular expression";
  }
  return null;
}

/** Who approves `kind`: its own approver, else the team approver. */
export function approverOf(p: ApprovalPolicy, kind: string | null | undefined): Approver {
  return (kind && p.approvers?.[kind as GateKind]) || p.approver;
}

/**
 * The run's gates (machine protocol `run.assign.gates`): each checked kind with the default
 * patterns and the team's own, except kinds a person already approved for the whole piece of
 * work (这件事里都同意).
 */
export function runGates(p: ApprovalPolicy, approved: readonly string[] = []): RunGate[] {
  return GATE_KINDS.filter((k) => p.askBefore[k] && !approved.includes(k)).map((k) => ({
    kind: k,
    patterns: [...new Set([...DEFAULT_GATE_PATTERNS[k], ...(p.patterns?.[k] ?? [])])].slice(0, 64),
  }));
}

const ACTION_TEXT: Record<keyof ApprovalPolicy["askBefore"], string> = {
  mergeMain: "merge into the main branch",
  deployProd: "deploy or publish to production",
  prodConfig: "change production configuration or secrets",
  deleteData: "delete data or files outside your own scratch work",
  externalPublish: "publish anything outside the team (social media, email, public posts)",
};

/**
 * The policy lines for a run prompt, or undefined when nothing needs approval. With `gated` the
 * machine stops the matching commands itself and asks the approver (feature "gates"): the CLI
 * then asks in the topic only when it does such an action another way (an API call, a web
 * page) or spends money.
 */
export function policyPrompt(p: ApprovalPolicy, gated = false): string | undefined {
  const kinds = GATE_KINDS.filter((k) => p.askBefore[k]);
  const items = kinds.map((k) => ACTION_TEXT[k]);
  const spend = p.maxSpendCny !== null ? `spend more than ¥${p.maxSpendCny} at once (paid APIs, cloud resources, purchases)` : null;
  if (items.length === 0 && !spend) return undefined;
  const who = (k?: GateKind) => (approverOf(p, k) === "owners" ? "a team owner" : "the person who asked you");
  if (!gated) {
    return [
      `Team rule: before you do any of these, stop, ask ${who()} for approval in the topic and wait for the answer:`,
      ...[...kinds.map((k) => `${ACTION_TEXT[k]}${approverOf(p, k) !== p.approver ? ` (ask ${who(k)})` : ""}`), ...(spend ? [spend] : [])].map((x) => `- ${x}`),
    ].join("\n");
  }
  const lines: string[] = [];
  if (items.length > 0) {
    lines.push(
      "Team rule: these need a person's approval. When you run a command for one of them (for example `gh pr merge`, a production deploy), the machine asks the approver and the command waits for the answer; do not ask for it in the topic yourself. If a person declines, do not try again, do the rest another way:",
      ...items.map((x) => `- ${x}`),
      `When you do one of these another way (an API call, a web page), first ask ${who()} in the topic and wait for the answer.`,
    );
  }
  if (spend) lines.push(`${lines.length > 0 ? "Also" : "Team rule"}: before you ${spend}, ask ${who()} in the topic and wait for the answer.`);
  return lines.join("\n");
}
