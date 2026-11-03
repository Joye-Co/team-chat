// A fake server for unit tests: a WebSocket class for `BotOptions.WebSocket` that answers
// hello with a welcome, records every client frame, and answers requests through handlers
// the test sets. No network.
export type Frame = Record<string, unknown> & { t: string };
export type Reply = { ok: true; result?: unknown } | { ok: false; error: { code: string; message: string; reason?: string } } | "none";

export const BOT_ID = "01JC0MBR000000000000LEDGER";
export const HUMAN_ID = "01JC0MBR00000000000000JOYE";
export const GROUP_ID = "01JC0GRW0000000000000000GA";
export const TOPIC_ID = "01JC0TPC0000000000000000T1";

export class FakeServer {
  readonly frames: Frame[] = [];
  sockets: FakeSocket[] = [];
  features = ["interact/1", "refs/1", "diagram/1"];
  groups: unknown[] = [];
  private handlers = new Map<string, (f: Frame) => Reply | Promise<Reply>>();
  private waiters: { pred: (f: Frame) => boolean; resolve: (f: Frame) => void }[] = [];

  readonly WebSocket: new (url: string) => WebSocket;

  constructor() {
    const server = this;
    class Socket extends FakeSocket {
      constructor(url: string) {
        super(server, url);
      }
    }
    this.WebSocket = Socket as unknown as new (url: string) => WebSocket;
  }

  on(t: string, h: (f: Frame) => Reply | Promise<Reply>): this {
    this.handlers.set(t, h);
    return this;
  }

  /** The handler set for `t`, to wrap it. */
  handler(t: string): ((f: Frame) => Reply | Promise<Reply>) | undefined {
    return this.handlers.get(t);
  }

  of(t: string): Frame[] {
    return this.frames.filter((f) => f.t === t);
  }

  /** Resolves with the first client frame (already sent or next) that matches. */
  next(pred: (f: Frame) => boolean, ms = 3000): Promise<Frame> {
    const seen = this.frames.find(pred);
    if (seen) return Promise.resolve(seen);
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("no matching frame")), ms);
      this.waiters.push({
        pred,
        resolve: (f) => {
          clearTimeout(t);
          resolve(f);
        },
      });
    });
  }

  /** Sends a server frame to every open socket. */
  push(frame: Record<string, unknown>): void {
    for (const s of this.sockets) if (s.readyState === 1) s.deliver(frame);
  }

  transient(event: Record<string, unknown>): void {
    this.push({ t: "transient", event });
  }

  event(event: Record<string, unknown>): void {
    this.push({ t: "event", event });
  }

  async receive(sock: FakeSocket, f: Frame): Promise<void> {
    if (f.t === "ping") return;
    this.frames.push(f);
    for (const w of this.waiters.splice(0)) {
      if (w.pred(f)) w.resolve(f);
      else this.waiters.push(w);
    }
    if (f.t === "hello") {
      sock.deliver({
        t: "welcome",
        protocol: 1,
        me: { id: BOT_ID, kind: "bot", name: "Ledger", level: "member" },
        members: [
          { id: BOT_ID, kind: "bot", name: "Ledger", level: "member" },
          { id: HUMAN_ID, kind: "human", name: "Joye", level: "owner" },
        ],
        groups: this.groups,
        serverTime: Date.now(),
        features: this.features,
      });
      return;
    }
    if (typeof f.rid !== "string") return;
    const h = this.handlers.get(f.t);
    const r: Reply = h ? await h(f) : { ok: true, result: {} };
    if (r === "none") return;
    if (f.t === "history" && r.ok) {
      sock.deliver({ t: "history", rid: f.rid, ...(r.result as object) });
      return;
    }
    sock.deliver(r.ok ? { t: "ack", rid: f.rid, ok: true, result: r.result } : { t: "ack", rid: f.rid, ok: false, error: r.error });
  }
}

export class FakeSocket {
  readyState = 0;
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  private readonly server: FakeServer;

  constructor(server: FakeServer, _url: string) {
    this.server = server;
    server.sockets.push(this);
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.();
    }, 0);
  }

  send(text: string): void {
    void this.server.receive(this, JSON.parse(text) as Frame);
  }

  close(code = 1000, reason = ""): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    setTimeout(() => this.onclose?.({ code, reason }), 0);
  }

  deliver(frame: Record<string, unknown>): void {
    setTimeout(() => this.onmessage?.({ data: JSON.stringify(frame) }), 0);
  }
}

let seq = 0;
/** A valid message of the bot (or `authorId`) in the test topic. */
export function message(o: Record<string, unknown> & { id: string }): Record<string, unknown> {
  seq++;
  return {
    clientMsgId: `c${seq}`,
    groupId: GROUP_ID,
    topicId: TOPIC_ID,
    authorId: BOT_ID,
    body: "",
    mentions: [],
    createdSeq: seq,
    createdAt: 1762138320000 + seq,
    ...o,
  };
}
