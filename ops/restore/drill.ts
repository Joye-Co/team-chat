#!/usr/bin/env bun
// Local restore drill. Starts a fresh local stack A, fills it with real data through the public
// API, runs the archive, copies the archive to a folder and stops A. Then starts a stack B with
// empty D1 and Durable Objects (A's R2 bucket, which holds the file bytes, stays),
// restores the whole env from that folder, and compares what members see in B with what they
// saw in A (messages, topics, 产出 and their files). A message deleted after the first archive
// run must be gone from every archive file and from B. Exits 0 when all match.
// Report: ops/restore/.drill/report.json (no secrets).
//
//   bun ops/restore/drill.ts [--port 8798]
//
// It runs `wrangler dev` itself (persisting under ops/restore/.drill/) and stops it by PID.
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, openSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { AdminApi, ROOT, dirSource, flag, gunzipText, httpSource, parseArgs } from "./lib";
import { fetchArchive, restoreEnv } from "./restore";

const { flags } = parseArgs(["drill", ...process.argv.slice(2)]);
const PORT = Number(flag(flags, "port") ?? 8798);
const BASE = `http://127.0.0.1:${PORT}`;
const WORK = join(ROOT, "ops/restore/.drill");
const SERVER = join(ROOT, "server");
const WRANGLER = [join(SERVER, "node_modules/.bin/wrangler"), join(ROOT, "node_modules/.bin/wrangler")].find(existsSync) ?? "wrangler";

rmSync(WORK, { recursive: true, force: true });
mkdirSync(WORK, { recursive: true, mode: 0o700 });
const secret = randomBytes(24).toString("base64url");
const envFile = join(WORK, "admin.env");
writeFileSync(envFile, `ADMIN_SECRET=${secret}\n`, { mode: 0o600 });
const admin = new AdminApi(BASE, secret);

async function sh(cmd: string[], log: string): Promise<void> {
  const fd = openSync(join(WORK, log), "a");
  const p = Bun.spawn(cmd, { cwd: SERVER, stdout: fd, stderr: fd, env: { ...process.env, CI: "1" } });
  if ((await p.exited) !== 0) throw new Error(`${cmd.slice(0, 4).join(" ")} failed; see ${log}`);
}

async function startStack(name: string): Promise<Bun.Subprocess> {
  const persist = join(WORK, name);
  await sh([WRANGLER, "d1", "migrations", "apply", "DB", "--local", "--persist-to", persist], `${name}.log`);
  const fd = openSync(join(WORK, `${name}.log`), "a");
  const proc = Bun.spawn(
    [
      WRANGLER, "dev", "--ip", "127.0.0.1", "--port", String(PORT), "--inspector-port", String(PORT + 1000),
      "--persist-to", persist, "--env-file", envFile, "--show-interactive-dev-session=false",
    ],
    { cwd: SERVER, stdout: fd, stderr: fd, stdin: "ignore", env: { ...process.env, CI: "1" } },
  );
  const deadline = Date.now() + 90_000;
  for (;;) {
    try {
      const r = await fetch(`${BASE}/v1/health`);
      if (r.ok) return proc;
    } catch {
      // not up yet
    }
    if (proc.exitCode !== null) throw new Error(`wrangler dev for ${name} exited; see ${name}.log`);
    if (Date.now() > deadline) {
      await stopStack(proc);
      throw new Error(`wrangler dev for ${name} did not start; see ${name}.log`);
    }
    await Bun.sleep(500);
  }
}

async function stopStack(proc: Bun.Subprocess): Promise<void> {
  if (proc.exitCode !== null) return;
  proc.kill("SIGTERM");
  const t = setTimeout(() => proc.kill("SIGKILL"), 10_000);
  await proc.exited;
  clearTimeout(t);
}

