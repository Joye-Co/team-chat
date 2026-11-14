// MachineLink: one per machine. Holds the daemon's WebSocket (hibernatable, at most one live
// link), the runs assigned to the machine and their leases, and forwards run frames to the
// bot's BotDesk before it acks them. It never runs anything itself. docs/machine-protocol.md.
import { OFFLINE_GRACE_MS, recordActivity } from "../activity/events";
import { DurableObject } from "cloudflare:workers";
import {
  CliInfo,
  Health,
  type MachineMetrics,
  type MonitorRange,
  type MonitorSeries,
  MACHINE_PROTOCOL_VERSION,
  MachineInfo,
  type RunAssignFrame,
  RunCursor,
  RunDoneFrame,
  RunFailure,
  RunId,
  RunSessionFrame,
} from "@chat/protocol";
import { z } from "zod";
import { logger as baseLogger, type Logger } from "../core/logger";
import { runMigrations } from "../core/migrations";
import { LOGINS_MIGRATION, METRICS_MIGRATION, latestMetrics, readSeries, recordMetrics } from "../monitor/store";
import { type MachineLogin, LoginStatusFrame } from "@chat/protocol";
import {
  ALERT_OFFLINE_MS,
  type AlertNote,
  type GithubState,
  type MachineAlert,
  githubAlerts,
  heartbeatAlerts,
  notifyAlert,
  withdrawAlert,
} from "../monitor/alerts";
import { ASSIGN_RETRY_MS, LIMIT_SLACK_MS, LINK_IDLE_MS, cliName } from "./dispatch";
import { type BotsEnv, deskStub } from "./env";

export const MACHINE_HEADER = "x-chat-machine-id";

/** True when a version like "1.1.0" or "1.2.0-beta.1" is at least `min` (major, minor, patch). */
export function versionAtLeast(version: string, min: [number, number, number]): boolean {
  const m = /^v?(\d+)\.(\d+)\.(\d+)/.exec(version.trim());
  if (!m) return false;
  const v = [Number(m[1]), Number(m[2]), Number(m[3])];
  for (let i = 0; i < 3; i++) if (v[i] !== min[i]) return v[i] > min[i];
  return true;
}

/** How often the alarm checks leases, assigns and the link while anything is open. */
const TICK_MS = 30_000;
/** A finished run waits this long for its run.session before it is closed. */
const SESSION_WAIT_MS = 60 * 60_000;
const ACK_EVERY = 100;
/** 盯 1.4: a working step is passed on to its desk at most this often per run. */
const ALIVE_EVERY_MS = 60_000;
const MAX_FRAME_BYTES = 1_048_576;

// Daemon frames as the server reads them. Run events are kept as objects with a string `type`
// so a newer daemon's event types are stored, not rejected (machine-protocol.md: unknown
// fields and types are ignored on read, and a rejected frame would be resent forever).
const LooseEvent = z.looseObject({ type: z.string().min(1).max(64) });
/**
 * design-github-identity 3.6: the heartbeat's `metrics.github`, read apart from the other metrics.
 * A state this server does not know reads as "unknown".
 */
const GITHUB_STATES = ["signed_in", "signed_out", "no_gh", "unknown"] as const;
const HeartbeatGithub = z.object({
  state: z
    .string()
    .min(1)
    .max(32)
    .transform((s): (typeof GITHUB_STATES)[number] => ((GITHUB_STATES as readonly string[]).includes(s) ? (s as (typeof GITHUB_STATES)[number]) : "unknown")),
  login: z.string().min(1).max(100).optional(),
});

/** The heartbeat's GitHub sign-in, or undefined when it has none the server can read. */
export function heartbeatGithub(metrics: unknown): GithubState | undefined {
  if (typeof metrics !== "object" || metrics === null) return undefined;
  const r = HeartbeatGithub.safeParse((metrics as { github?: unknown }).github);
  if (!r.success) return undefined;
  return r.data.login !== undefined ? { state: r.data.state, login: r.data.login } : { state: r.data.state };
}
const UpFrame = z.discriminatedUnion("t", [
  z.object({
    t: z.literal("hello"),
    protocol: z.number(),
    machine: MachineInfo,
    clis: z.array(CliInfo).max(64),
    health: Health,
    runs: z.array(RunCursor).max(1000),
  }),
  // `metrics` is checked apart (monitor/store.ts): metrics a server cannot read never fail the heartbeat.
  z.object({ t: z.literal("heartbeat"), at: z.number(), health: Health, runs: z.array(RunCursor).max(1000), metrics: z.unknown().optional() }),
  z.object({ t: z.literal("run.accept"), runId: RunId }),
  z.object({ t: z.literal("run.reject"), runId: RunId, reason: z.string().max(64), message: z.string().max(4096) }),
  z.object({ t: z.literal("run.event"), runId: RunId, seq: z.int().positive(), at: z.number(), event: LooseEvent }),
  // A failure this server cannot read is left out, never a reason to refuse the frame.
  RunDoneFrame.extend({ status: z.string().max(32), failure: RunFailure.optional().catch(undefined) }),
  RunSessionFrame.extend({ source: z.string().max(32).optional() }),
  // Feature "accounts": where a sign-in the owner started stands.
  LoginStatusFrame,
]);
type UpFrame = z.infer<typeof UpFrame>;
/**
 * A run frame with a seq, as stored by BotDesk. `serverAt` (set here, never by the daemon) is
 * when the daemon made a RunEvent `output`, on the server clock: the frame's machine-clock `at`
 * moved by the offset the last heartbeat showed, and never later than its arrival.
 */
export type SeqFrame = Extract<UpFrame, { seq: number }> & { serverAt?: number };

type RunState = "assigned" | "accepted" | "done" | "closed" | "lost";
type RunRow = {
  run_id: string;
  bot_id: string;
  assign: string;
  state: RunState;
  acked_seq: number;
  lease_ms: number;
  lease_until: number;
  sent_at: number | null;
  updated_at: number;
  /** 1 while a person holds the run (paused or taken over): its lease does not run out. */
  held: number;
};

interface Attachment {
  machineId: string;
  ready: boolean;
}

export interface LinkStatus {
  machineId: string | null;
  connected: boolean;
  revoked: boolean;
  lastHeartbeatAt?: number;
  info?: z.infer<typeof MachineInfo>;
  health?: z.infer<typeof Health>;
  clis?: z.infer<typeof CliInfo>[];
  runningRuns: string[];
  /** 模 3.1: runtimes whose CLI is not signed in (`signedOutRuntimes`). */
  signedOut?: string[];
  /** 模 3.4: runtimes at their subscription limit until `until` (`limitedRuntimes`). */
  limited?: LimitedRuntime[];
  /** design-github-identity 3.6: the GitHub sign-in of the latest heartbeat. */
  github?: GithubState;
  /** How many runs it may do at once as set in the app (`machines.parallel_runs`); absent = the daemon's own. */
  parallelRuns?: number;
}

