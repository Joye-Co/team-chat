#!/usr/bin/env bun
// This worktree's local sandbox: the real server (`wrangler dev`, local D1, R2 and Durable
// Objects) with the sample team of seed.ts, for the real app to sign in to. Everything lives in
// .tmp/sandbox/ of the worktree: the server state (kept across runs), the log, the PID, the
// admin secret and the owner token (mode 600, never printed).
//
//   bun scripts/sandbox/up.ts            start the server (or reuse it), migrate, seed once
//   bun scripts/sandbox/up.ts --reset    throw the data away and seed again
//   bun scripts/sandbox/up.ts --stop     stop this worktree's server
//
// Migrations run on every start, so new tables of main arrive without a reset. The app reads
// .tmp/sandbox/server-url and owner-token (`desktop --sandbox .tmp/sandbox`).
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { seed } from "./seed.ts";

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../..");
const DIR = join(ROOT, ".tmp", "sandbox");
const STATE = join(DIR, "state");
const PID = join(DIR, "server.pid");
const LOG = join(DIR, "server.log");
const WRANGLER = join(ROOT, "server", "node_modules", ".bin", "wrangler");
const args = process.argv.slice(2);

const log = (line: string) => console.log(`[sandbox] ${line}`);
const read = (p: string) => (existsSync(p) ? readFileSync(p, "utf8").trim() : undefined);
const write = (p: string, v: string, secret = false) => {
  writeFileSync(p, `${v}\n`, { mode: secret ? 0o600 : 0o644 });
  if (secret) chmodSync(p, 0o600);
};

/** The server's PID when it is this worktree's wrangler and still runs. */
function runningPid(): number | undefined {
  const pid = Number(read(PID));
  if (!pid) return undefined;
  const cmd = spawnSync("ps", ["-p", String(pid), "-o", "command="], { encoding: "utf8" }).stdout.trim();
  return cmd.includes(STATE) ? pid : undefined;
}

function stop() {
  const pid = runningPid();
  if (pid) {
    // wrangler starts workerd as a child: stop the whole process group.
    try {
      process.kill(-pid, "SIGTERM");
    } catch {
      process.kill(pid, "SIGTERM");
    }
    log(`stopped the server (pid ${pid})`);
  }
  rmSync(PID, { force: true });
}

async function healthy(url: string): Promise<boolean> {
  try {
    const r = await fetch(`${url}/v1/health`, { signal: AbortSignal.timeout(1000) });
    return r.ok;
  } catch {
    return false;
  }
}

async function free(port: number): Promise<boolean> {
  try {
    const s = Bun.listen({ hostname: "127.0.0.1", port, socket: { data() {} } });
    s.stop(true);
    return true;
  } catch {
    return false;
  }
}

/** A port of its own per worktree, kept in .tmp/sandbox/port. */
async function port(): Promise<number> {
  const saved = Number(read(join(DIR, "port")));
  if (saved && (await free(saved)) && (await free(saved + 1000))) return saved;
  let p = 8850 + (Number(BigInt(`0x${new Bun.CryptoHasher("sha1").update(ROOT).digest("hex").slice(0, 8)}`)) % 100);
  while (!(await free(p)) || !(await free(p + 1000))) p++;
  write(join(DIR, "port"), String(p));
  return p;
}

/** The admin secret: the one of server/.dev.vars when it exists (wrangler loads it), else ours. */
function adminSecret(): { secret: string; envFile?: string } {
  const devVars = read(join(ROOT, "server", ".dev.vars"));
  const fromDevVars = devVars?.match(/^ADMIN_SECRET=(.+)$/m)?.[1]?.trim();
  if (fromDevVars) return { secret: fromDevVars };
  const path = join(DIR, "admin-secret");
  let secret = read(path);
  if (!secret) {
    secret = Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url");
    write(path, secret, true);
  }
  const envFile = join(DIR, "server.env");
  write(envFile, `ADMIN_SECRET=${secret}`, true);
  return { secret, envFile };
}

function migrate() {
  const r = spawnSync(WRANGLER, ["d1", "migrations", "apply", "DB", "--local", "--persist-to", STATE], {
    cwd: join(ROOT, "server"),
    encoding: "utf8",
    env: { ...process.env, CI: "1" },
  });
  if (r.status !== 0) {
    console.error(r.stdout, r.stderr);
    throw new Error("D1 migrations failed");
  }
}

async function start(): Promise<string> {
  const existing = runningPid();
  const p = Number(read(join(DIR, "port")));
  if (existing && p && (await healthy(`http://127.0.0.1:${p}`))) return `http://127.0.0.1:${p}`;
  stop();
  const listen = await port();
  const url = `http://127.0.0.1:${listen}`;
  const { envFile } = adminSecret();
  migrate();
  const out = openSync(LOG, "a");
  const child = spawn(
    WRANGLER,
    [
      "dev",
      "--ip", "127.0.0.1",
      "--port", String(listen),
      "--inspector-port", String(listen + 1000),
      "--persist-to", STATE,
      ...(envFile ? ["--env-file", envFile] : []),
    ],
    { cwd: join(ROOT, "server"), detached: true, stdio: ["ignore", out, out], env: { ...process.env, CI: "1" } },
  );
  child.unref();
  write(PID, String(child.pid));
  for (let i = 0; i < 120; i++) {
    if (await healthy(url)) return url;
    if (child.exitCode !== null) break;
    await Bun.sleep(500);
  }
  throw new Error(`the server did not start; see ${LOG}`);
}

async function main() {
  mkdirSync(DIR, { recursive: true });
  if (!existsSync(WRANGLER)) throw new Error("wrangler is missing: run `bun install --frozen-lockfile` first");
  if (args.includes("--stop")) return stop();
  if (args.includes("--reset")) {
    stop();
    for (const f of ["state", "seed.json", "owner-token", "app"]) rmSync(join(DIR, f), { recursive: true, force: true });
    log("reset");
  }
  // A seed that stopped halfway leaves data without seed.json: start over.
  if (existsSync(STATE) && !existsSync(join(DIR, "seed.json"))) {
    stop();
    rmSync(STATE, { recursive: true, force: true });
    rmSync(join(DIR, "app"), { recursive: true, force: true });
  }
  const url = await start();
  write(join(DIR, "server-url"), url);
  if (!existsSync(join(DIR, "seed.json"))) {
    log("seeding the sample team…");
    await seed({ url, adminSecret: adminSecret().secret, out: DIR, log: (l) => log(l) });
  }
  log(`server ${url} (log ${LOG})`);
}

await main().catch((e) => {
  console.error(`[sandbox] ${e instanceof Error ? e.message : e}`);
  process.exit(1);
});
