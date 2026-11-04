#!/usr/bin/env bun
// Local end-to-end check of the backend data the desktop pages read (v13/wire) against
// `wrangler dev` of this checkout (fresh local state) and the real machine daemon with the fake
// Claude Code CLI. It makes the owner, a runner-backed bot "Jojo", an SDK bot "Pixel" and a
// machine that takes one run at a time, starts the daemon, then runs the Rust live test
// `provider::wire_live` (the chat-core provider the app uses) with the tokens in mode-600 files.
// Stops every process it started by PID.
//
// Usage (from the repo root): bun apps/desktop/e2e/wire-e2e.ts [--port 8806]
// Needs `bun install`; builds the daemon with cargo when it is missing (uses CARGO_TARGET_DIR).
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, openSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const args = process.argv.slice(2);
const PORT = Number(args[args.indexOf("--port") + 1] || 0) || 8806;
const BASE = `http://127.0.0.1:${PORT}`;
// Scratch, logs and token files stay in this worktree (`.tmp/` is ignored).
mkdirSync(join(ROOT, ".tmp"), { recursive: true });
const TMP = mkdtempSync(join(ROOT, ".tmp", "wire-e2e-"));
const LABEL = "e2e";
const children: { name: string; pid: number }[] = [];

function log(s: string) {
  console.log(`- ${s}`);
}

function secretFile(name: string, value: string): string {
  const p = join(TMP, name);
  writeFileSync(p, value, { mode: 0o600 });
  chmodSync(p, 0o600);
  return p;
}

function start(name: string, cmd: string, argv: string[], cwd: string) {
  const out = openSync(join(TMP, `${name}.log`), "a");
  const child = spawn(cmd, argv, { cwd, env: process.env, stdio: ["ignore", out, out], detached: true });
  if (!child.pid) throw new Error(`${name} did not start`);
  children.push({ name, pid: child.pid });
  log(`${name} started (pid ${child.pid}, log ${join(TMP, `${name}.log`)})`);
}

function stopAll() {
  for (const c of children.reverse()) {
    try {
      // The process group we started (wrangler runs workerd as a child).
      process.kill(-c.pid, "SIGTERM");
    } catch {
      // gone
    }
  }
}

async function until<T>(what: string, fn: () => Promise<T | undefined | false>, ms = 60_000): Promise<T> {
  const t0 = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // retry
    }
    if (Date.now() - t0 > ms) throw new Error(`timed out: ${what}`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

async function call(method: string, path: string, opts: { token?: string; body?: unknown; headers?: Record<string, string> } = {}) {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${BASE}${path}`, { method, headers, body: opts.body === undefined ? undefined : JSON.stringify(opts.body) });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}`);
  return JSON.parse(text);
}

async function main() {
  const server = join(ROOT, "server");
  const state = join(TMP, "wrangler-state");
  // A throwaway admin secret for this fresh local state only.
  const adminSecret = crypto.randomUUID();
  const mig = spawnSync("bunx", ["wrangler", "d1", "migrations", "apply", "DB", "--local", "--persist-to", state], {
    cwd: server,
    encoding: "utf8",
    env: { ...process.env, CI: "1" },
  });
  if (mig.status !== 0) throw new Error(`d1 migrations failed: ${mig.stderr?.slice(-400)}`);
  start(
    "wrangler",
    "bunx",
    ["wrangler", "dev", "--port", String(PORT), "--inspector-port", String(PORT + 1000), "--ip", "127.0.0.1", "--persist-to", state, "--var", `ADMIN_SECRET:${adminSecret}`],
    server,
  );
  await until("server health", async () => (await fetch(`${BASE}/v1/health`)).ok, 120_000);
  log("server is up");

  const boot = await call("POST", "/v1/admin/bootstrap", { headers: { "x-admin-secret": adminSecret }, body: { ownerName: "Joye" } });
  const owner = boot.token as string;
  // Jojo: runner-backed (the 首页 desk note, 排队 and 重试). Pixel: an SDK bot that makes a page
  // with declared events (页面里的回应).
  const bot = await call("POST", "/v1/bots", { token: owner, body: { name: "Jojo", role: "助理" } });
  await call("PUT", `/v1/bots/${bot.member.id}/runner`, { token: owner, body: { runtime: "claude", labels: [LABEL] } });
  const pageBot = await call("POST", "/v1/bots", { token: owner, body: { name: "Pixel", role: "设计" } });
  const botToken = secretFile("bot-token", bot.token);
  const pageBotToken = secretFile("page-bot-token", pageBot.token);
  const machine = await call("POST", "/v1/machines", { token: owner, body: { name: "e2e-mac", labels: [LABEL] } });
  const machineToken = secretFile("machine-token", machine.token);
  const ownerToken = secretFile("owner-token", owner);
  log("owner, bots, runner and machine made");

  const daemonBin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"), "debug", "daemon");
  if (!existsSync(daemonBin)) {
    const b = spawnSync("cargo", ["build", "-p", "daemon"], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p daemon failed");
  }
  const fake = join(ROOT, "apps/daemon/tests/fake/fake_claude.py");
  chmodSync(fake, 0o755);
  const claudeHome = join(TMP, "claude-home");
  mkdirSync(claudeHome, { recursive: true });
  const config = join(TMP, "daemon.toml");
  writeFileSync(
    config,
    [
      `server_url = "${BASE}"`,
      `token_file = "${machineToken}"`,
      `name = "e2e-mac"`,
      `labels = ["${LABEL}"]`,
      `work_root = "${join(TMP, "work")}"`,
      // One run at a time, so a second mention waits (排队).
      "max_parallel_runs = 1",
      "min_free_gb = 1",
      "",
      "[runtimes.claude]",
      'kind = "claude"',
      `command = "${fake}"`,
      `env = { CLAUDE_CONFIG_DIR = "${claudeHome}" }`,
      "",
    ].join("\n"),
  );
  start("daemon", daemonBin, ["run", "--config", config], TMP);
  await until("machine healthy", async () => {
    const list = (await call("GET", "/v1/machines", { token: owner })) as any[];
    return list.find((m) => m.id === machine.machine.id && m.healthy);
  });
  log("daemon connected and healthy");

  const test = spawnSync(
    "cargo",
    ["test", "--locked", "-p", "desktop", "wire_live", "--", "--ignored", "--nocapture", "--test-threads=1"],
    {
      cwd: ROOT,
      stdio: "inherit",
      env: {
        ...process.env,
        TC_LIVE_URL: BASE,
        TC_LIVE_TOKEN: ownerToken,
        TC_LIVE_BOT: bot.member.id,
        TC_LIVE_BOT_TOKEN: botToken,
        TC_LIVE_PAGE_BOT: pageBot.member.id,
        TC_LIVE_PAGE_BOT_TOKEN: pageBotToken,
        TC_LIVE_OUT: TMP,
      },
    },
  );
  if (test.status !== 0) throw new Error("wire_live failed");
  log(`wire_live passed; report ${join(TMP, "wire-live.json")}`);
}

let code = 0;
try {
  await main();
} catch (err) {
  code = 1;
  console.error(`FAILED: ${String(err)}`);
} finally {
  stopAll();
  console.log(`logs: ${TMP}`);
}
process.exit(code);
