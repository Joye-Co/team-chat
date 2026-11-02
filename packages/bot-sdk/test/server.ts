// Starts a local `wrangler dev` of server/ with its own D1 and Durable Object storage in a temp
// directory, so tests never touch another stack. Stopped by the PID it started.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, openSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const SERVER_DIR = resolve(import.meta.dir, "../../../server");

export interface LocalServer {
  url: string;
  port: number;
  adminSecret: string;
  dir: string;
  logFile: string;
  start(): Promise<void>;
  stop(): Promise<void>;
  dispose(): Promise<void>;
}

async function waitHealthy(url: string, ms: number): Promise<void> {
  const until = Date.now() + ms;
  for (;;) {
    try {
      const r = await fetch(`${url}/v1/health`, { signal: AbortSignal.timeout(2000) });
      if (r.ok) return;
    } catch {
      // not up yet
    }
    if (Date.now() > until) throw new Error(`server at ${url} did not become healthy`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

async function waitDown(url: string, ms: number): Promise<void> {
  const until = Date.now() + ms;
  for (;;) {
    try {
      await fetch(`${url}/v1/health`, { signal: AbortSignal.timeout(500) });
    } catch {
      return;
    }
    if (Date.now() > until) throw new Error(`server at ${url} did not stop`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

export function localServer(port: number): LocalServer {
  const dir = mkdtempSync(join(tmpdir(), "bot-sdk-e2e-"));
  const persist = join(dir, "state");
  const logFile = join(dir, "wrangler.log");
  const adminSecret = Buffer.from(crypto.getRandomValues(new Uint8Array(24))).toString("base64url");
  const varsFile = join(dir, "dev.vars");
  writeFileSync(varsFile, `ADMIN_SECRET=${adminSecret}\n`, { mode: 0o600 });
  chmodSync(varsFile, 0o600);
  const url = `http://127.0.0.1:${port}`;
  let child: ChildProcess | null = null;
  let migrated = false;

  const wrangler = join(SERVER_DIR, "node_modules", ".bin", "wrangler");

  const server: LocalServer = {
    url,
    port,
    adminSecret,
    dir,
    logFile,
    async start() {
      if (!migrated) {
        const r = spawnSync(wrangler, ["d1", "migrations", "apply", "DB", "--local", "--persist-to", persist], {
          cwd: SERVER_DIR,
          env: { ...process.env, CI: "1", WRANGLER_SEND_METRICS: "false" },
          encoding: "utf8",
        });
        if (r.status !== 0) throw new Error(`d1 migrations failed: ${r.stderr || r.stdout}`);
        migrated = true;
      }
      const out = openSync(logFile, "a");
      child = spawn(
        wrangler,
        [
          "dev",
          "--port",
          String(port),
          "--inspector-port",
          String(port + 1000),
          "--ip",
          "127.0.0.1",
          "--persist-to",
          persist,
          "--env-file",
          varsFile,
          "--show-interactive-dev-session=false",
        ],
        {
          cwd: SERVER_DIR,
          env: { ...process.env, CI: "1", WRANGLER_SEND_METRICS: "false" },
          stdio: ["ignore", out, out],
          detached: true,
        },
      );
      await waitHealthy(url, 60_000);
    },
    async stop() {
      const c = child;
      child = null;
      if (!c?.pid) return;
      const exited = new Promise<void>((r) => c.once("exit", () => r()));
      try {
        // The process group we started: wrangler and its workerd child.
        process.kill(-c.pid, "SIGTERM");
      } catch {
        // already gone
      }
      const killed = await Promise.race([exited.then(() => true), new Promise<boolean>((r) => setTimeout(() => r(false), 10_000))]);
      if (!killed) {
        try {
          process.kill(-c.pid, "SIGKILL");
        } catch {
          // gone
        }
      }
      await waitDown(url, 15_000);
    },
    async dispose() {
      await server.stop();
      if (!process.env.KEEP_E2E_DIR) rmSync(dir, { recursive: true, force: true });
    },
  };
  return server;
}
