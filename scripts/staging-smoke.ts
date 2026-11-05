#!/usr/bin/env bun
// Measures the V1 quality bar (docs/PRODUCT.md, V1 items 1, 2 and 4) against a deployed env.
// Two members take part: the owner and a bot member, both with token files. Everything happens
// in one group named "smoke" (made once, then reused), so repeated runs add no groups.
//
//   delivery     p95 time from the owner's send to the bot's live event (bar: 300 ms)
//   search       p95 round trip of a Chinese two-character search (bar: 300 ms)
//   catchUp      hello -> welcome -> every group fetched from seq 0, page by page (bar: 2 s;
//                a full fetch is more than a day offline)
//   reconnect    the bot goes offline, the owner sends 5 messages, the bot reconnects with its
//                cursor: every message arrives once, none twice
//   notify       a bot message that mentions the owner arrives live with notify "mention"
//   upload       a file of --upload-mb MB (default 100; 0 skips) is accepted and reads back with
//                the same size and SHA-256
//   cards        p95 time from the bot's send of a Card v2 ask (ask.from requester and owners,
//                the per-send requester and member lookups, GAPS PC-16) to its ack, and to the
//                owner's live event (bar: 300 ms); skipped on a server before 1.1
// Each timed loop starts with --warmup untimed rounds (default 2), so the first access to a room
// after a deploy (cold start, named migrations) is not counted. Every sample is in the report
// (`samples`, ms), so one slow request shows.
// Round trips include the network: `pingMs` (WebSocket ping) is reported next to them so the
// server part can be read off. Sleep and the macOS notification banner are app behavior and are
// not measured here.
//
// Usage: bun scripts/staging-smoke.ts [--env staging] [--base <url>] [--owner-token <file>]
//        [--bot-token <file>] [--upload-mb 100] [--samples 50] [--warmup 2] [--out <folder>]
// Token files default to ~/.config/team-chat/<env>/owner-token and the newest
// ~/.config/team-chat/<env>/bot-*.token. Report (no tokens): .tmp/staging-smoke/report.json.
// Exits 1 when a check fails or a bar is missed.
import { createHash, randomBytes } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const ROOT = join(import.meta.dir, "..");
const args = process.argv.slice(2);
const opt = (name: string, dflt?: string): string | undefined => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : dflt;
};
const ENV = opt("env", "staging")!;
const product = JSON.parse(readFileSync(join(ROOT, "product.json"), "utf8")) as { servers: Record<string, string> };
const BASE = (opt("base") ?? product.servers[ENV] ?? "").replace(/\/$/, "");
if (!BASE) throw new Error(`no base URL for env ${ENV}; pass --base`);
const CONF = join(process.env.TEAM_CHAT_CONFIG_DIR ?? join(homedir(), ".config/team-chat"), ENV);
const newestBot = (): string => {
  const files = readdirSync(CONF).filter((f) => /^bot-.*\.token$/.test(f));
  files.sort((a, b) => statSync(join(CONF, b)).mtimeMs - statSync(join(CONF, a)).mtimeMs);
  if (!files[0]) throw new Error(`no bot-*.token in ${CONF}; pass --bot-token`);
  return join(CONF, files[0]);
};
const OWNER = readFileSync(opt("owner-token") ?? join(CONF, "owner-token"), "utf8").trim();
const BOT = readFileSync(opt("bot-token") ?? newestBot(), "utf8").trim();
const UPLOAD_MB = Number(opt("upload-mb", "100"));
const SAMPLES = Number(opt("samples", "50"));
const WARMUP = Number(opt("warmup", "2"));
const OUT = opt("out") ?? join(ROOT, ".tmp/staging-smoke");

type Frame = { t: string; rid?: string; [k: string]: any };

class Conn {
  ws!: WebSocket;
  welcome!: Frame;
  private waiters: { match: (f: Frame) => boolean; resolve: (f: Frame) => void }[] = [];
  private rid = 0;
  listeners: ((f: Frame) => void)[] = [];
  constructor(private token: string) {}

