import { SELF, env } from "cloudflare:test";
import { ServerFrame } from "@chat/protocol";
import { expect } from "vitest";
import { newToken, sha256Hex } from "../src/core/crypto";
import { ulid } from "../src/core/ids";

export const BASE = "http://chat.test";

export interface TestMember {
  id: string;
  token: string;
  tokenId: string;
  name: string;
}

/** Inserts a member and a token straight into D1 (bypasses bootstrap, which works once per DB). */
export async function createMember(
  opts: { level?: "owner" | "member"; kind?: "human" | "bot"; name?: string; teamOwner?: boolean } = {},
): Promise<TestMember> {
  const id = ulid();
  const tokenId = ulid();
  const token = newToken();
  const name = opts.name ?? `m-${id.slice(-6)}`;
  const now = Date.now();
  await env.DB.batch([
    // A human owner is a 所有者 unless the test says otherwise, as migration 0017_roles makes them.
    env.DB.prepare("INSERT INTO members (id, kind, name, level, team_owner, created_at) VALUES (?, ?, ?, ?, ?, ?)").bind(
      id,
      opts.kind ?? "human",
      name,
      opts.level ?? "member",
      (opts.teamOwner ?? (opts.level === "owner" && (opts.kind ?? "human") === "human")) ? 1 : 0,
      now,
    ),
    env.DB.prepare("INSERT INTO tokens (id, token_hash, member_id, label, created_at) VALUES (?, ?, ?, 'test', ?)").bind(
      tokenId,
      await sha256Hex(token),
      id,
      now,
    ),
  ]);
  return { id, token, tokenId, name };
}

export async function api(
  path: string,
  opts: { method?: string; token?: string; body?: unknown; headers?: Record<string, string>; raw?: string } = {},
): Promise<{ status: number; body: any }> {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  let body: string | undefined = opts.raw;
  if (opts.body !== undefined) {
    headers["content-type"] = "application/json";
    body = JSON.stringify(opts.body);
  }
  const res = await SELF.fetch(`${BASE}${path}`, { method: opts.method ?? (body ? "POST" : "GET"), headers, body });
  const text = await res.text();
  let parsed: unknown = text;
  try {
    parsed = JSON.parse(text);
  } catch {
    // not JSON
  }
  return { status: res.status, body: parsed };
}

export async function createGroup(owner: TestMember, memberIds: string[], name = "g"): Promise<string> {
  const r = await api("/v1/groups", { token: owner.token, body: { name, memberIds } });
  expect(r.status).toBe(200);
  return r.body.group.id as string;
}

export async function waitUntil<T>(fn: () => T | undefined | false, ms = 5000, what = "condition"): Promise<T> {
  const start = Date.now();
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

/** Every server frame a test client got that does not parse with the protocol schema. */
export const protocolViolations: unknown[] = [];

export class Client {
  frames: any[] = [];
  private seen = 0;
  closed: { code: number } | null = null;
  private n = 0;
  constructor(readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      const f = JSON.parse(e.data as string);
      const check = ServerFrame.safeParse(f);
      if (!check.success) protocolViolations.push({ frame: f, issues: check.error.issues });
      this.frames.push(f);
    });
    ws.addEventListener("close", (e) => {
      this.closed = { code: e.code };
    });
  }

  static async open(token: string): Promise<Client> {
    const res = await SELF.fetch(`${BASE}/v1/ws`, {
      headers: { upgrade: "websocket", authorization: `Bearer ${token}` },
    });
    if (!res.webSocket) throw new Error(`upgrade failed: ${res.status}`);
    res.webSocket.accept();
    return new Client(res.webSocket);
  }

  /** Opens a socket and completes hello; returns the welcome frame too. */
  static async connect(token: string, opts: { features?: string[]; supportedCatalogs?: string[] } = {}): Promise<{ c: Client; welcome: any }> {
    const c = await Client.open(token);
    const welcome = await c.hello(opts.features, opts.supportedCatalogs);
    return { c, welcome };
  }

  send(frame: unknown): void {
    this.ws.send(typeof frame === "string" ? frame : JSON.stringify(frame));
  }

  rid(): string {
    return `r${++this.n}`;
  }

  async hello(features?: string[], supportedCatalogs?: string[]): Promise<any> {
    this.send({
      t: "hello",
      protocol: 1,
      client: { name: "test", version: "0" },
      ...(features ? { features } : {}),
      ...(supportedCatalogs ? { supportedCatalogs } : {}),
    });
    return this.next((f) => f.t === "welcome" || f.t === "error");
  }

  /** Waits for a frame (from all frames received so far) matching `pred`. */
  async next(pred: (f: any) => boolean, ms = 5000): Promise<any> {
    return waitUntil(() => this.frames.find(pred), ms, "frame");
  }

  /** Sends a frame with a fresh rid and waits for its answer (ack, page or history). */
  async request(frame: Record<string, unknown>): Promise<any> {
    const rid = this.rid();
    this.send({ ...frame, rid });
    return this.next((f) => f.rid === rid);
  }

  async ok(frame: Record<string, unknown>): Promise<any> {
    const r = await this.request(frame);
    expect(r, JSON.stringify(r)).toMatchObject({ ok: true });
    return r.result;
  }

  events(groupId?: string): any[] {
    return this.frames.filter((f) => f.t === "event" && (!groupId || f.event.groupId === groupId)).map((f) => f.event);
  }

  async waitEvent(pred: (e: any) => boolean, ms = 5000): Promise<any> {
    const f = await this.next((f) => f.t === "event" && pred(f.event), ms);
    return f.event;
  }

  close(): void {
    try {
      this.ws.close(1000, "done");
    } catch {
      // ignore
    }
  }
}

export function clientMsgId(): string {
  return crypto.randomUUID();
}