/** How long a run that failed to sign in keeps its runtime signed out without a heartbeat saying so. */
export const SIGNED_OUT_MARK_MS = 15 * 60_000;

/**
 * 模 3.1, 机 2.5: the runtimes that take no runs: the latest heartbeat says a confirmed
 * `logged_out` (the CLI's own check, or a run that just failed to sign in; the credential files
 * alone do not know every way a CLI signs in), or a run failed with `signed_out` (`marks`:
 * runtime -> when) and no later heartbeat says `logged_in`. A mark older than
 * `SIGNED_OUT_MARK_MS` leaves it to the heartbeat.
 */
export function signedOutRuntimes(
  logins: { runtime: string; state: string; confirmed?: boolean }[] | undefined,
  metricsAt: number | null,
  marks: Record<string, number>,
  now: number,
): string[] {
  const state = new Map((logins ?? []).map((l) => [l.runtime, l.state]));
  const out = new Set<string>();
  for (const l of logins ?? []) if (l.state === "logged_out" && l.confirmed === true) out.add(l.runtime);
  for (const [rt, at] of Object.entries(marks)) {
    if (now - at > SIGNED_OUT_MARK_MS) continue;
    if (metricsAt !== null && metricsAt > at && state.get(rt) === "logged_in") continue;
    out.add(rt);
  }
  return [...out].sort();
}

/**
 * 模 3.4, 盯 1.5 (decided: wait for the reset): a runtime whose run ended at its subscription
 * limit takes no runs on this machine until `until` (server clock). The sign-in is the
 * machine's, shared by every AI member there. `known`: the CLI or the daemon's usage windows
 * said when it resets; else the desk tries again after a backoff (`tries` in a row).
 */
export interface LimitMark {
  until: number;
  known: boolean;
  tries: number;
}
export interface LimitedRuntime {
  runtime: string;
  until: number;
  known: boolean;
}
/** Without a reset time: 15 min, then twice as long each time in a row, at most 2 h. */
export const LIMIT_BACKOFF_MS = 15 * 60_000;
export const LIMIT_BACKOFF_MAX_MS = 2 * 60 * 60_000;
/** A usage window this full is the one that stopped the run. */
export const LIMIT_FULL_PCT = 95;
/** A reset further away than this is not believed (a week's window and a day of slack). */
const LIMIT_MAX_MS = 8 * 24 * 60 * 60_000;

/**
 * When a runtime that just hit its limit takes runs again: the CLI's reset time, else the end of
 * the fullest usage windows the daemon read (`windows` on the machine clock, `offset` puts them
 * on the server's), else a backoff that doubles while the earlier mark was a guess too.
 */
export function limitMark(
  resetsAt: number | undefined,
  windows: { usedPct: number; resetsAt?: number }[] | undefined,
  offset: number,
  prev: LimitMark | undefined,
  now: number,
): LimitMark {
  const ok = (t: number | undefined): t is number => t !== undefined && Number.isFinite(t) && t > now && t - now <= LIMIT_MAX_MS;
  if (ok(resetsAt)) return { until: resetsAt + LIMIT_SLACK_MS, known: true, tries: 0 };
  const full = (windows ?? []).filter((w) => w.usedPct >= LIMIT_FULL_PCT).map((w) => (w.resetsAt === undefined ? undefined : w.resetsAt + offset)).filter(ok);
  if (full.length > 0) return { until: Math.max(...full) + LIMIT_SLACK_MS, known: true, tries: 0 };
  // A guess in a row (the earlier one ended within a backoff of now): twice as long.
  const tries = prev && !prev.known && now - prev.until < LIMIT_BACKOFF_MAX_MS ? prev.tries + 1 : 1;
  return { until: now + Math.min(LIMIT_BACKOFF_MAX_MS, LIMIT_BACKOFF_MS * 2 ** (tries - 1)), known: false, tries };
}

/** The runtimes still at their limit (marks whose time has not come), by runtime. */
export function limitedRuntimes(marks: Record<string, LimitMark>, now: number): LimitedRuntime[] {
  return Object.entries(marks)
    .filter(([, m]) => m.until > now)
    .map(([runtime, m]) => ({ runtime, until: m.until, known: m.known }))
    .sort((a, b) => a.runtime.localeCompare(b.runtime));
}

const OPEN_STATES = "('assigned','accepted','done')";

export class MachineLink extends DurableObject<BotsEnv> {
  private readonly sql: SqlStorage;
  private machineId: string | null = null;
  /** 盯 1.4: when each run's desk last heard that its step is working (in memory only). */
  private readonly aliveSent = new Map<string, number>();
  private log: Logger = baseLogger.child({ obj: "MachineLink" });
  private chain: Promise<void> = Promise.resolve();
  private readonly queues = new Map<string, { frames: SeqFrame[]; busy: boolean; sinceAck: number }>();