  async open(cursors?: Record<string, number>): Promise<number> {
    const t0 = performance.now();
    this.ws = new WebSocket(`${BASE.replace(/^http/, "ws")}/v1/ws`, { headers: { authorization: `Bearer ${this.token}` } } as any);
    this.ws.onmessage = (e) => {
      const f = JSON.parse(String(e.data)) as Frame;
      for (const l of this.listeners) l(f);
      const i = this.waiters.findIndex((w) => w.match(f));
      if (i >= 0) this.waiters.splice(i, 1)[0].resolve(f);
    };
    await new Promise<void>((res, rej) => {
      this.ws.onopen = () => res();
      this.ws.onerror = () => rej(new Error("socket error"));
    });
    const w = this.wait((f) => f.t === "welcome" || f.t === "error");
    this.ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "staging-smoke", version: "1" }, ...(cursors ? { cursors } : {}) }));
    this.welcome = await w;
    if (this.welcome.t !== "welcome") throw new Error(`hello failed: ${this.welcome.error?.code}`);
    return performance.now() - t0;
  }

  wait(match: (f: Frame) => boolean, ms = 15000): Promise<Frame> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("timeout waiting for a frame")), ms);
      this.waiters.push({ match, resolve: (f) => (clearTimeout(timer), resolve(f)) });
    });
  }

  async call(frame: Frame): Promise<Frame> {
    const rid = `r${++this.rid}`;
    const p = this.wait((f) => f.rid === rid && f.t !== "event");
    this.ws.send(JSON.stringify({ ...frame, rid }));
    const f = await p;
    if (f.t === "ack" && !f.ok) throw new Error(`${frame.t} failed: ${f.error?.code} ${f.error?.message ?? ""}`);
    return f;
  }

  async ping(): Promise<number> {
    const t0 = performance.now();
    const p = this.wait((f) => f.t === "pong");
    this.ws.send(JSON.stringify({ t: "ping" }));
    await p;
    return performance.now() - t0;
  }

  close() {
    this.ws.close();
  }
}

