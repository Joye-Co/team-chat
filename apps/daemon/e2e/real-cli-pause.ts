#!/usr/bin/env bun
// GAPS SB-13: 接管 (pause) with the real Claude Code and Codex CLIs. `wrangler dev` (fresh local
// state) plus the real machine daemon with the real CLIs on this computer. Per CLI, a member
// mentions a bot whose CLI runs a shell loop that appends a time to ticks.txt every second (the
// loop runs in the CLI's tool process, in a group or session of its own). After a few ticks the
// member pauses the run for --pause-minutes (default 10), then hands it back. The check: while
// paused, every process of the run is stopped (state T) and no tick is written, the run is not
// marked stalled and keeps pausedBy, the machine stays connected; after the resume the loop goes
// on and the run ends with a result. Each sample goes to a JSON log.
//
// Usage (from the repo root):
//   bun apps/daemon/e2e/real-cli-pause.ts [--port 8795] [--pause-minutes 10] [--runtimes claude,codex]
//     [--out .tmp/real-cli-pause/report.json] [--keep]
// Needs: `bun install`, the daemon binary (built with cargo when missing), and the CLIs signed in.
// It uses the CLIs' own sign-in and spends a few model turns. Tokens stay in memory or in files of
// mode 600 inside the run folder; nothing is printed. Exit 0 when every CLI passes.
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const args = process.argv.slice(2);
const flag = (name: string) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
const PORT = Number(flag("--port") || 0) || 8795;
const PAUSE_MS = Math.round(Number(flag("--pause-minutes") ?? "10") * 60_000);
const KEEP = args.includes("--keep");
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = resolve(flag("--out") ?? join(ROOT, ".tmp/real-cli-pause/report.json"));
const TMP = join(ROOT, ".tmp", `real-cli-pause-${Date.now()}`);
const LABEL = "real-cli";
const TICKS = 40;
const SAMPLE_MS = 30_000;
const wanted = (flag("--runtimes") ?? "claude,codex").split(",").filter(Boolean);
const LOOP = `for i in $(seq 1 ${TICKS}); do date +%s >> ticks.txt; sleep 1; done`;
const PROMPT = [
  "Run this exact shell command once in the current folder, in the foreground, and wait until it",
  `finishes (it takes about ${TICKS} seconds; use the longest timeout your shell tool allows):`,
  "",
  LOOP,
  "",
  "Do not run anything else. Then answer with one line: the number of lines in ticks.txt.",
].join("\n");

type Sample = { at: number; phase: string; ticks: number; procs: { pid: number; stat: string; command: string }[]; run?: unknown; machine?: unknown };
const report: Record<string, any> = { base: BASE, tmp: TMP, pauseMs: PAUSE_MS, ticksWanted: TICKS, startedAt: new Date().toISOString(), runtimes: {} };
const children: { name: string; pid: number }[] = [];

mkdirSync(TMP, { recursive: true, mode: 0o700 });
mkdirSync(dirname(OUT), { recursive: true });

function log(s: string): void {
  console.log(`${new Date().toISOString()} ${s}`);
}

function start(name: string, cmd: string, argv: string[], opts: { cwd: string; env?: Record<string, string> }) {
  const file = join(TMP, `${name}.log`);
  const fd = openSync(file, "a");
  const child = spawn(cmd, argv, { cwd: opts.cwd, env: { ...process.env, ...opts.env }, stdio: ["ignore", fd, fd], detached: true });
  if (!child.pid) throw new Error(`${name} did not start`);
  children.push({ name, pid: child.pid });
  log(`${name} started (pid ${child.pid}, log ${file})`);
}

/** Stops the process groups this script started (wrangler runs workerd as a child). */
function stopAll(): void {
  for (const c of children.reverse()) {
    try {
      process.kill(-c.pid, "SIGTERM");
    } catch {
      // gone
    }
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function until<T>(what: string, fn: () => Promise<T | undefined | false> | T | undefined | false, ms = 30_000): Promise<T> {
  const t0 = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // retry
    }
    if (Date.now() - t0 > ms) throw new Error(`timed out: ${what}`);
    await sleep(300);
  }
}