  constructor(ctx: DurableObjectState, env: BotsEnv) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      runMigrations(ctx.storage, [
        {
          name: "machine-1",
          up: `
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE runs (
              run_id TEXT PRIMARY KEY,
              bot_id TEXT NOT NULL,
              assign TEXT NOT NULL,
              state TEXT NOT NULL,
              acked_seq INTEGER NOT NULL DEFAULT 0,
              lease_ms INTEGER NOT NULL,
              lease_until INTEGER NOT NULL,
              sent_at INTEGER,
              updated_at INTEGER NOT NULL
            );
            CREATE INDEX runs_state ON runs(state);
            CREATE TABLE outbox (id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL, frame TEXT NOT NULL);
          `,
        },
        METRICS_MIGRATION,
        LOGINS_MIGRATION,
        {
          // Usable (design-takeover 3.7): a run a person holds keeps its lease while its machine is away.
          name: "link-hold-1",
          up: `ALTER TABLE runs ADD COLUMN held INTEGER NOT NULL DEFAULT 0;`,
        },
      ]);
      this.machineId = this.meta("machine_id");
      if (this.machineId) this.log = baseLogger.child({ obj: "MachineLink", machineId: this.machineId });
    });
  }

  // -------------------------------------------------------------------------
  // State helpers
  // -------------------------------------------------------------------------

  private meta(key: string): string | null {
    return this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = ?", key).toArray()[0]?.value ?? null;
  }

  /** A machine-clock time on the server clock, at most `now`; `now` before the first heartbeat. */
  private serverTime(machineAt: number, now: number): number {
    const offset = Number(this.meta("clock_offset"));
    if (!this.meta("clock_offset") || !Number.isFinite(offset)) return now;
    return Math.min(now, machineAt + offset);
  }

  private setMeta(key: string, value: string): void {
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", key, value);
  }

  private bind(machineId: string): void {
    if (this.machineId === machineId) return;
    if (this.machineId !== null) throw new Error("link bound to another machine");
    this.setMeta("machine_id", machineId);
    this.machineId = machineId;
    this.log = baseLogger.child({ obj: "MachineLink", machineId });
  }

  /** The assign frame as sent: with the session upload URL on this server's origin. */
  private assignOut(r: RunRow): RunAssignFrame {
    const frame = JSON.parse(r.assign) as RunAssignFrame;
    const origin = this.meta("origin");
    if (!frame.sessionUpload && origin) frame.sessionUpload = { url: `${origin}/v1/machine/runs/${r.run_id}/session` };
    return frame;
  }

  private run(runId: string): RunRow | undefined {
    return this.sql.exec<RunRow>("SELECT * FROM runs WHERE run_id = ?", runId).toArray()[0] as RunRow | undefined;
  }

  /**
   * 盯 1.4: runs whose step the daemon reports working (`RunCursor.active`); their desks hear it
   * at most once a minute per run. Never holds up the heartbeat.
   */
  private alive(runIds: string[], now: number): void {
    for (const id of runIds) {
      if (now - (this.aliveSent.get(id) ?? 0) < ALIVE_EVERY_MS) continue;
      const r = this.run(id);
      if (!r || !this.machineId || (r.state !== "assigned" && r.state !== "accepted")) continue;
      this.aliveSent.set(id, now);
      this.ctx.waitUntil(
        deskStub(this.env, r.bot_id)
          .runAlive(id, this.machineId)
          .catch((err: unknown) => this.log.warn("run alive failed", { runId: id, err })),
      );
    }
    for (const [id, at] of this.aliveSent) if (now - at > 10 * ALIVE_EVERY_MS) this.aliveSent.delete(id);
  }

  private openRuns(): RunRow[] {
    return this.sql.exec<RunRow>(`SELECT * FROM runs WHERE state IN ${OPEN_STATES}`).toArray() as RunRow[];
  }

  private socket(): WebSocket | null {
    for (const ws of this.ctx.getWebSockets()) {
      if (ws.readyState !== WebSocket.OPEN) continue;
      if ((ws.deserializeAttachment() as Attachment | null)?.ready) return ws;
    }
    return null;
  }

  private send(ws: WebSocket, frame: unknown): void {
    try {
      ws.send(JSON.stringify(frame));
    } catch {
      // Closed meanwhile.
    }
  }

  private renew(runIds: string[], now: number): void {
    for (const id of runIds) {
      this.sql.exec(
        `UPDATE runs SET lease_until = max(lease_until, ? + lease_ms), updated_at = ? WHERE run_id = ? AND state IN ('assigned','accepted')`,
        now,
        now,
        id,
      );
    }
  }

  private async ensureAlarm(at = Date.now() + TICK_MS): Promise<void> {
    const cur = await this.ctx.storage.getAlarm();
    if (cur === null || cur > at) await this.ctx.storage.setAlarm(at);
  }

  // -------------------------------------------------------------------------
  // WebSocket
  // -------------------------------------------------------------------------

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") return new Response("expected websocket", { status: 426 });
    const machineId = request.headers.get(MACHINE_HEADER);
    if (!machineId) return new Response("unauthorized", { status: 401 });
    this.bind(machineId);
    this.setMeta("origin", new URL(request.url).origin);
    // One live link per machine: the new link replaces the old one.
    for (const old of this.ctx.getWebSockets()) {
      try {
        old.close(4002, "replaced by a new link");
      } catch {
        // Already closed.
      }
    }
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair) as [WebSocket, WebSocket];
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ machineId, ready: false } satisfies Attachment);
    this.setMeta("last_frame_at", String(Date.now()));
    await this.ensureAlarm();
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== "string" || message.length > MAX_FRAME_BYTES) {
      return this.send(ws, { t: "error", error: { code: "invalid", message: "text frames of at most 1 MiB only" } });
    }
    let raw: unknown;
    try {
      raw = JSON.parse(message);
    } catch {
      return this.send(ws, { t: "error", error: { code: "invalid", message: "frame is not valid JSON" } });
    }
    const parsed = UpFrame.safeParse(raw);
    if (!parsed.success) {
      const t = (raw as { t?: unknown })?.t;
      this.log.warn("bad daemon frame", { t: typeof t === "string" ? t.slice(0, 32) : null, issue: parsed.error.issues[0]?.message });
      return this.send(ws, { t: "error", error: { code: "invalid", message: "invalid frame" } });
    }
    // Frames of one link are handled one at a time, in order.
    const next = this.chain.then(() => this.handle(ws, parsed.data));
    this.chain = next.catch((err) => this.log.error("daemon frame failed", { err }));
    return next.catch(() => {});
  }

  async webSocketClose(ws: WebSocket, code: number): Promise<void> {
    try {
      ws.close(code === 1005 || code === 1006 ? 1000 : code, "closed");
    } catch {
      // Already closed.
    }
    await this.wentOffline(Date.now());
  }

  // -------------------------------------------------------------------------
  // 活动记录: offline and back online, debounced (activity/events.ts)
  // -------------------------------------------------------------------------

  /** The link closed: the alarm records 离线 if the machine is still gone after the grace. */
  private async wentOffline(now: number): Promise<void> {
    if (this.socket() !== null || this.meta("revoked") || this.meta("offline_since")) return;
    this.setMeta("offline_since", String(now));
    await this.ensureAlarm(now + OFFLINE_GRACE_MS);
  }

  /** A hello: 上线 again, recorded only when its 离线 was. A short reconnect records nothing. */
  private backOnline(now: number): void {
    const since = Number(this.meta("offline_since") ?? 0);
    const logged = this.meta("offline_logged") !== null;
    this.sql.exec("DELETE FROM meta WHERE key IN ('offline_since', 'offline_logged')");
    this.clearAlert("offline");
    if (!since || !logged || !this.machineId) return;
    this.ctx.waitUntil(
      recordActivity(this.env.DB, {
        at: now,
        kind: "machine",
        action: "machine.online",
        target: this.machineId,
        machineId: this.machineId,
        details: { offlineMs: now - since },
      }).catch((err) => this.log.warn("activity online failed", { err })),
    );
  }

  /** From the alarm: 离线 once the machine stayed away for the grace. */
  private async offlineCheck(now: number): Promise<void> {
    const since = Number(this.meta("offline_since") ?? 0);
    if (!since || this.socket() !== null || !this.machineId) return;
    if (this.meta("offline_logged") === null) {
      if (now - since < OFFLINE_GRACE_MS) {
        await this.ensureAlarm(since + OFFLINE_GRACE_MS);
        return;
      }
      this.setMeta("offline_logged", String(now));
      this.heldMachine(false);
      await recordActivity(this.env.DB, {
        at: since,
        kind: "machine",
        action: "machine.offline",
        target: this.machineId,
        machineId: this.machineId,
      }).catch((err) => this.log.warn("activity offline failed", { err }));
    }
    // 机 5.1: still away after 10 minutes, the managers hear of it (when an AI member depends on it).
    if (now - since < ALERT_OFFLINE_MS) {
      await this.ensureAlarm(since + ALERT_OFFLINE_MS);
      return;
    }
    this.raiseAlert({ key: "offline", kind: "offline", since }, now);
  }

  // -------------------------------------------------------------------------
  // 机 5.1: alerts to the computer's managers (monitor/alerts.ts)
  // -------------------------------------------------------------------------

  private upAlerts(): Record<string, MachineAlert & { notes?: AlertNote[] }> {
    try {
      return JSON.parse(this.meta("alerts") ?? "{}") as Record<string, MachineAlert & { notes?: AlertNote[] }>;
    } catch {
      return {};
    }
  }

  private saveAlerts(all: Record<string, MachineAlert & { notes?: AlertNote[] }>): void {
    if (Object.keys(all).length === 0) this.sql.exec("DELETE FROM meta WHERE key = 'alerts'");
    else this.setMeta("alerts", JSON.stringify(all));
  }

  /** Starts an alert once: the notices go out in the background and are kept to withdraw later. */
  private raiseAlert(alert: MachineAlert, now: number): void {
    const all = this.upAlerts();
    if (all[alert.key] || !this.machineId || this.meta("revoked")) return;
    all[alert.key] = alert;
    this.saveAlerts(all);
    const machineId = this.machineId;
    this.ctx.waitUntil(
      notifyAlert(this.env, machineId, alert, now, (p) => this.ctx.waitUntil(p))
        .then(async (notes) => {
          const cur = this.upAlerts();
          const still = cur[alert.key];
          if (still && still.since === alert.since) {
            cur[alert.key] = { ...still, notes };
            this.saveAlerts(cur);
          } else if (notes.length > 0) {
            // It ended while the notices went out.
            await withdrawAlert(this.env, notes);
          }
        })
        .catch((err: unknown) => this.log.warn("machine alert failed", { kind: alert.kind, err })),
    );
  }

  /** Ends an alert: its notices say it recovered. */
  private clearAlert(key: string): void {
    const all = this.upAlerts();
    const a = all[key];
    if (!a) return;
    delete all[key];
    this.saveAlerts(all);
    if (a.notes && a.notes.length > 0) {
      this.ctx.waitUntil(withdrawAlert(this.env, a.notes).catch((err: unknown) => this.log.warn("machine alert withdraw failed", { err })));
    }
  }

  /** A heartbeat: free disk and the CLIs' sign-ins start or end alerts. */
  private heartbeatAlerts(health: z.infer<typeof Health>, metrics: MachineMetrics | null, now: number): void {
    let seen: Record<string, string> = {};
    try {
      seen = JSON.parse(this.meta("login_seen") ?? "{}") as Record<string, string>;
    } catch {
      // Starts over.
    }
    const clis = (() => {
      try {
        return JSON.parse(this.meta("clis") ?? "[]") as { runtime: string; kind?: string }[];
      } catch {
        return [];
      }
    })();
    const r = heartbeatAlerts({
      up: this.upAlerts(),
      health,
      ...(metrics?.logins ? { logins: metrics.logins } : {}),
      seen,
      confirmsSignIn: this.hasFeature("sign-in-confirmed"),
      cliName: (runtime) => cliName(clis.find((c) => c.runtime === runtime)?.kind, runtime),
      now,
    });
    this.setMeta("login_seen", JSON.stringify(r.seen));
    for (const key of r.clear) this.clearAlert(key);
    for (const a of r.raise) this.raiseAlert(a, now);
  }

  /** design-github-identity 3.6: the heartbeat's GitHub sign-in is kept, and starts or ends its alert. */
  private heartbeatGithub(raw: unknown, now: number): void {
    const github = heartbeatGithub(raw);
    if (github) this.setMeta("github", JSON.stringify(github));
    else this.sql.exec("DELETE FROM meta WHERE key = 'github'");
    const r = githubAlerts({ up: this.upAlerts(), runsIn: this.runsIn(), github, seen: this.meta("github_seen") ?? undefined, now });
    if (r.seen !== undefined) this.setMeta("github_seen", r.seen);
    for (const key of r.clear) this.clearAlert(key);
    for (const a of r.raise) this.raiseAlert(a, now);
  }

  private runsIn(): string | undefined {
    try {
      return (JSON.parse(this.meta("info") ?? "{}") as { runsIn?: string }).runsIn;
    } catch {
      return undefined;
    }
  }

  /**
   * design-github-identity 3.6: an AI member's run on this computer found no GitHub sign-in. A
   * team computer that runs tasks in VMs tells its admins once (also when no heartbeat ever showed
   * a sign-in); the alert ends with a heartbeat that shows one. True when the alert started.
   */
  async githubMissing(reason: string): Promise<boolean> {
    if (!this.machineId || this.runsIn() !== "vm" || this.upAlerts().github) return false;
    const row = await this.env.DB.prepare("SELECT owner_id FROM machines WHERE id = ? AND revoked_at IS NULL")
      .bind(this.machineId)
      .first<{ owner_id: string | null }>();
    if (!row || row.owner_id !== null || this.upAlerts().github) return false;
    const now = Date.now();
    this.raiseAlert({ key: "github", kind: "github", since: now, reason: reason.slice(0, 64) }, now);
    return this.upAlerts().github !== undefined;
  }

  async webSocketError(): Promise<void> {}

  private async handle(ws: WebSocket, f: UpFrame): Promise<void> {
    const att = ws.deserializeAttachment() as Attachment | null;
    if (!att || ws.readyState !== WebSocket.OPEN) return;
    const now = Date.now();
    this.setMeta("last_frame_at", String(now));
    if (this.meta("revoked")) {
      this.send(ws, { t: "error", error: { code: "unauthorized", message: "machine token revoked" } });
      ws.close(4001, "unauthorized");
      return;
    }
    if (f.t === "hello") return this.hello(ws, att, f, now);
    if (!att.ready) {
      this.send(ws, { t: "error", error: { code: "invalid", message: "send hello first" } });
      return;
    }
    switch (f.t) {
      case "heartbeat":
        // The machine clock against the server's (a heartbeat is sent when it is made).
        this.setMeta("clock_offset", String(now - f.at));
        this.setMeta("health", JSON.stringify(f.health));
        this.setMeta("last_heartbeat_at", String(now));
        {
          const m = recordMetrics(this.sql, (k, v) => this.setMeta(k, v), f.metrics, f.health, now);
          this.heartbeatAlerts(f.health, m, now);
          this.heartbeatGithub(f.metrics, now);
        }
        this.renew(
          f.runs.map((r) => r.runId),
          now,
        );
        this.alive(
          f.runs.filter((r) => r.active === true && !r.done).map((r) => r.runId),
          now,
        );
        this.send(ws, { t: "heartbeat.ack", at: now });
        this.touchD1(now);
        return;
      case "account.login.status": {
        this.sql.exec(
          "INSERT OR REPLACE INTO logins (login_id, status, updated_at) VALUES (?, ?, ?)",
          f.loginId,
          JSON.stringify(f),
          now,
        );
        this.sql.exec("DELETE FROM logins WHERE updated_at < ?", now - 86_400_000);
        return;
      }
      case "run.accept": {
        const r = this.run(f.runId);
        if (!r) return;
        this.renew([f.runId], now);
        if (r.state !== "assigned") return;
        this.sql.exec("UPDATE runs SET state = 'accepted', updated_at = ? WHERE run_id = ?", now, f.runId);
        await deskStub(this.env, r.bot_id).runAccepted(f.runId, this.machineId as string);
        return;
      }
      case "run.reject": {
        const r = this.run(f.runId);
        if (!r || (r.state !== "assigned" && r.state !== "accepted")) return;
        this.sql.exec("UPDATE runs SET state = 'closed', updated_at = ? WHERE run_id = ?", now, f.runId);
        await deskStub(this.env, r.bot_id).runRejected(f.runId, this.machineId as string, f.reason, f.message);
        return;
      }
      default: {
        const r = this.run(f.runId);
        if (!r || r.state === "closed" || r.state === "lost") {
          // Not ours any more: ack so the daemon frees it; welcome leaves it out next time.
          this.send(ws, { t: "ack", runId: f.runId, seq: f.seq });
          return;
        }
        this.renew([f.runId], now);
        if (f.seq <= r.acked_seq) {
          this.send(ws, { t: "ack", runId: f.runId, seq: r.acked_seq });
          return;
        }
        // 模 3.2: a run that failed because its CLI is not signed in takes the runtime out of
        // the routing at once, before the next heartbeat says so.
        if (f.t === "run.done" && f.failure?.kind === "signed_out") this.markSignedOut(this.assignOut(r).runtime, now);
        // 模 3.4: a run at the subscription limit holds the runtime back here until the reset,
        // before the desk hears of it (it reads the mark to say when); a run that went well
        // after the mark's time clears it (one that went well before then may have started
        // before the limit was hit, and proves nothing).
        if (f.t === "run.done" && f.failure?.kind === "limit") this.markLimited(this.assignOut(r).runtime, f.failure.resetsAt, now);
        else if (f.t === "run.done" && f.status === "succeeded") this.clearLimited(this.assignOut(r).runtime, now);
        this.enqueue(r, f.t === "run.event" && f.event.type === "output" ? { ...f, serverAt: this.serverTime(f.at, now) } : f);
      }
    }
  }

  private hello(ws: WebSocket, att: Attachment, f: Extract<UpFrame, { t: "hello" }>, now: number): void {
    if (f.protocol !== MACHINE_PROTOCOL_VERSION) {
      this.send(ws, { t: "error", error: { code: "unsupported_protocol", message: `machine protocol ${MACHINE_PROTOCOL_VERSION} only` } });
      ws.close(4000, "unsupported protocol");
      return;
    }
    this.setMeta("info", JSON.stringify(f.machine));
    this.setMeta("connected_at", String(now));
    this.setMeta("clis", JSON.stringify(f.clis));
    this.setMeta("health", JSON.stringify(f.health));
    this.setMeta("last_heartbeat_at", String(now));
    this.renew(
      f.runs.map((r) => r.runId),
      now,
    );
    const open = this.openRuns();
    this.send(ws, {
      t: "welcome",
      protocol: MACHINE_PROTOCOL_VERSION,
      machineId: this.machineId,
      serverTime: now,
      runs: open.map((r) => ({ runId: r.run_id, ackedSeq: r.acked_seq })),
    });
    att.ready = true;
    ws.serializeAttachment(att);
    // Feature "limits": the number set in the app, before any assign; then D1's, in case it
    // changed while this link was asleep.
    this.sendLimits(ws);
    this.ctx.waitUntil(this.refreshLimits());
    this.backOnline(now);
    this.heldMachine(true);
    // Assigns that got no answer yet, then queued inputs, approvals and cancels.
    for (const r of open) {
      if (r.state === "assigned") {
        this.send(ws, this.assignOut(r));
        this.sql.exec("UPDATE runs SET sent_at = ? WHERE run_id = ?", now, r.run_id);
      }
    }
    this.flushOutbox(ws);
    this.touchD1(now, true);
  }

  /** True when the daemon of the last hello reports a 1.1 feature (machine-protocol.md "1.1 features"). */
  private hasFeature(name: string): boolean {
    const info = this.meta("info");
    if (!info) return false;
    try {
      return ((JSON.parse(info) as { features?: string[] }).features ?? []).includes(name);
    } catch {
      return false;
    }
  }

  /**
   * True when the daemon takes the 1.1 interaction frames (`run.answer`, `run.interaction`):
   * it reports feature "interact", or it is 1.1.0 or newer (`hello.machine.daemonVersion`).
   */
  private takesInteract(): boolean {
    if (this.hasFeature("interact")) return true;
    const info = this.meta("info");
    if (!info) return false;
    try {
      return versionAtLeast((JSON.parse(info) as { daemonVersion?: string }).daemonVersion ?? "", [1, 1, 0]);
    } catch {
      return false;
    }
  }

  /**
   * The frame as this daemon may get it. `run.answer` and `run.interaction` carry a `fallback`
   * (never sent): an older daemon gets it as `run.input` with the text envelope instead. Typed
   * answers and page events in `run.input` go only to a daemon with feature "answer".
   */
  private forDaemon(frame: Record<string, unknown>): Record<string, unknown> {
    if (frame.t === "run.answer" || frame.t === "run.interaction") {
      const { fallback, ...typed } = frame as { fallback?: Record<string, unknown> } & Record<string, unknown>;
      if (this.takesInteract() || !fallback) return typed;
      return this.forDaemon({ t: "run.input", runId: frame.runId, inputId: frame.inputId, ...fallback });
    }
    if (frame.t !== "run.input" || (frame.answer === undefined && frame.artifactEvent === undefined)) return frame;
    if (this.hasFeature("answer")) return frame;
    const { answer: _a, artifactEvent: _e, ...text } = frame;
    return text;
  }

  private flushOutbox(ws: WebSocket): void {
    const rows = this.sql.exec<{ id: number; frame: string }>("SELECT id, frame FROM outbox ORDER BY id").toArray();
    for (const row of rows) this.send(ws, this.forDaemon(JSON.parse(row.frame)));
    if (rows.length > 0) this.sql.exec("DELETE FROM outbox WHERE id <= ?", rows[rows.length - 1].id);
  }

  private touchD1(now: number, force = false): void {
    const last = Number(this.meta("d1_seen_at") ?? 0);
    if (!force && now - last < 60_000) return;
    this.setMeta("d1_seen_at", String(now));
    this.ctx.waitUntil(
      this.env.DB.prepare("UPDATE machines SET last_seen_at = ? WHERE id = ?")
        .bind(now, this.machineId)
        .run()
        .catch((err) => this.log.warn("last_seen update failed", { err })),
    );
  }

  // -------------------------------------------------------------------------
  // Forwarding to BotDesk, then ack
  // -------------------------------------------------------------------------

  private enqueue(r: RunRow, f: SeqFrame): void {
    let q = this.queues.get(r.run_id);
    if (!q) {
      q = { frames: [], busy: false, sinceAck: 0 };
      this.queues.set(r.run_id, q);
    }
    q.frames.push(f);
    if (!q.busy) this.ctx.waitUntil(this.drain(r.run_id, r.bot_id, q));
  }

  private async drain(runId: string, botId: string, q: { frames: SeqFrame[]; busy: boolean; sinceAck: number }) {
    q.busy = true;
    try {
      while (q.frames.length > 0) {
        const batch = q.frames.splice(0, ACK_EVERY);
        let stored = false;
        for (let attempt = 0; attempt < 3 && !stored; attempt++) {
          try {
            await deskStub(this.env, botId).ingest(runId, batch);
            stored = true;
          } catch (err) {
            this.log.warn("ingest failed", { runId, err, attempt });
            await new Promise((res) => setTimeout(res, 200 * (attempt + 1)));
          }
        }
        if (!stored) {
          // Unacked frames stay with the daemon; a new link replays them after welcome.
          q.frames.length = 0;
          this.socket()?.close(1011, "storage unavailable");
          return;
        }
        const top = Math.max(...batch.map((f) => f.seq));
        const done = batch.some((f) => f.t === "run.done");
        const session = batch.some((f) => f.t === "run.session");
        const row = this.run(runId);
        const hasUpload = row ? Boolean(this.assignOut(row).sessionUpload) : false;
        const now = Date.now();
        this.sql.exec("UPDATE runs SET acked_seq = max(acked_seq, ?), updated_at = ? WHERE run_id = ?", top, now, runId);
        if (session || (done && !hasUpload)) {
          this.sql.exec("UPDATE runs SET state = 'closed' WHERE run_id = ? AND state IN ('assigned','accepted','done')", runId);
        } else if (done) {
          this.sql.exec("UPDATE runs SET state = 'done' WHERE run_id = ? AND state IN ('assigned','accepted')", runId);
        }
        const ws = this.socket();
        if (ws) this.send(ws, { t: "ack", runId, seq: this.run(runId)?.acked_seq ?? top });
      }
    } finally {
      q.busy = false;
      if (q.frames.length === 0) this.queues.delete(runId);
    }
  }

  // -------------------------------------------------------------------------
  // Alarm: idle link, leases, assign retries
  // -------------------------------------------------------------------------

  async alarm(): Promise<void> {
    const now = Date.now();
    await this.offlineCheck(now);
    const ws = this.socket() ?? this.ctx.getWebSockets().find((s) => s.readyState === WebSocket.OPEN) ?? null;
    if (ws && now - Number(this.meta("last_frame_at") ?? 0) > LINK_IDLE_MS) {
      try {
        ws.close(4003, "no frames for 90 s");
      } catch {
        // Already closed.
      }
    }
    await this.expireLeases(now);
    const live = this.socket();
    for (const r of this.openRuns()) {
      if (r.state === "done" && now - r.updated_at > SESSION_WAIT_MS) {
        this.sql.exec("UPDATE runs SET state = 'closed' WHERE run_id = ?", r.run_id);
      } else if (live && r.state === "assigned" && (r.sent_at === null || now - r.sent_at >= ASSIGN_RETRY_MS)) {
        this.send(live, this.assignOut(r));
        this.sql.exec("UPDATE runs SET sent_at = ? WHERE run_id = ?", now, r.run_id);
      }
    }
    if (this.openRuns().length > 0 || this.ctx.getWebSockets().length > 0) await this.ctx.storage.setAlarm(now + TICK_MS);
  }

  private async expireLeases(now: number): Promise<void> {
    const expired = this.sql
      .exec<RunRow>("SELECT * FROM runs WHERE state IN ('assigned','accepted') AND held = 0 AND lease_until < ?", now)
      .toArray() as RunRow[];
    for (const r of expired) {
      // BotDesk decides: a run that waits for this machine (盯 1.1) stays open with a new lease,
      // so a daemon that comes back still holding it replays and goes on.
      let keep = false;
      try {
        keep = (await deskStub(this.env, r.bot_id).runLost(r.run_id, this.machineId as string)) === true;
      } catch (err) {
        this.log.error("runLost failed", { runId: r.run_id, err });
      }
      if (keep) {
        this.sql.exec("UPDATE runs SET lease_until = ? + lease_ms WHERE run_id = ? AND state IN ('assigned','accepted')", Date.now(), r.run_id);
        continue;
      }
      this.sql.exec("UPDATE runs SET state = 'lost', updated_at = ? WHERE run_id = ? AND state IN ('assigned','accepted')", now, r.run_id);
      this.sql.exec("DELETE FROM outbox WHERE run_id = ?", r.run_id);
    }
  }

  // -------------------------------------------------------------------------
  // RPC (BotDesk and the API Worker)
  // -------------------------------------------------------------------------

  /** Gives a run to this machine. Idempotent by runId. */
  async assign(machineId: string, botId: string, frame: RunAssignFrame): Promise<{ sent: boolean }> {
    this.bind(machineId);
    const now = Date.now();
    if (!this.run(frame.runId)) {
      this.sql.exec(
        `INSERT INTO runs (run_id, bot_id, assign, state, acked_seq, lease_ms, lease_until, sent_at, updated_at)
         VALUES (?, ?, ?, 'assigned', 0, ?, ?, NULL, ?)`,
        frame.runId,
        botId,
        JSON.stringify(frame),
        frame.leaseMs,
        now + frame.leaseMs,
        now,
      );
    }
    const ws = this.socket();
    if (ws) {
      this.send(ws, this.assignOut(this.run(frame.runId) as RunRow));
      this.sql.exec("UPDATE runs SET sent_at = ? WHERE run_id = ?", now, frame.runId);
    }
    await this.ensureAlarm();
    return { sent: ws !== null };
  }

  /**
   * run.input, run.approval, run.cancel, run.pause or run.resume for an open run; queued while the
   * link is down. With `noQueue` (a person's command, design-takeover 3.7) a frame is sent now or
   * not at all: `{ ok: true, sent: false }` while the link is down.
   */
  async toRun(
    frame: {
      t:
        | "run.input"
        | "run.approval"
        | "run.cancel"
        | "run.pause"
        | "run.resume"
        | "run.answer"
        | "run.interaction"
        | "run.output.result"
        | "run.command"
        | "run.command.cancel";
      runId: string;
    },
    opts: { noQueue?: boolean } = {},
  ): Promise<{ ok: boolean; sent: boolean }> {
    const r = this.run(frame.runId);
    if (!r || (r.state !== "assigned" && r.state !== "accepted")) return { ok: false, sent: false };
    const ws = this.socket();
    if (ws) this.send(ws, this.forDaemon(frame));
    else if (!opts.noQueue) this.sql.exec("INSERT INTO outbox (run_id, frame) VALUES (?, ?)", frame.runId, JSON.stringify(frame));
    return { ok: true, sent: ws !== null };
  }

  /**
   * design-takeover 3.7: BotDesk marks a run a person holds (paused or taken over). A held run's
   * lease does not run out, so it is never moved or ended because its machine is away, and its
   * queued frames stay. Released, it gets a fresh lease.
   */
  async setHeld(runId: string, held: boolean): Promise<void> {
    const now = Date.now();
    if (held) this.sql.exec("UPDATE runs SET held = 1 WHERE run_id = ?", runId);
    else this.sql.exec("UPDATE runs SET held = 0, lease_until = max(lease_until, ? + lease_ms) WHERE run_id = ? AND held = 1", now, runId);
  }

  /** Tells the desks of the runs people hold that this machine went away or is back. */
  private heldMachine(online: boolean): void {
    if (!this.machineId) return;
    const machineId = this.machineId;
    const held = this.sql
      .exec<{ run_id: string; bot_id: string }>(`SELECT run_id, bot_id FROM runs WHERE held = 1 AND state IN ('assigned','accepted')`)
      .toArray();
    for (const r of held) {
      this.ctx.waitUntil(
        deskStub(this.env, r.bot_id)
          .heldMachine(r.run_id, machineId, online)
          .catch((err) => this.log.warn("held machine update failed", { runId: r.run_id, err })),
      );
    }
  }

  private signedOutMarks(): Record<string, number> {
    try {
      return JSON.parse(this.meta("signed_out") ?? "{}") as Record<string, number>;
    } catch {
      return {};
    }
  }

  private markSignedOut(runtime: string, now: number): void {
    this.setMeta("signed_out", JSON.stringify({ ...this.signedOutMarks(), [runtime]: now }));
  }

  private limitMarks(): Record<string, LimitMark> {
    try {
      return JSON.parse(this.meta("limited") ?? "{}") as Record<string, LimitMark>;
    } catch {
      return {};
    }
  }

  private markLimited(runtime: string, resetsAt: number | undefined, now: number): void {
    const { metrics, at } = latestMetrics((k) => this.meta(k));
    const windows = metrics?.usage?.find((u) => u.runtime === runtime)?.windows;
    // Usage is read on the machine clock; the latest sample's offset puts it on the server's.
    const offset = metrics && at !== null ? at - metrics.at : 0;
    const marks = this.limitMarks();
    const mark = limitMark(resetsAt, windows, offset, marks[runtime], now);
    // Runs that hit the limit together keep the latest reset they know of.
    const prev = marks[runtime];
    const keep = prev && prev.until > mark.until && (prev.known || !mark.known) ? prev : mark;
    this.setMeta("limited", JSON.stringify({ ...marks, [runtime]: keep }));
    this.log.info("runtime at its limit", { runtime, until: keep.until, known: keep.known });
  }

  private clearLimited(runtime: string, now: number): void {
    const marks = this.limitMarks();
    const mark = marks[runtime];
    if (!mark || mark.until > now) return;
    delete marks[runtime];
    this.setMeta("limited", JSON.stringify(marks));
  }

  /**
   * 盯 1.14: the run moved to another machine. It is closed here: a connected daemon gets
   * `run.cancel` now; a daemon that is away finds it missing from its next `welcome.runs` and
   * stops it before anything else. Queued frames for it are dropped, and its late frames are
   * acked and dropped.
   */
  async closeRun(runId: string): Promise<void> {
    const r = this.run(runId);
    if (!r || r.state === "closed" || r.state === "lost") return;
    const ws = this.socket();
    if (ws) this.send(ws, { t: "run.cancel", runId });
    this.sql.exec("UPDATE runs SET state = 'closed', updated_at = ? WHERE run_id = ?", Date.now(), runId);
    this.sql.exec("DELETE FROM outbox WHERE run_id = ?", runId);
  }

  async status(): Promise<LinkStatus> {
    const parse = <T>(k: string): T | undefined => {
      const v = this.meta(k);
      return v ? (JSON.parse(v) as T) : undefined;
    };
    const hb = this.meta("last_heartbeat_at");
    const { metrics, at } = latestMetrics((k) => this.meta(k));
    const signedOut = signedOutRuntimes(metrics?.logins, at, this.signedOutMarks(), Date.now());
    const limited = limitedRuntimes(this.limitMarks(), Date.now());
    const github = parse<GithubState>("github");
    const parallelRuns = this.parallelRuns();
    return {
      ...(parallelRuns !== null ? { parallelRuns } : {}),
      ...(signedOut.length > 0 ? { signedOut } : {}),
      ...(limited.length > 0 ? { limited } : {}),
      ...(github ? { github } : {}),
      machineId: this.machineId,
      connected: this.socket() !== null,
      revoked: this.meta("revoked") !== null,
      lastHeartbeatAt: hb ? Number(hb) : undefined,
      info: parse("info"),
      health: parse("health"),
      clis: parse("clis"),
      runningRuns: this.sql
        .exec<{ run_id: string }>("SELECT run_id FROM runs WHERE state IN ('assigned','accepted') ORDER BY run_id")
        .toArray()
        .map((r) => r.run_id),
    };
  }

  /**
   * The owner's monitor (docs/protocol.md, "Machine monitor"): the link's status, the latest
   * metrics, the history of `range` and the bot of every open run.
   */
  async monitor(range: MonitorRange, now = Date.now()): Promise<{
    status: LinkStatus;
    connectedAt?: number;
    metrics: MachineMetrics | null;
    metricsAt: number | null;
    series: MonitorSeries;
    runBots: { runId: string; botId: string }[];
  }> {
    const { metrics, at } = latestMetrics((k) => this.meta(k));
    const connectedAt = Number(this.meta("connected_at")) || undefined;
    return {
      status: await this.status(),
      ...(connectedAt ? { connectedAt } : {}),
      metrics,
      metricsAt: at,
      series: readSeries(this.sql, range, now),
      runBots: this.openRuns().map((r) => ({ runId: r.run_id, botId: r.bot_id })),
    };
  }

  // -------------------------------------------------------------------------
  // Feature "accounts": sign in and API keys from the app (docs/protocol.md "Machine accounts")
  // -------------------------------------------------------------------------

  /** The open link of a daemon that takes "accounts" frames, or the reason it cannot. */
  private accountsLink(): WebSocket | "offline" | "unsupported" {
    const ws = this.socket();
    if (!ws) return "offline";
    return this.hasFeature("accounts") ? ws : "unsupported";
  }

  /** Sends one "accounts" frame now: these act on the machine at once, so nothing is queued. */
  async accountsFrame(frame: Record<string, unknown>): Promise<"sent" | "offline" | "unsupported"> {
    const ws = this.accountsLink();
    if (typeof ws === "string") return ws;
    if (frame.t === "account.login" && frame.op === "start") {
      this.sql.exec(
        "INSERT OR REPLACE INTO logins (login_id, status, updated_at) VALUES (?, ?, ?)",
        frame.loginId as string,
        JSON.stringify({ loginId: frame.loginId, runtime: frame.runtime, state: "starting" }),
        Date.now(),
      );
    }
    this.send(ws, frame);
    return "sent";
  }

  /** 机 4.3: the connected daemon keeps `secret.set.ask` (feature "ask-keys"). */
  async hasAskKeys(): Promise<boolean> {
    return this.socket() !== null && this.hasFeature("ask-keys");
  }

  /** The run limit set in the app as this link last heard it; null = the daemon's own. */
  private parallelRuns(): number | null {
    const v = this.meta("parallel_runs");
    return v ? Number(v) : null;
  }

  /** Feature "limits": tells the daemon how many runs it may do at once. */
  private sendLimits(ws: WebSocket): void {
    if (!this.hasFeature("limits")) return;
    const n = this.parallelRuns();
    this.send(ws, n === null ? { t: "machine.limits" } : { t: "machine.limits", maxParallelRuns: n });
  }

  private async refreshLimits(): Promise<void> {
    if (!this.machineId) return;
    try {
      const row = await this.env.DB.prepare("SELECT parallel_runs FROM machines WHERE id = ?")
        .bind(this.machineId)
        .first<{ parallel_runs: number | null }>();
      if (row) await this.setLimits(row.parallel_runs ?? null);
    } catch (err) {
      this.log.warn("run limit read failed", { err });
    }
  }

  /** `PATCH /v1/machines/:id` set how many runs it may do at once; the daemon hears it now when connected. */
  async setLimits(parallelRuns: number | null): Promise<void> {
    if (parallelRuns === this.parallelRuns()) return;
    if (parallelRuns === null) this.sql.exec("DELETE FROM meta WHERE key = 'parallel_runs'");
    else this.setMeta("parallel_runs", String(parallelRuns));
    const ws = this.socket();
    if (ws) this.sendLimits(ws);
  }

  /** Usable, feature "homes": sends `home.clear` now (never queued), or why it cannot. */
  async clearHome(memberId: string): Promise<"sent" | "offline" | "unsupported"> {
    const ws = this.socket();
    if (!ws) return "offline";
    if (!this.hasFeature("homes")) return "unsupported";
    this.send(ws, { t: "home.clear", memberId });
    return "sent";
  }

  /** A sign-in as the daemon last reported it. */
  async login(loginId: string): Promise<MachineLogin | null> {
    const row = this.sql
      .exec<{ status: string; updated_at: number }>("SELECT status, updated_at FROM logins WHERE login_id = ?", loginId)
      .toArray()[0];
    if (!row) return null;
    const f = JSON.parse(row.status) as Omit<MachineLogin, "updatedAt"> & { t?: string };
    const { t: _t, ...rest } = f;
    return { ...rest, updatedAt: row.updated_at };
  }

  /** The machine key and the keys it holds (names and last 4), from the latest heartbeat. */
  async secrets(): Promise<{ publicKey?: string; secrets: NonNullable<MachineMetrics["secrets"]> }> {
    const info = this.meta("info");
    const publicKey = info ? (JSON.parse(info) as { publicKey?: string }).publicKey : undefined;
    const { metrics } = latestMetrics((k) => this.meta(k));
    return { ...(publicKey && this.hasFeature("accounts") ? { publicKey } : {}), secrets: metrics?.secrets ?? [] };
  }

  /** How each runtime's CLI is signed in ("subscription" | "api_key"), from the latest heartbeat (budget/index.ts). */
  async loginMethods(): Promise<Record<string, string>> {
    const { metrics } = latestMetrics((k) => this.meta(k));
    return Object.fromEntries((metrics?.logins ?? []).flatMap((l) => (l.method ? [[l.runtime, l.method]] : [])));
  }

  /** The token was revoked: close the link and refuse frames from now on. */
  async revoke(machineId: string): Promise<void> {
    this.bind(machineId);
    this.setMeta("revoked", String(Date.now()));
    // 机 5.1: a removed computer needs nobody any more.
    for (const key of Object.keys(this.upAlerts())) this.clearAlert(key);
    for (const ws of this.ctx.getWebSockets()) {
      this.send(ws, { t: "error", error: { code: "unauthorized", message: "machine token revoked" } });
      try {
        ws.close(4001, "unauthorized");
      } catch {
        // Already closed.
      }
    }
  }
}
