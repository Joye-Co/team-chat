// One check of one target: GET /v1/health, then a real round trip over the WebSocket: the
// monitor bot sends a message in its own topic of the probe group (default: the ops group) and
// waits for the event. Then the archive freshness (GET /v1/ops/archive).
import type { CheckResult } from "./alerts";
import type { Target } from "./config";

export const HEALTH_TIMEOUT_MS = 10_000;
export const ROUND_TRIP_TIMEOUT_MS = 15_000;
export const ROUND_TRIP_TITLE = "监测往返";
/** The daily archive runs at 03:00 UTC; a pass older than this missed a day. */
export const BACKUP_MAX_AGE_MS = 26 * 60 * 60 * 1000;

export interface ProbeResult extends CheckResult {
  /** The ops group id as resolved from welcome (for posting alerts). */
  opsGroupId: string | null;
  /** The round-trip topic, to reuse next time. */
  topicId: string | null;
  /** Archive freshness, checked only when the round trip passed. */
  backup?: CheckResult;
  /** People (human members) of the ops group: mentioned on alerts unless `oncall` is set. */
  oncall?: string[];
}

class Fail extends Error {}

function withTimeout<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  return Promise.race([
    p,
    new Promise<T>((_, rej) => {
      timer = setTimeout(() => rej(new Fail(`${what}: timeout`)), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

/** A WebSocket session that waits for frames. */
export class Session {
  private frames: any[] = [];
  private waiters: { pred: (f: any) => boolean; resolve: (f: any) => void }[] = [];
  private closed = false;
  private n = 0;
  constructor(readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      let f: any;
      try {
        f = JSON.parse(typeof e.data === "string" ? e.data : new TextDecoder().decode(e.data as ArrayBuffer));
      } catch {
        return;
      }
      this.frames.push(f);
      if (this.frames.length > 500) this.frames.shift();
      this.waiters = this.waiters.filter((w) => (w.pred(f) ? (w.resolve(f), false) : true));
    });
    ws.addEventListener("close", () => {
      this.closed = true;
    });
  }

  static async open(fetcher: typeof fetch, baseUrl: string, token: string): Promise<Session> {
    const res = await fetcher(`${baseUrl}/v1/ws`, {
      headers: { upgrade: "websocket", authorization: `Bearer ${token}` },
    });
    const ws = (res as Response & { webSocket?: WebSocket | null }).webSocket;
    if (!ws) throw new Fail(`websocket: ${res.status}`);
    ws.accept();
    return new Session(ws);
  }

  wait(pred: (f: any) => boolean, ms: number, what: string): Promise<any> {
    const hit = this.frames.find(pred);
    if (hit) return Promise.resolve(hit);
    if (this.closed) return Promise.reject(new Fail(`${what}: socket closed`));
    return withTimeout(new Promise((resolve) => this.waiters.push({ pred, resolve })), ms, what);
  }

  rid(): string {
    return `m${++this.n}`;
  }

  send(frame: unknown): void {
    this.ws.send(JSON.stringify(frame));
  }

  close(): void {
    try {
      this.ws.close(1000, "done");
    } catch {
      // already closed
    }
  }
}

function findGroup(welcome: any, opsGroup: string): string | null {
  const groups: any[] = Array.isArray(welcome?.groups) ? welcome.groups : [];
  const hit =
    groups.find((g) => g?.group?.id === opsGroup) ??
    groups.find((g) => g?.group?.kind === "group" && g?.group?.name === opsGroup);
  return hit?.group?.id ?? null;
}

function humansOf(welcome: any, groupId: string): string[] {
  const group = (welcome.groups as any[]).find((g) => g?.group?.id === groupId)?.group;
  const members: any[] = Array.isArray(welcome?.members) ? welcome.members : [];
  const humans = new Set(members.filter((m) => m?.kind === "human" && !m?.deactivated).map((m) => m.id as string));
  return (Array.isArray(group?.memberIds) ? (group.memberIds as string[]) : []).filter((id) => humans.has(id));
}

export async function probe(
  target: Target,
  token: string | null,
  topicId: string | null,
  opts: { fetcher?: typeof fetch; now?: () => number } = {},
): Promise<ProbeResult> {
  const fetcher = opts.fetcher ?? fetch;
  const now = opts.now ?? Date.now;
  const at = now();
  const result: ProbeResult = {
    target: target.name,
    at,
    ok: false,
    healthMs: null,
    rttMs: null,
    error: null,
    opsGroupId: null,
    topicId,
  };
  try {
    const t0 = now();
    const res = await withTimeout(fetcher(`${target.baseUrl}/v1/health`), HEALTH_TIMEOUT_MS, "health");
    const body = (await res.json().catch(() => null)) as { ok?: boolean } | null;
    if (!res.ok || body?.ok !== true) throw new Fail(`health: ${res.status}`);
    result.healthMs = now() - t0;

    if (!token) throw new Fail("round trip: no monitor token configured");
    const s = await withTimeout(Session.open(fetcher, target.baseUrl, token), ROUND_TRIP_TIMEOUT_MS, "websocket");
    try {
      s.send({ t: "hello", protocol: 1, client: { name: "monitor", version: "1" } });
      const welcome = await s.wait((f) => f.t === "welcome" || f.t === "error", ROUND_TRIP_TIMEOUT_MS, "welcome");
      if (welcome.t !== "welcome") throw new Fail(`welcome: ${welcome.error?.code ?? "error"}`);
      const opsGroupId = findGroup(welcome, target.opsGroup);
      if (!opsGroupId) throw new Fail("round trip: monitor bot is not in the ops group");
      result.opsGroupId = opsGroupId;
      result.oncall = humansOf(welcome, opsGroupId);
      const groupId = target.probeGroup ? findGroup(welcome, target.probeGroup) : opsGroupId;
      if (!groupId) throw new Fail("round trip: monitor bot is not in the probe group");
      for (let attempt = 0; attempt < 2; attempt++) {
        const clientMsgId = crypto.randomUUID();
        const rid = s.rid();
        const frame: Record<string, unknown> = {
          t: "send",
          rid,
          groupId,
          clientMsgId,
          body: `往返检测 ${new Date(at).toISOString()}`,
        };
        if (result.topicId) frame.topicId = result.topicId;
        else frame.title = ROUND_TRIP_TITLE;
        const sentAt = now();
        s.send(frame);
        const ack = await s.wait((f) => f.rid === rid, ROUND_TRIP_TIMEOUT_MS, "round trip ack");
        if (!ack.ok) {
          if (ack.error?.code === "not_found" && result.topicId) {
            result.topicId = null; // the topic is gone (e.g. after a restore): start a new one
            continue;
          }
          throw new Fail(`round trip: ${ack.error?.code ?? "error"}`);
        }
        if (ack.result?.topic?.id) result.topicId = ack.result.topic.id;
        await s.wait(
          (f) =>
            f.t === "event" &&
            f.event?.type === "message.created" &&
            f.event?.data?.message?.clientMsgId === clientMsgId,
          ROUND_TRIP_TIMEOUT_MS,
          "round trip event",
        );
        result.rttMs = now() - sentAt;
        result.ok = true;
        break;
      }
      if (!result.ok) throw new Fail("round trip: topic not found");
    } finally {
      s.close();
    }
    result.backup = await backupCheck(fetcher, target, token, now);
  } catch (err) {
    result.ok = false;
    result.error = err instanceof Fail ? err.message : "network error";
  }
  return result;
}

/** Fails when the newest complete archive pass is older than BACKUP_MAX_AGE_MS (or never ran). */
export async function backupCheck(
  fetcher: typeof fetch,
  target: Target,
  token: string,
  now: () => number,
): Promise<CheckResult> {
  const at = now();
  const r: CheckResult = { target: target.name, check: "backup", at, ok: false, healthMs: null, rttMs: null, error: null };
  try {
    const res = await withTimeout(
      fetcher(`${target.baseUrl}/v1/ops/archive`, { headers: { authorization: `Bearer ${token}` } }),
      HEALTH_TIMEOUT_MS,
      "backup",
    );
    if (!res.ok) throw new Fail(`backup: ${res.status}`);
    const body = (await res.json().catch(() => null)) as { lastCompleteAt?: number | null } | null;
    const last = typeof body?.lastCompleteAt === "number" ? body.lastCompleteAt : null;
    if (last === null) throw new Fail("backup: no complete archive yet");
    const age = at - last;
    if (age > BACKUP_MAX_AGE_MS) throw new Fail(`backup: last complete archive ${Math.round(age / 3_600_000)} h ago`);
    r.ok = true;
  } catch (err) {
    r.error = err instanceof Fail ? err.message : "backup: network error";
  }
  return r;
}
