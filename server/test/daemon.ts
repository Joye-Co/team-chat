// A fake machine daemon speaking docs/machine-protocol.md, and a world with a runner bot, for
// the bots and 1.1 routing tests.
import { SELF } from "cloudflare:test";
import { MachineDownFrame } from "@chat/protocol";
import { expect } from "vitest";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, waitUntil } from "./helpers";

/** Down frames that do not match the protocol; tests expect it empty at the end. */
export const downViolations: unknown[] = [];

// ---------------------------------------------------------------------------
// A fake machine daemon speaking docs/machine-protocol.md
// ---------------------------------------------------------------------------

export const HEALTH = { diskFreeBytes: 100e9, diskTotalBytes: 500e9, load1: 0.5, load5: 0.5, load15: 0.5, cpuCount: 8, activeRuns: 0 };

export class Daemon {
  frames: any[] = [];
  closed: { code: number } | null = null;
  seq = new Map<string, number>();
  constructor(readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      const f = JSON.parse(e.data as string);
      const check = MachineDownFrame.safeParse(f);
      if (!check.success) downViolations.push({ frame: f, issues: check.error.issues });
      this.frames.push(f);
    });
    ws.addEventListener("close", (e) => {
      this.closed = { code: e.code };
    });
  }

  static async connect(
    token: string,
    opts: { labels?: string[]; health?: Partial<typeof HEALTH>; runs?: { runId: string; lastSeq: number; done: boolean }[]; runtime?: string; features?: string[]; version?: string } = {},
  ): Promise<{ d: Daemon; welcome: any }> {
    const res = await SELF.fetch(`${BASE}/v1/machine/ws`, { headers: { upgrade: "websocket", authorization: `Bearer ${token}` } });
    if (!res.webSocket) throw new Error(`upgrade failed: ${res.status}`);
    res.webSocket.accept();
    const d = new Daemon(res.webSocket);
    d.send({
      t: "hello",
      protocol: 1,
      machine: { name: "mac-mini", labels: opts.labels ?? ["mac"], daemonVersion: opts.version ?? "0.1.0", os: "macos", arch: "aarch64", maxParallelRuns: 2, ...(opts.features ? { features: opts.features } : {}) },
      clis: [{ runtime: opts.runtime ?? "codex", kind: "codex", command: "codex", version: "0.159.0", available: true }],
      health: { ...HEALTH, ...opts.health },
      runs: opts.runs ?? [],
    });
    const welcome = await d.next((f) => f.t === "welcome" || f.t === "error");
    return { d, welcome };
  }

  send(f: unknown): void {
    this.ws.send(JSON.stringify(f));
  }

  async next(pred: (f: any) => boolean, ms = 5000): Promise<any> {
    return waitUntil(() => this.frames.find(pred), ms, "daemon frame");
  }

  event(runId: string, event: unknown): number {
    const seq = (this.seq.get(runId) ?? 0) + 1;
    this.seq.set(runId, seq);
    this.send({ t: "run.event", runId, seq, at: Date.now(), event });
    return seq;
  }

  done(runId: string, extra: Record<string, unknown>): number {
    const seq = (this.seq.get(runId) ?? 0) + 1;
    this.seq.set(runId, seq);
    this.send({ t: "run.done", runId, seq, at: Date.now(), durationMs: 1000, ...extra });
    return seq;
  }

  async acked(runId: string, seq: number): Promise<void> {
    await this.next((f) => f.t === "ack" && f.runId === runId && f.seq >= seq);
  }

  close(): void {
    try {
      this.ws.close(1000, "done");
    } catch {
      // ignore
    }
  }
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

export interface World {
  owner: TestMember;
  human: TestMember;
  bot: TestMember;
  gid: string;
  machineToken: string;
  machineId: string;
}

/** Every world gets its own machine label, so runs never go to another test's machine. */
export async function world(opts: { runtime?: string; labels?: string[] } = {}): Promise<World> {
  const label = `t-${crypto.randomUUID().slice(0, 8)}`;
  const owner = await createMember({ level: "owner", name: "Joye" });
  const human = await createMember({ name: "Alice" });
  const bot = await createMember({ kind: "bot", name: "dev" });
  const gid = await createGroup(owner, [human.id, bot.id]);
  const r = await api(`/v1/bots/${bot.id}/runner`, {
    method: "PUT",
    token: owner.token,
    body: { runtime: opts.runtime ?? "codex", labels: opts.labels ?? [label], repo: { path: "/tmp/repo" }, baseBranch: "main" },
  });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const m = await api("/v1/machines", { token: owner.token, body: { name: "mac-mini", labels: [label] } });
  expect(m.status).toBe(200);
  return { owner, human, bot, gid, machineToken: m.body.token, machineId: m.body.machine.id };
}

/** Human starts a topic that mentions the bot; returns the topic id. */
export async function mention(c: Client, w: World, body = "@dev please fix tests"): Promise<string> {
  const res = await c.ok({ t: "send", groupId: w.gid, clientMsgId: clientMsgId(), body, mentions: [w.bot.id] });
  return res.message.topicId as string;
}
