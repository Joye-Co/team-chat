// MachineLink: one per machine. Holds the daemon's WebSocket (hibernatable, at most one live
// link), the runs assigned to the machine and their leases, and forwards run frames to the
// bot's BotDesk before it acks them. It never runs anything itself. docs/machine-protocol.md.
import { DurableObject } from "cloudflare:workers";
import {
  CliInfo,
  Health,
  MACHINE_PROTOCOL_VERSION,
  MachineInfo,
  type RunAssignFrame,
  RunCursor,
  RunDoneFrame,
  RunId,
  RunSessionFrame,
} from "@chat/protocol";
import { z } from "zod";
import { logger as baseLogger, type Logger } from "../core/logger";
import { runMigrations } from "../core/migrations";
import { ASSIGN_RETRY_MS, LINK_IDLE_MS } from "./dispatch";
import { type BotsEnv, deskStub } from "./env";

export const MACHINE_HEADER = "x-chat-machine-id";

/** How often the alarm checks leases, assigns and the link while anything is open. */
const TICK_MS = 30_000;
/** A finished run waits this long for its run.session before it is closed. */
const SESSION_WAIT_MS = 60 * 60_000;
const ACK_EVERY = 100;
const MAX_FRAME_BYTES = 1_048_576;

// Daemon frames as the server reads them. Run events are kept as objects with a string `type`
// so a newer daemon's event types are stored, not rejected (machine-protocol.md: unknown
// fields and types are ignored on read, and a rejected frame would be resent forever).
const LooseEvent = z.looseObject({ type: z.string().min(1).max(64) });
const UpFrame = z.discriminatedUnion("t", [
  z.object({
    t: z.literal("hello"),
    protocol: z.number(),
    machine: MachineInfo,
    clis: z.array(CliInfo).max(64),
    health: Health,
    runs: z.array(RunCursor).max(1000),
  }),
  z.object({ t: z.literal("heartbeat"), at: z.number(), health: Health, runs: z.array(RunCursor).max(1000) }),
  z.object({ t: z.literal("run.accept"), runId: RunId }),
  z.object({ t: z.literal("run.reject"), runId: RunId, reason: z.string().max(64), message: z.string().max(4096) }),
  z.object({ t: z.literal("run.event"), runId: RunId, seq: z.int().positive(), at: z.number(), event: LooseEvent }),
  RunDoneFrame.extend({ status: z.string().max(32) }),
  RunSessionFrame.extend({ source: z.string().max(32).optional() }),
]);
type UpFrame = z.infer<typeof UpFrame>;
/** A run frame with a seq, as stored by BotDesk. */
export type SeqFrame = Extract<UpFrame, { seq: number }>;

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
}

const OPEN_STATES = "('assigned','accepted','done')";

export class MachineLink extends DurableObject<BotsEnv> {
  private readonly sql: SqlStorage;
  private machineId: string | null = null;
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
        this.setMeta("health", JSON.stringify(f.health));
        this.setMeta("last_heartbeat_at", String(now));
        this.renew(
          f.runs.map((r) => r.runId),
          now,
        );
        this.send(ws, { t: "heartbeat.ack", at: now });
        this.touchD1(now);
        return;
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
        this.enqueue(r, f);
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

  /** The frame as this daemon may get it: typed answers and page events only with feature "answer". */
  private forDaemon(frame: Record<string, unknown>): Record<string, unknown> {
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
      .exec<RunRow>("SELECT * FROM runs WHERE state IN ('assigned','accepted') AND lease_until < ?", now)
      .toArray() as RunRow[];
    for (const r of expired) {
      this.sql.exec("UPDATE runs SET state = 'lost', updated_at = ? WHERE run_id = ?", now, r.run_id);
      this.sql.exec("DELETE FROM outbox WHERE run_id = ?", r.run_id);
      try {
        await deskStub(this.env, r.bot_id).runLost(r.run_id, this.machineId as string);
      } catch (err) {
        this.log.error("runLost failed", { runId: r.run_id, err });
      }
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

  /** run.input, run.approval, run.cancel, run.pause or run.resume for an open run; queued while the link is down. */
  async toRun(frame: { t: "run.input" | "run.approval" | "run.cancel" | "run.pause" | "run.resume"; runId: string }): Promise<{ ok: boolean; sent: boolean }> {
    const r = this.run(frame.runId);
    if (!r || (r.state !== "assigned" && r.state !== "accepted")) return { ok: false, sent: false };
    const ws = this.socket();
    if (ws) this.send(ws, this.forDaemon(frame));
    else this.sql.exec("INSERT INTO outbox (run_id, frame) VALUES (?, ?)", frame.runId, JSON.stringify(frame));
    return { ok: true, sent: ws !== null };
  }

  async status(): Promise<LinkStatus> {
    const parse = <T>(k: string): T | undefined => {
      const v = this.meta(k);
      return v ? (JSON.parse(v) as T) : undefined;
    };
    const hb = this.meta("last_heartbeat_at");
    return {
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

  /** The token was revoked: close the link and refuse frames from now on. */
  async revoke(machineId: string): Promise<void> {
    this.bind(machineId);
    this.setMeta("revoked", String(Date.now()));
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