async function http(token: string, method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text.slice(0, 200)}`);
  return text ? JSON.parse(text) : null;
}

// At most 20 sends per second per member (server LIMITS.write): a fast (local) server needs a gap.
const PACE_MS = 60;
const pace = async (t0: number) => {
  const left = PACE_MS - (performance.now() - t0);
  if (left > 0) await new Promise((r) => setTimeout(r, left));
};

const pct = (xs: number[], p: number) => {
  const s = [...xs].sort((a, b) => a - b);
  return Math.round(s[Math.min(s.length - 1, Math.ceil((p / 100) * s.length) - 1)] * 10) / 10;
};
const stats = (xs: number[]) => ({ n: xs.length, p50: pct(xs, 50), p95: pct(xs, 95), max: pct(xs, 100) });
/** stats plus every sample in order, rounded to 0.1 ms. */
const timed = (xs: number[]) => ({ ...stats(xs), samples: xs.map((x) => Math.round(x * 10) / 10) });

const report: Record<string, any> = { env: ENV, base: BASE, at: new Date().toISOString() };
const failures: string[] = [];
const check = (name: string, ok: boolean, detail: string) => {
  if (!ok) failures.push(`${name}: ${detail}`);
};

const me = await http(OWNER, "GET", "/v1/me");
const bot = await http(BOT, "GET", "/v1/me");
report.health = await (await fetch(`${BASE}/v1/health`)).json();

let groups = (await http(OWNER, "GET", "/v1/groups")) as any[];
let smoke = groups.find((g) => g.group?.name === "smoke" && g.group?.kind === "group");
if (!smoke) smoke = await http(OWNER, "POST", "/v1/groups", { name: "smoke", memberIds: [bot.id] });
const groupId: string = smoke.group.id;
if (!smoke.group.memberIds.includes(bot.id)) await http(OWNER, "POST", `/v1/groups/${groupId}/members`, { memberId: bot.id });

const owner = new Conn(OWNER);
const botc = new Conn(BOT);
await owner.open();
await botc.open();
report.pingMs = stats(await Promise.all(Array.from({ length: 5 }, () => owner.ping())));

// Delivery: owner send -> bot live event.
const run = randomBytes(4).toString("hex");
const root = (await owner.call({ t: "send", groupId, title: `smoke ${run}`, clientMsgId: `smoke-${run}-root`, body: `冒烟测试 ${run}` })).result;
const topicId: string = root.message.topicId;
const delivery: number[] = [];
for (let i = -WARMUP; i < SAMPLES; i++) {
  const id = `smoke-${run}-d${i < 0 ? `w${-i}` : i}`;
  const got = botc.wait((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.clientMsgId === id);
  const t0 = performance.now();
  await owner.call({ t: "send", groupId, topicId, clientMsgId: id, body: `送达 ${i} 检索词${run}` });
  await got;
  if (i >= 0) delivery.push(performance.now() - t0);
  await pace(t0);
}
report.delivery = timed(delivery);
check("delivery", report.delivery.p95 < 300, `p95 ${report.delivery.p95} ms`);

// Search: a Chinese two-character word in this group and across all groups.
const searchMs: number[] = [];
let hits = 0;
for (let i = -2 * WARMUP; i < SAMPLES; i++) {
  const t0 = performance.now();
  const r = await owner.call({ t: "search", query: "送达", ...(i % 2 ? {} : { groupId }), limit: 20 });
  if (i < 0) continue;
  searchMs.push(performance.now() - t0);
  hits = Math.max(hits, r.hits?.length ?? 0);
}
report.search = { ...timed(searchMs), hits };
check("search", hits > 0, "no hits for 送达");
check("search", report.search.p95 < 300, `p95 ${report.search.p95} ms`);

// Catch-up: a new connection that fetches every group from seq 0.
{
  const c = new Conn(OWNER);
  const t0 = performance.now();
  await c.open();
  let events = 0;
  for (const g of c.welcome.groups as any[]) {
    let after = 0;
    for (;;) {
      const page = await c.call({ t: "fetch", groupId: g.group.id, afterSeq: after, limit: 500 });
      events += page.events.length;
      after = page.lastSeq;
      if (!page.hasMore) break;
    }
  }
  report.catchUp = { ms: Math.round(performance.now() - t0), groups: c.welcome.groups.length, events };
  c.close();
  check("catchUp", report.catchUp.ms < 2000, `${report.catchUp.ms} ms`);
}

// Reconnect: the bot misses 5 messages while offline and gets each once after hello with cursors.
{
  const cursor = (botc.welcome.groups as any[]).find((g) => g.group.id === groupId)?.lastSeq ?? 0;
  let last = cursor;
  botc.listeners.push((f) => {
    if (f.t === "event" && f.event.groupId === groupId) last = Math.max(last, f.event.seq);
  });
  await botc.ping();
  botc.close();
  const ids: string[] = [];
  for (let i = 0; i < 5; i++) {
    ids.push(`smoke-${run}-o${i}`);
    await owner.call({ t: "send", groupId, topicId, clientMsgId: ids[i], body: `离线 ${i}` });
  }
  const seen = new Map<string, number>();
  const re = new Conn(BOT);
  re.listeners.push((f) => {
    const evs = f.t === "page" ? f.events : f.t === "event" ? [f.event] : [];
    for (const e of evs) if (e.type === "message.created") seen.set(e.data.message.clientMsgId, (seen.get(e.data.message.clientMsgId) ?? 0) + 1);
  });
  await re.open({ [groupId]: last });
  let after = last;
  for (;;) {
    const page = await re.call({ t: "fetch", groupId, afterSeq: after, limit: 500 });
    after = page.lastSeq;
    if (!page.hasMore) break;
  }
  const counts = ids.map((id) => seen.get(id) ?? 0);
  report.reconnect = { missed: counts.filter((n) => n === 0).length, duplicated: counts.filter((n) => n > 1).length };
  check("reconnect", report.reconnect.missed === 0 && report.reconnect.duplicated === 0, JSON.stringify(report.reconnect));

  // Live notification: a bot message that mentions the owner.
  const id = `smoke-${run}-m`;
  const got = owner.wait((f) => f.t === "event" && f.event.data?.message?.clientMsgId === id);
  await re.call({ t: "send", groupId, topicId, clientMsgId: id, body: `@${me.name} 冒烟提醒`, mentions: [me.id] });
  const f = await got;
  report.notify = { reason: f.notify ?? null };
  check("notify", f.notify === "mention", `notify ${f.notify}`);
  re.close();
}

// Upload: --upload-mb MB, then read back size and SHA-256.
if (UPLOAD_MB > 0) {
  const size = Math.min(100_000_000, Math.round(UPLOAD_MB * 1_000_000));
  const bytes = randomBytes(size);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  const created = await http(OWNER, "POST", "/v1/files", { groupId, topicId, name: `smoke-${run}.bin`, size, mime: "application/octet-stream", sha256 });
  const t0 = performance.now();
  const put = await fetch(created.uploadUrl, { method: "PUT", headers: { authorization: `Bearer ${OWNER}` }, body: bytes });
  const putMs = Math.round(performance.now() - t0);
  const meta = put.ok ? await put.json() : null;
  const t1 = performance.now();
  const got = await fetch(`${BASE}/v1/files/${created.file.id}/content`, { headers: { authorization: `Bearer ${OWNER}` } });
  const back = new Uint8Array(await got.arrayBuffer());
  const getMs = Math.round(performance.now() - t1);
  const same = back.length === size && createHash("sha256").update(back).digest("hex") === sha256;
  report.upload = { bytes: size, status: put.status, ready: meta?.status === "ready", putMs, getMs, sameBytes: same };
  check("upload", put.ok && meta?.status === "ready" && same, `status ${put.status}, same ${same}`);
}

// Card v2 asks from the bot in the topic: each send resolves `requester` and `owners`.
{
  const card = (i: string) => ({
    v: 2,
    rev: 1,
    intent: "ask",
    title: `冒烟卡片 ${i}`,
    fallback: `冒烟卡片 ${i}：选一个`,
    ask: { id: `a${i}`, from: ["requester", "owners"] },
    blocks: [{ id: "pick", type: "decision", label: "选一个", options: [{ id: "a", label: "A" }, { id: "b", label: "B" }] }],
    actions: [{ id: "send", label: "发送", kind: "submit", style: "primary", inputs: ["pick"] }],
  });
  const bc = new Conn(BOT);
  await bc.open();
  const ackMs: number[] = [];
  const deliveryMs: number[] = [];
  try {
    for (let i = -WARMUP; i < SAMPLES; i++) {
      const k = i < 0 ? `w${-i}` : String(i);
      const id = `smoke-${run}-c${k}`;
      const got = owner.wait((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.clientMsgId === id);
      const t0 = performance.now();
      await bc.call({ t: "send", groupId, topicId, clientMsgId: id, body: `冒烟卡片 ${k}`, cardV2: card(k) });
      const ack = performance.now() - t0;
      await got;
      if (i >= 0) {
        ackMs.push(ack);
        deliveryMs.push(performance.now() - t0);
      }
      await pace(t0);
    }
    report.cards = { ack: timed(ackMs), delivery: timed(deliveryMs) };
    check("cards", report.cards.delivery.p95 < 300, `p95 ${report.cards.delivery.p95} ms`);
  } catch (err) {
    if (ackMs.length > 0) throw err;
    report.cards = { skipped: String((err as Error).message ?? err) };
  }
  bc.close();
}

owner.close();
report.failures = failures;
report.ok = failures.length === 0;
mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify(report, null, 2));
process.exit(report.ok ? 0 : 1);
