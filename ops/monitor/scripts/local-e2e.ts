#!/usr/bin/env bun
// Local end-to-end check of the monitor against a real API stack. Starts a fresh `wrangler dev`
// of the API (port 8798) and of the monitor (env local, port 18798), makes the monitor bot and
// the ops group, then drives the cron: healthy -> API stopped (down alert queued) -> API back
// (down and recovery alerts posted). Reads the ops group as the owner to check the posts.
// Stops both stacks by PID. Report: ops/monitor/.e2e/report.json (no secrets).
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, openSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const HERE = resolve(dirname(new URL(import.meta.url).pathname), "..");
const ROOT = resolve(HERE, "../..");
const SERVER = join(ROOT, "server");
const WORK = join(HERE, ".e2e");
// Ports: E2E_API_PORT (default 8798) and E2E_MONITOR_PORT (default 18798); inspectors at +1000.
const API_PORT = Number(process.env.E2E_API_PORT ?? 8798);
const MON_PORT = Number(process.env.E2E_MONITOR_PORT ?? 18798);
const API = `http://127.0.0.1:${API_PORT}`;
const MON = `http://127.0.0.1:${MON_PORT}`;
const wrangler = (dir: string) =>
  [join(dir, "node_modules/.bin/wrangler"), join(ROOT, "node_modules/.bin/wrangler")].find(existsSync) ?? "wrangler";

rmSync(WORK, { recursive: true, force: true });
mkdirSync(WORK, { recursive: true, mode: 0o700 });
const secret = randomBytes(24).toString("base64url");
const monitorAdmin = randomBytes(24).toString("base64url");
const apiEnv = join(WORK, "api.env");
writeFileSync(apiEnv, `ADMIN_SECRET=${secret}\n`, { mode: 0o600 });
const procs: Bun.Subprocess[] = [];

async function waitUp(url: string, proc: Bun.Subprocess, what: string) {
  const deadline = Date.now() + 90_000;
  for (;;) {
    try {
      await fetch(url);
      return;
    } catch {
      // not up
    }
    if (proc.exitCode !== null || Date.now() > deadline) throw new Error(`${what} did not start; see .e2e logs`);
    await Bun.sleep(500);
  }
}

async function startApi(): Promise<Bun.Subprocess> {
  const persist = join(WORK, "api-state");
  const fd = openSync(join(WORK, "api.log"), "a");
  const mig = Bun.spawn([wrangler(SERVER), "d1", "migrations", "apply", "DB", "--local", "--persist-to", persist], {
    cwd: SERVER, stdout: fd, stderr: fd, env: { ...process.env, CI: "1" },
  });
  if ((await mig.exited) !== 0) throw new Error("migrations failed");
  const p = Bun.spawn(
    [wrangler(SERVER), "dev", "--ip", "127.0.0.1", "--port", String(API_PORT), "--inspector-port", String(API_PORT + 1000), "--persist-to", persist,
      "--env-file", apiEnv, "--show-interactive-dev-session=false"],
    { cwd: SERVER, stdout: fd, stderr: fd, stdin: "ignore", env: { ...process.env, CI: "1" } },
  );
  procs.push(p);
  await waitUp(`${API}/v1/health`, p, "api");
  return p;
}

async function stop(p: Bun.Subprocess) {
  if (p.exitCode !== null) return;
  p.kill("SIGTERM");
  const t = setTimeout(() => p.kill("SIGKILL"), 10_000);
  await p.exited;
  clearTimeout(t);
}

async function j<T>(url: string, init: RequestInit = {}): Promise<T> {
  const r = await fetch(url, init);
  const text = await r.text();
  if (!r.ok) throw new Error(`${url}: ${r.status} ${text.slice(0, 200)}`);
  return JSON.parse(text) as T;
}

const post = (token: string | null, body: unknown, extra: Record<string, string> = {}) => ({
  method: "POST",
  headers: { "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}), ...extra },
  body: JSON.stringify(body),
});

async function cron() {
  const r = await fetch(`${MON}/__scheduled?cron=${encodeURIComponent("*/5 * * * *")}`);
  if (!r.ok) throw new Error(`scheduled: ${r.status}`);
  await Bun.sleep(2500); // the run continues in waitUntil
}

async function opsEvents(ownerToken: string, groupId: string): Promise<any[]> {
  const ws = new WebSocket(`${API.replace("http", "ws")}/v1/ws`, { headers: { authorization: `Bearer ${ownerToken}` } } as any);
  const frames: any[] = [];
  ws.addEventListener("message", (e) => frames.push(JSON.parse(String(e.data))));
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "e2e", version: "1" } }));
  for (let i = 0; i < 100 && !frames.find((f) => f.t === "welcome"); i++) await Bun.sleep(50);
  ws.send(JSON.stringify({ t: "fetch", rid: "f", groupId, afterSeq: 0, limit: 500 }));
  for (let i = 0; i < 100 && !frames.find((f) => f.rid === "f"); i++) await Bun.sleep(50);
  ws.close();
  return frames.find((f) => f.rid === "f")?.events ?? [];
}