async function call(method: string, path: string, opts: { token?: string; body?: unknown; headers?: Record<string, string> } = {}) {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${BASE}${path}`, { method, headers, body: opts.body === undefined ? undefined : JSON.stringify(opts.body) });
  const text = await res.text();
  let data: any = text;
  try {
    data = JSON.parse(text);
  } catch {
    // not JSON
  }
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${data?.error?.code ?? ""}`);
  return data;
}

/** The CLI's process group leader and every descendant, with their ps state. */
function tree(runDir: string): { pid: number; stat: string; command: string }[] {
  let root = 0;
  try {
    root = JSON.parse(readFileSync(join(runDir, "run.json"), "utf8")).pgid ?? 0;
  } catch {
    return [];
  }
  if (root <= 1) return [];
  const ps = spawnSync("ps", ["-axo", "pid=,ppid=,stat=,command="], { encoding: "utf8" });
  const rows = ps.stdout
    .split("\n")
    .map((l) => l.trim().match(/^(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/))
    .filter((m): m is RegExpMatchArray => m !== null)
    .map((m) => ({ pid: Number(m[1]), ppid: Number(m[2]), stat: m[3], command: m[4].slice(0, 120) }));
  const keep = new Set([root]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const r of rows) if (!keep.has(r.pid) && keep.has(r.ppid)) (keep.add(r.pid), (grew = true));
  }
  return rows.filter((r) => keep.has(r.pid)).map(({ pid, stat, command }) => ({ pid, stat, command }));
}

function ticks(runDir: string): number {
  try {
    return readFileSync(join(runDir, "work", "ticks.txt"), "utf8").split("\n").filter(Boolean).length;
  } catch {
    return 0;
  }
}

