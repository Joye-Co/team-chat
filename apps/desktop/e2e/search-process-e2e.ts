#!/usr/bin/env bun
// Local end-to-end check of desktop search and the process view against `wrangler dev` of this
// checkout (fresh local state) and the real machine daemon with the fake Claude Code CLI.
// It makes the owner, a runner-backed bot "dev" and a machine, starts the daemon, then runs the
// Rust live test `provider::search_live` (the chat-core provider the app uses) with the owner's
// token in a mode-600 file. Stops every process it started by PID.
//
// Usage (from the repo root): bun apps/desktop/e2e/search-process-e2e.ts [--port 8804]
// Needs `bun install`; builds the daemon with cargo when it is missing (uses CARGO_TARGET_DIR).
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, openSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const args = process.argv.slice(2);
const PORT = Number(args[args.indexOf("--port") + 1] || 0) || 8804;
const BASE = `http://127.0.0.1:${PORT}`;
const TMP = mkdtempSync(join(tmpdir(), "search-process-e2e-"));
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
  const bot = await call("POST", "/v1/bots", { token: owner, body: { name: "dev", role: "开发" } });
  await call("PUT", `/v1/bots/${bot.member.id}/runner`, { token: owner, body: { runtime: "claude", labels: [LABEL] } });
  const machine = await call("POST", "/v1/machines", { token: owner, body: { name: "e2e-mac", labels: [LABEL] } });
  const machineToken = secretFile("machine-token", machine.token);
  const ownerToken = secretFile("owner-token", owner);
  log("owner, bot runner and machine made");

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
      "max_parallel_runs = 2",
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
    ["test", "-p", "desktop", "search_live", "--", "--ignored", "--nocapture", "--test-threads=1"],
    {
      cwd: ROOT,
      stdio: "inherit",
      env: { ...process.env, TC_LIVE_URL: BASE, TC_LIVE_TOKEN: ownerToken, TC_LIVE_BOT: bot.member.id, TC_LIVE_OUT: TMP },
    },
  );
  if (test.status !== 0) throw new Error("search_live failed");
  log("search_live passed");
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