async function main() {
  const report: Record<string, unknown> = {};
  let api = await startApi();
  const boot = await j<any>(`${API}/v1/admin/bootstrap`, post(null, { ownerName: "Owner" }, { "x-admin-secret": secret }));
  const bot = await j<any>(`${API}/v1/bots`, post(boot.token, { name: "monitor", role: "ops" }));
  const ops = await j<any>(`${API}/v1/groups`, post(boot.token, { name: "ops", memberIds: [bot.member.id] }));
  const opsId = ops.group.id as string;
  writeFileSync(join(HERE, ".dev.vars"), `MONITOR_TOKEN_LOCAL=${bot.token}\nMONITOR_ADMIN_SECRET=${monitorAdmin}\n`, { mode: 0o600 });
  const targets = JSON.stringify([{ name: "local", baseUrl: API, opsGroup: "ops", tokenSecret: "MONITOR_TOKEN_LOCAL" }]);

  const fd = openSync(join(WORK, "monitor.log"), "a");
  const mon = Bun.spawn(
    [wrangler(HERE), "dev", "--env", "local", "--ip", "127.0.0.1", "--port", String(MON_PORT), "--inspector-port", String(MON_PORT + 1000), "--var", `TARGETS:${targets}`,
      "--test-scheduled", "--persist-to", join(WORK, "monitor-state"), "--show-interactive-dev-session=false"],
    { cwd: HERE, stdout: fd, stderr: fd, stdin: "ignore", env: { ...process.env, CI: "1" } },
  );
  procs.push(mon);
  await waitUp(`${MON}/`, mon, "monitor");

  await cron(); // healthy: round trip + daily summary
  let ev = await opsEvents(boot.token, opsId);
  const msgs = () => ev.filter((e) => e.type === "message.created").map((e) => e.data.message);
  report.healthy = {
    roundTrips: msgs().filter((m) => m.body.startsWith("往返检测")).length,
    summary: msgs().some((m) => m.card?.title?.startsWith("每日运行摘要")),
  };

  await stop(api); // down
  await cron();
  await cron();
  api = await startApi(); // back: posts the queued down alert and the recovery alert
  await cron();
  ev = await opsEvents(boot.token, opsId);
  // A fresh local stack has no archive pass, so the archive check raises 备份过期 too.
  const cards = msgs().filter((m) => m.kind === "card" && !m.card.title.startsWith("每日"));
  const alerts = cards.filter((m) => !m.card.title.includes("备份"));
  report.alerts = cards.map((m) => ({ title: m.card.title, tone: m.card.tone, topicId: m.topicId }));
  report.roundTrips = msgs().filter((m) => m.body.startsWith("往返检测")).length;
  const topics = new Set(msgs().filter((m) => m.body.startsWith("往返检测")).map((m) => m.topicId));
  report.roundTripTopics = topics.size;
  // Manual test alert: a wrong secret is refused; the right one posts one 测试 card.
  const testUrl = `${MON}/test-alert`;
  const wrong = await fetch(testUrl, { method: "POST", headers: { authorization: "Bearer not-the-secret-0000000000" } });
  const right = await fetch(testUrl, post(monitorAdmin, { sink: "local" }));
  const rightBody = (await right.json()) as any;
  ev = await opsEvents(boot.token, opsId);
  const tests = msgs().filter((m) => m.kind === "card" && m.card.status?.label === "测试");
  report.testAlert = { wrongStatus: wrong.status, status: right.status, sink: rightBody?.sink ?? null, cards: tests.map((m) => m.card.title) };
  report.ok =
    wrong.status === 401 &&
    right.status === 200 &&
    tests.length === 1 &&
    tests[0].body.startsWith("【测试】") &&
    (report.healthy as any).roundTrips === 1 &&
    (report.healthy as any).summary === true &&
    alerts.length === 2 &&
    alerts[0].card.tone === "danger" &&
    alerts[1].card.tone === "success" &&
    report.roundTrips === 2 &&
    topics.size === 1;
  writeFileSync(join(WORK, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify(report, null, 2));
  return report.ok;
}

main()
  .then(async (ok) => {
    for (const p of procs) await stop(p);
    rmSync(join(HERE, ".dev.vars"), { force: true });
    rmSync(apiEnv, { force: true });
    process.exit(ok ? 0 : 1);
  })
  .catch(async (err) => {
    console.error(`e2e failed: ${err instanceof Error ? err.message : err}`);
    for (const p of procs) await stop(p);
    rmSync(join(HERE, ".dev.vars"), { force: true });
    rmSync(apiEnv, { force: true });
    process.exit(1);
  });