async function http<T>(path: string, opts: { token?: string; body?: unknown; method?: string } = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${BASE}${path}`, {
    method: opts.method ?? (opts.body !== undefined ? "POST" : "GET"),
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${path}: ${res.status} ${text.slice(0, 200)}`);
  return JSON.parse(text) as T;
}

class Ws {
  private frames: any[] = [];
  private waiters: { pred: (f: any) => boolean; resolve: (f: any) => void }[] = [];
  private n = 0;
  private constructor(private readonly ws: WebSocket) {
    ws.addEventListener("message", (e) => {
      const f = JSON.parse(String(e.data));
      this.frames.push(f);
      this.waiters = this.waiters.filter((w) => (w.pred(f) ? (w.resolve(f), false) : true));
    });
  }
  static async open(token: string): Promise<{ ws: Ws; welcome: any }> {
    const sock = new WebSocket(`${BASE.replace("http", "ws")}/v1/ws`, { headers: { authorization: `Bearer ${token}` } } as any);
    await new Promise((res, rej) => {
      sock.addEventListener("open", res, { once: true });
      sock.addEventListener("error", rej, { once: true });
    });
    const ws = new Ws(sock);
    const welcome = await ws.call({ t: "hello", protocol: 1, client: { name: "drill", version: "1" } }, (f) => f.t === "welcome");
    return { ws, welcome };
  }
  wait(pred: (f: any) => boolean, ms = 10_000): Promise<any> {
    const hit = this.frames.find(pred);
    if (hit) return Promise.resolve(hit);
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("timeout waiting for frame")), ms);
      this.waiters.push({ pred, resolve: (f) => (clearTimeout(t), resolve(f)) });
    });
  }
  call(frame: Record<string, unknown>, pred?: (f: any) => boolean): Promise<any> {
    const rid = `d${++this.n}`;
    this.ws.send(JSON.stringify({ ...frame, rid }));
    return this.wait(pred ?? ((f) => f.rid === rid));
  }
  async ok(frame: Record<string, unknown>): Promise<any> {
    for (let attempt = 0; ; attempt++) {
      const r = await this.call(frame);
      if (r.ok) return r.result;
      if (r.error?.code === "rate_limited" && attempt < 20) {
        await Bun.sleep(1000);
        continue;
      }
      throw new Error(`${frame.t} failed: ${JSON.stringify(r.error)}`);
    }
  }
  close() {
    this.ws.close(1000, "done");
  }
}

interface FileRef {
  fileId: string;
  name: string;
  size: number;
  mime: string;
  sha256: string;
}

async function upload(token: string, groupId: string, text: string, name: string, mime: string): Promise<FileRef> {
  const bytes = new TextEncoder().encode(text);
  const sha256 = new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
  const f = await http<any>("/v1/files", { token, body: { groupId, name, size: bytes.length, mime, sha256 } });
  const res = await fetch(`${BASE}/v1/files/${f.file.id}/content`, {
    method: "PUT",
    headers: { authorization: `Bearer ${token}` },
    body: bytes,
  });
  if (!res.ok) throw new Error(`upload ${name}: ${res.status}`);
  return { fileId: f.file.id as string, name, size: bytes.length, mime, sha256 };
}

/** Deleted after the first archive run: no archive file and no restored event may keep it. */
const LATE_DELETE = "DRILL-DELETED-AFTER-ARCHIVE";

interface Seeded {
  note: FileRef;
  late: { groupId: string; messageId: string };
  ownerToken: string;
  memberToken: string;
  botToken: string;
}