async function main() {
  const server = join(ROOT, "server");
  const state = join(TMP, "wrangler-state");
  const adminSecret = crypto.randomUUID();
  const runtimes = wanted.filter((rt) => {
    const found = Bun.which(rt);
    if (!found) report.runtimes[rt] = { skipped: `${rt} is not on PATH` };
    return found;
  });
  if (runtimes.length === 0) throw new Error("no real CLI found");

  const mig = spawnSync("bunx", ["wrangler", "d1", "migrations", "apply", "DB", "--local", "--persist-to", state], { cwd: server, encoding: "utf8", env: { ...process.env, CI: "1" } });
  if (mig.status !== 0) throw new Error(`d1 migrations failed: ${mig.stderr?.slice(-400)}`);
  start("wrangler", "bunx", ["wrangler", "dev", "--port", String(PORT), "--inspector-port", String(PORT + 1000), "--ip", "127.0.0.1", "--persist-to", state, "--var", `ADMIN_SECRET:${adminSecret}`], { cwd: server });
  await until("server health", async () => (await fetch(`${BASE}/v1/health`)).ok, 90_000);
  const boot = await call("POST", "/v1/admin/bootstrap", { headers: { "x-admin-secret": adminSecret }, body: { ownerName: "Joye" } });
  const owner = boot.token as string;
  const groupId = boot.group.id as string;
  const invite = await call("POST", "/v1/invites", { token: owner, body: { name: "Alice", kind: "human", level: "member" } });
  const alice = await call("POST", "/v1/auth/redeem", { body: { inviteCode: invite.code, deviceName: "real-cli-pause" } });
  const bots: Record<string, { member: { id: string; name: string } }> = {};
  for (const rt of runtimes) {
    bots[rt] = await call("POST", "/v1/bots", { token: owner, body: { name: `${rt}-pause`, role: "开发" } });
    await call("PUT", `/v1/bots/${bots[rt].member.id}/runner`, { token: owner, body: { runtime: rt, labels: [LABEL] } });
  }
  const machine = await call("POST", "/v1/machines", { token: owner, body: { name: "real-cli-mac", labels: [LABEL] } });
  const tokenFile = join(TMP, "machine-token");
  writeFileSync(tokenFile, machine.token, { mode: 0o600 });
  chmodSync(tokenFile, 0o600);

  const daemonBin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"), "debug", "daemon");
  if (!existsSync(daemonBin)) {
    const b = spawnSync("cargo", ["build", "-p", "daemon"], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p daemon failed");
  }
  const work = join(TMP, "work");
  const config = join(TMP, "daemon.toml");
  const lines = [`server_url = "${BASE}"`, `token_file = "${tokenFile}"`, 'name = "real-cli-mac"', `labels = ["${LABEL}"]`, `work_root = "${work}"`, "max_parallel_runs = 2", "min_free_gb = 1", ""];
  if (runtimes.includes("claude")) lines.push("[runtimes.claude]", 'kind = "claude"', `command = "${Bun.which("claude")}"`, 'allowed_tools = "Bash"', "");
  if (runtimes.includes("codex")) lines.push("[runtimes.codex]", 'kind = "codex"', `command = "${Bun.which("codex")}"`, 'sandbox = "workspace-write"', 'approval_policy = "never"', "");
  writeFileSync(config, lines.join("\n"));
  start("daemon", daemonBin, ["run", "--config", config], { cwd: TMP });
  await until("machine healthy", async () => ((await call("GET", "/v1/machines", { token: owner })) as any[]).find((m) => m.id === machine.machine.id && m.healthy), 60_000);
  log("daemon connected");

  const events: any[] = [];
  const ws = new WebSocket(`${BASE.replace("http", "ws")}/v1/ws`, { headers: { authorization: `Bearer ${alice.token}` } } as any);
  ws.addEventListener("message", (e) => {
    const f = JSON.parse(String(e.data));
    if (f.t === "event") events.push(f.event);
    if (f.t === "ack" || f.t === "welcome") events.push({ frame: f });
  });
  await until("socket open", () => ws.readyState === WebSocket.OPEN, 10_000);
  ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "real-cli-pause", version: "0" } }));
  await until("welcome", () => events.find((e) => e.frame?.t === "welcome"));

  const machineState = async () => {
    const m = ((await call("GET", "/v1/machines", { token: owner })) as any[]).find((x) => x.id === machine.machine.id);
    return m && { connected: m.connected, healthy: m.healthy, runningRuns: m.runningRuns };
  };
  const checkOne = async (rt: string) => {
    const bot = bots[rt];
    const r: Record<string, any> = { samples: [] as Sample[] };
    report.runtimes[rt] = r;
    const rid = `r-${rt}`;
    ws.send(JSON.stringify({ t: "send", rid, groupId, clientMsgId: crypto.randomUUID(), body: `@${bot.member.name} ${PROMPT}`, mentions: [bot.member.id] }));
    const ack = await until(`send ack (${rt})`, () => events.find((e) => e.frame?.rid === rid));
    if (!ack.frame.ok) throw new Error(`send failed (${rt})`);
    const topicId = ack.frame.result.message.topicId as string;
    const runOf = () => events.filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run).at(-1);
    const run0 = await until(`run (${rt})`, runOf, 60_000);
    const runDir = join(work, "runs", run0.id);
    r.runId = run0.id;
    const sample = async (phase: string) => {
      const run = await call("GET", `/v1/runs/${run0.id}`, { token: alice.token });
      const s: Sample = { at: Date.now(), phase, ticks: ticks(runDir), procs: tree(runDir), run: { status: run.status, pausedBy: run.pausedBy ?? null }, machine: await machineState() };
      r.samples.push(s);
      return s;
    };
    // A CLI that cannot start (for example not signed in) ends the run before the first tick.
    const ended = () => ["done", "failed", "cancelled"].includes(runOf()?.status);
    await until(`first ticks (${rt})`, () => ticks(runDir) >= 3 || ended(), 5 * 60_000);
    if (ended()) throw new Error(`the run ended before the loop started: ${runOf().status} ${runOf().error ?? ""}`.trim());
    await sample("before");
    const paused = await call("POST", `/v1/runs/${run0.id}/pause`, { token: alice.token, body: {} });
    r.pausedAt = Date.now();
    r.pausedBy = paused.pausedBy;
    log(`${rt}: paused run ${run0.id}`);
    await sleep(3000);
    const first = await sample("paused");
    let frozen = true;
    let allStopped = first.procs.length > 0;
    let stayedPaused = true;
    let connected = true;
    for (let t = 3000 + SAMPLE_MS; t <= PAUSE_MS; t += SAMPLE_MS) {
      await sleep(SAMPLE_MS);
      const s = await sample("paused");
      if (s.ticks !== first.ticks) frozen = false;
      if (!s.procs.every((p) => p.stat.startsWith("T"))) allStopped = false;
      if ((s.run as any).pausedBy !== paused.pausedBy || (s.run as any).status !== "running") stayedPaused = false;
      if (!(s.machine as any)?.connected) connected = false;
    }
    await sleep(Math.max(0, r.pausedAt + PAUSE_MS - Date.now()));
    if (!first.procs.every((p) => p.stat.startsWith("T"))) allStopped = false;
    const pausedTicks = ticks(runDir);
    if (pausedTicks !== first.ticks) frozen = false;
    const resumed = await call("POST", `/v1/runs/${run0.id}/resume`, { token: alice.token, body: {} });
    r.resumedAt = Date.now();
    log(`${rt}: resumed after ${Math.round((r.resumedAt - r.pausedAt) / 1000)} s`);
    await sleep(5000);
    const after = await sample("resumed");
    const end = await until(`run end (${rt})`, () => ["done", "failed", "cancelled"].includes(runOf()?.status) && runOf(), 15 * 60_000);
    await sample("ended");
    const result = events
      .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === bot.member.id)
      .map((e) => e.data.message)
      .at(-1);
    const stalled = events.some((e) => e.type === "run.updated" && e.data.run.id === run0.id && e.data.run.status === "stalled");
    Object.assign(r, {
      topicId,
      resumedPausedBy: resumed.pausedBy ?? null,
      ticksBefore: r.samples[0].ticks,
      ticksWhilePaused: pausedTicks,
      ticksAfterResume: after.ticks,
      ticksAtEnd: ticks(runDir),
      frozen,
      allStopped,
      stayedPaused,
      connected,
      stalled,
      status: end.status,
      error: end.error ?? null,
      durationMs: end.durationMs ?? null,
      usage: end.usage ?? null,
      result: result?.body?.slice(0, 400) ?? null,
    });
    r.ok = frozen && allStopped && stayedPaused && connected && !stalled && !r.resumedPausedBy && after.ticks > pausedTicks && end.status === "done";
    log(`${rt}: ${r.ok ? "ok" : "FAILED"} (ticks ${r.ticksBefore} -> ${pausedTicks} paused -> ${r.ticksAtEnd}, status ${end.status})`);
  };
  const settled = await Promise.allSettled(runtimes.map(checkOne));
  settled.forEach((s, i) => {
    if (s.status === "rejected") Object.assign((report.runtimes[runtimes[i]] ??= {}), { ok: false, error: String(s.reason?.message ?? s.reason) });
  });
  ws.close();
  report.ok = runtimes.every((rt) => report.runtimes[rt]?.ok === true);
}

let code = 0;
try {
  await main();
  if (!report.ok) code = 1;
} catch (e) {
  report.ok = false;
  report.error = String((e as Error).message ?? e);
  code = 1;
} finally {
  stopAll();
  report.endedAt = new Date().toISOString();
  writeFileSync(OUT, `${JSON.stringify(report, null, 2)}\n`);
  log(`report: ${OUT}`);
  if (!KEEP) rmSync(join(TMP, "wrangler-state"), { recursive: true, force: true });
}
process.exit(code);