async function seed(): Promise<Seeded> {
  const boot = await admin.json<{ token: string; member: { id: string } }>("/v1/admin/bootstrap", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ownerName: "Drill Owner" }),
  });
  const ownerToken = boot.token;
  const bot = await http<{ token: string; member: { id: string } }>("/v1/bots", { token: ownerToken, body: { name: "Drill Bot", role: "tests" } });
  const invite = await http<{ code: string }>("/v1/invites", { token: ownerToken, body: { name: "Intern", kind: "human", level: "member" } });
  const member = await http<{ token: string; member: { id: string } }>("/v1/auth/redeem", { body: { inviteCode: invite.code, deviceName: "drill" } });
  const group = await http<any>("/v1/groups", { token: ownerToken, body: { name: "Drill", memberIds: [bot.member.id, member.member.id] } });
  const gid = group.group.id as string;
  const dm = await http<any>("/v1/dms", { token: ownerToken, body: { memberId: member.member.id } });

  const { ws: o } = await Ws.open(ownerToken);
  const { ws: m } = await Ws.open(member.token);
  const { ws: b } = await Ws.open(bot.token);
  const id = () => crypto.randomUUID();
  for (let t = 0; t < 5; t++) {
    const root = await o.ok({ t: "send", groupId: gid, clientMsgId: id(), body: `话题 ${t}\n第一条消息` });
    const topicId = root.topic.id;
    for (let i = 0; i < 6; i++) {
      await m.ok({ t: "send", groupId: gid, topicId, clientMsgId: id(), body: `回复 ${t}.${i}`, mentions: i === 0 ? [boot.member.id] : [] });
    }
    const streamed = await b.ok({ t: "send", groupId: gid, topicId, clientMsgId: id(), body: "", streaming: true });
    await b.ok({ t: "edit", groupId: gid, messageId: streamed.message.id, body: `结果 ${t}`, streaming: false });
    const oops = await m.ok({ t: "send", groupId: gid, topicId, clientMsgId: id(), body: "删掉这条" });
    await m.ok({ t: "delete", groupId: gid, messageId: oops.message.id });
    await o.ok({ t: "topic.update", groupId: gid, topicId, status: t % 2 ? "done" : "in_progress", title: `话题 ${t} (改)` });
  }
  // A file attachment and a 产出 with two versions: their room records have no event of their own
  // (files) or are rebuilt from artifact events (产出).
  const topicId = (await o.ok({ t: "send", groupId: gid, clientMsgId: id(), body: "产出话题" })).topic.id;
  const page1 = await upload(ownerToken, gid, "<h1>v1</h1>", "index.html", "text/html");
  const page2 = await upload(ownerToken, gid, "<h1>v2</h1>", "index.html", "text/html");
  const note = await upload(ownerToken, gid, "附件内容", "note.txt", "text/plain");
  await o.ok({ t: "send", groupId: gid, topicId, clientMsgId: id(), body: "附件", attachments: [note] });
  const art = await http<any>("/v1/artifacts", {
    token: ownerToken,
    body: { groupId: gid, topicId, title: "页面", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: page1.fileId }] },
  });
  await http<any>(`/v1/artifacts/${art.artifact.id}/versions`, {
    token: ownerToken,
    body: { entry: "index.html", files: [{ path: "index.html", fileId: page2.fileId }], note: "第二版" },
  });
  const late = await m.ok({ t: "send", groupId: gid, topicId, clientMsgId: id(), body: LATE_DELETE });
  await o.ok({ t: "send", groupId: dm.group.id, clientMsgId: id(), body: "私聊" });
  await m.ok({ t: "send", groupId: dm.group.id, clientMsgId: id(), body: "收到" });
  for (const ws of [o, m, b]) ws.close();
  return { note, late: { groupId: gid, messageId: late.message.id }, ownerToken, memberToken: member.token, botToken: bot.token };
}

/** Everything one member sees: members, groups, every event and every topic's history. */
async function view(token: string): Promise<any> {
  const members = await http<any[]>("/v1/members", { token });
  const groups = await http<any[]>("/v1/groups", { token });
  const { ws } = await Ws.open(token);
  const events: Record<string, any[]> = {};
  const history: Record<string, any[]> = {};
  for (const g of groups) {
    const gid = g.group.id;
    events[gid] = [];
    let after = 0;
    for (;;) {
      const page = await ws.call({ t: "fetch", groupId: gid, afterSeq: after, limit: 500 });
      events[gid].push(...page.events);
      if (!page.hasMore || page.events.length === 0) break;
      after = page.events.at(-1).seq;
    }
    for (const e of events[gid]) {
      if (e.type !== "topic.created") continue;
      const h = await ws.call({ t: "history", groupId: gid, topicId: e.data.topic.id, limit: 500 });
      history[e.data.topic.id] = h.messages;
    }
  }
  ws.close();
  // 产出 of every group: list, versions and the entry file of each version.
  const artifacts: Record<string, any[]> = {};
  for (const g of groups) {
    const gid = g.group.id;
    const list = await http<any[]>(`/v1/artifacts?groupId=${encodeURIComponent(gid)}`, { token });
    artifacts[gid] = [];
    for (const a of list) {
      const full = await http<any>(`/v1/artifacts/${a.id}`, { token });
      const entries: string[] = [];
      for (const v of full.versions) {
        const res = await fetch(`${BASE}/v1/artifacts/${a.id}/versions/${v.version}/files/${v.entry}`, {
          headers: { authorization: `Bearer ${token}` },
        });
        entries.push(`${res.status} ${await res.text()}`);
      }
      artifacts[gid].push({ ...full, entries });
    }
  }
  // Group read cursors live in each member's gateway and are not archived; compare the rest.
  const strip = (g: any) => {
    const { readSeq, unread, mentions, ...rest } = g;
    return rest;
  };
  return { members, groups: groups.map(strip), events, history, artifacts };
}

function diff(a: unknown, b: unknown, path = "$", out: string[] = []): string[] {
  if (out.length > 20) return out;
  if (typeof a !== typeof b || Array.isArray(a) !== Array.isArray(b) || (a === null) !== (b === null)) {
    out.push(`${path}: ${JSON.stringify(a)?.slice(0, 80)} != ${JSON.stringify(b)?.slice(0, 80)}`);
  } else if (a && typeof a === "object") {
    const keys = new Set([...Object.keys(a as object), ...Object.keys(b as object)]);
    for (const k of keys) diff((a as any)[k], (b as any)[k], `${path}.${k}`, out);
  } else if (a !== b) {
    out.push(`${path}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`);
  }
  return out;
}

async function main() {
  const report: Record<string, unknown> = { startedAt: new Date().toISOString(), port: PORT };
  let proc = await startStack("a");
  let seeded: Seeded;
  let before: { owner: any; member: any };
  try {
    seeded = await seed();
    const run = await admin.json<any>("/v1/admin/archive/run", { method: "POST" });
    const { ws: lw } = await Ws.open(seeded.memberToken);
    await lw.ok({ t: "delete", groupId: seeded.late.groupId, messageId: seeded.late.messageId });
    lw.close();
    const run2 = await admin.json<any>("/v1/admin/archive/run", { method: "POST" });
    report.archive = {
      files: run.files.length + run2.files.length,
      events: run.events + run2.events,
      complete: run.complete && run2.complete,
      scrubbed: run2.scrubbed.length,
    };
    before = { owner: await view(seeded.ownerToken), member: await view(seeded.memberToken) };
    report.archivedObjects = await fetchArchive(httpSource(admin), join(WORK, "archive"));
    const archive = dirSource(join(WORK, "archive"));
    let leaks = 0;
    for (const k of await archive.list("archive/groups/")) {
      if (gunzipText(await archive.get(k)).includes(LATE_DELETE)) leaks++;
    }
    report.deletedTextInArchive = leaks;
  } finally {
    await stopStack(proc);
  }

  // Stack B gets empty D1 and Durable Objects but keeps A's R2 bucket: uploaded file bytes and
  // 产出 files are not in the archive, they stay in the env's bucket (docs/RUNBOOK.md).
  mkdirSync(join(WORK, "b", "v3"), { recursive: true });
  symlinkSync(join(WORK, "a", "v3", "r2"), join(WORK, "b", "v3", "r2"));
  proc = await startStack("b");
  try {
    const health = await http<any>("/v1/health");
    const fresh = await admin.json<{ tables: Record<string, number> }>("/v1/admin/import/d1");
    if (fresh.tables.members !== 0) throw new Error("stack B is not empty");
    const restored = await restoreEnv(dirSource(join(WORK, "archive")), admin);
    report.restore = {
      d1Rows: restored.d1.rows,
      d1Inserted: restored.d1.inserted,
      skippedTables: restored.d1.skippedTables,
      groups: restored.groups,
      version: health.version,
    };
    const after = { owner: await view(seeded.ownerToken), member: await view(seeded.memberToken) };
    const differences = diff(before, after);
    const eventCount = Object.values(before.owner.events as Record<string, any[]>).reduce((n, l) => n + l.length, 0);
    report.compared = {
      groups: before.owner.groups.length,
      events: eventCount,
      topics: Object.keys(before.owner.history).length,
      members: before.owner.members.length,
      artifacts: Object.values(before.owner.artifacts as Record<string, any[]>).reduce((n, l) => n + l.length, 0),
    };
    report.differences = differences;
    // Writes keep working after the restore and continue the seq.
    const { ws, welcome } = await Ws.open(seeded.memberToken);
    const g = welcome.groups.find((x: any) => x.group.kind === "group" && x.group.name === "Drill");
    const sent = await ws.ok({ t: "send", groupId: g.group.id, clientMsgId: crypto.randomUUID(), body: "恢复后" });
    ws.close();
    // An attachment uploaded before the archive is still a ready file of the group, and the
    // 产出 takes a new version from it.
    const { ws: ow } = await Ws.open(seeded.ownerToken);
    const restoredArtifacts = after.owner.artifacts[g.group.id] as any[];
    const art = restoredArtifacts[0];
    const oldFile = art?.versions.at(-1)?.files[0]?.fileId;
    const again = await ow.ok({
      t: "send",
      groupId: g.group.id,
      topicId: art?.artifact.topicId,
      clientMsgId: crypto.randomUUID(),
      body: "旧附件",
      attachments: [seeded.note],
    });
    ow.close();
    const v3 = await http<any>(`/v1/artifacts/${art.artifact.id}/versions`, {
      token: seeded.ownerToken,
      body: { entry: "index.html", files: [{ path: "index.html", fileId: oldFile }] },
    });
    report.artifactsAfterRestore = {
      artifacts: restoredArtifacts.length,
      versions: art?.versions.length ?? 0,
      reattached: again.message.attachments?.length ?? 0,
      newVersion: v3.version.version,
    };
    // A send without topicId writes topic.created and then message.created.
    report.writeAfterRestore = { seq: sent.message.createdSeq, expected: g.lastSeq + 2 };
    report.ok =
      differences.length === 0 &&
      sent.message.createdSeq === g.lastSeq + 2 &&
      eventCount > 0 &&
      restoredArtifacts.length === 1 &&
      art.versions.length === 2 &&
      v3.version.version === 3 &&
      report.deletedTextInArchive === 0 &&
      (report.archive as { scrubbed: number }).scrubbed > 0 &&
      !JSON.stringify(after).includes(LATE_DELETE) &&
      again.message.attachments?.length === 1;
  } finally {
    await stopStack(proc);
  }
  report.finishedAt = new Date().toISOString();
  writeFileSync(join(WORK, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  rmSync(envFile, { force: true });
  console.log(JSON.stringify(report, null, 2));
  process.exit(report.ok ? 0 : 1);
}

if (!existsSync(SERVER)) throw new Error("run from the repository");
main().catch((err) => {
  console.error(`drill failed: ${err instanceof Error ? err.message : err}`);
  rmSync(envFile, { force: true });
  process.exit(1);
});
