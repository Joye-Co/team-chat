#!/usr/bin/env bun
// Local end-to-end run of a runner-backed bot: `wrangler dev` (fresh local state) plus the real
// machine daemon with the fake Claude Code CLI. A member mentions the bot in a topic; the run
// must show the acknowledgement (a reaction on the mention), no progress message in the topic,
// the result as a done card (Card v2, intent done) and the topic status `done`,
// the run log and the uploaded raw session. Then 停止: a second run hangs in the fake CLI, a member
// stops it through the client API (the chat-core `cancel_run` example), and the run must end as
// cancelled with the fake CLI stopped by the daemon's run.cancel, one bot note and the topic open.
// Between them, an answer in the first topic after its CLI exited starts a run that resumes the
// first run's CLI session (`--resume`), and a second bot on the fake Codex app-server uses
// `chat-run` through the file transport (local runs only).
// The first run also uses `chat-run` (the fake CLI's word "chatcard"): it reads `chat-run card
// schema`, posts a card built from it, posts it again (a retry: the idempotency key must give the
// same message, and the topic must hold the card once), and patches the posted card at its rev
// (exit 0, the next rev). The run log must hold the `output` events with the token in the patch
// body redacted, and the socket must be gone after the run.
// Then the usable milestone's fixes (local runs only, see `landedChecks`): a request with a quote
// and an image, and an image-only follow-up, reach the CLI whole; a member's runs in one topic
// share a work folder and branch; a lesson learned in a group stays with that group; approval
// cards close in place (同意, 拒绝, 没用上); the team's model is used by a member without one;
// each mention reaches its own member and a follow-up without one goes to the topic's latest;
// a request for an offline computer waits for it; Codex and ACP runs stop gated actions by code;
// a run reads the group's members, 产出, cards, decisions and topic state with `chat-run`.
//
// Usage (from the repo root): bun apps/daemon/e2e/local-e2e.ts [--port 8793] [--keep]
// With --vm on a Linux runner host (scripts/runner/host-setup.sh), every run's CLI runs in a
// Firecracker microVM: run as the runner user from a checkout under /srv/runner with
// TMPDIR=/srv/runner/<folder> (only those paths are shared with VMs). Codex then uses the socket
// through the VM's relay instead of the request folder. --vm-kernel and --vm-rootfs default to
// /srv/runner/kernels/vmlinux and /srv/runner/images/base.ext4.
// Against a deployed env (no wrangler; the owner token comes from a file, and the run makes its
// own group, member, bot runner and machine, then removes the runner and the machine):
//   bun apps/daemon/e2e/local-e2e.ts --base-url https://chat-api-e2e.<account>.workers.dev \
//     --owner-token-file ~/.config/team-chat/e2e/owner-token
// On Windows the fake CLIs are started through the `fake-cli` launcher (`cargo build -p daemon
// --features test-fakes`, built here when missing; `python` on PATH). With --service (Windows
// only) the daemon is not a child of this script: it is installed with
// `scripts/install-daemon.ps1` and runs as the scheduled task of this user, and after the checks
// above the script updates it by the replace protocol while it is idle and while a run is
// active, ends it the way a logoff does, and starts the task again the way a logon does. It
// refuses to run on a computer that is a machine already (a config, a task, a running daemon).
// With --server-only <folder> the script only starts the local server, writes its address to
// <folder>/url and an owner's token to <folder>/owner-token, and keeps it running until a file
// <folder>/stop appears (the desktop's 「用这台电脑」 test on Windows CI runs against it).
// Needs: `bun install`, and the daemon binary (built here with cargo when missing).
// Tokens stay in memory or in files of mode 600 inside the temp folder; nothing is printed.
import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const WIN = process.platform === "win32";
const EXE = WIN ? ".exe" : "";
const TARGET = process.env.CARGO_TARGET_DIR ?? join(ROOT, "target");
// `bunx`, by the path of the bun that runs this script (Windows has no `bunx` to start by name).
const BUNX = [process.execPath, "x"];
const args = process.argv.slice(2);
const flag = (name: string) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
const PORT = Number(flag("--port") || 0) || 8793;
const KEEP = args.includes("--keep");
const VM = args.includes("--vm");
const VM_KERNEL = flag("--vm-kernel") ?? "/srv/runner/kernels/vmlinux";
const VM_ROOTFS = flag("--vm-rootfs") ?? "/srv/runner/images/base.ext4";
const VM_DIR = join(process.env.TMPDIR ?? tmpdir(), `vms-${process.pid}`);
const REMOTE = flag("--base-url")?.replace(/\/$/, "");
const OWNER_TOKEN_FILE = flag("--owner-token-file")?.replace(/^~/, process.env.HOME ?? "~");
const SERVICE = args.includes("--service");
const SERVER_ONLY = flag("--server-only");
if (SERVICE && !WIN) throw new Error("--service is for Windows (the daemon as a scheduled task)");
const BASE = REMOTE ?? `http://127.0.0.1:${PORT}`;
const TMP = mkdtempSync(join(tmpdir(), "bots-e2e-"));
// A label of its own on a shared env, so no other machine takes the run.
const LABEL = REMOTE ? `e2e-${crypto.randomUUID().slice(0, 8)}` : "e2e";
const report: Record<string, unknown> = { base: BASE, tmp: TMP, steps: [] as string[] };
const children: { name: string; pid: number; kill: () => void }[] = [];
/** A path as a TOML string: a literal one, so a Windows path keeps its backslashes. */
const q = (path: string) => `'${path}'`;

/** Ends the process `pid` and everything it started. */
function killTree(pid: number): void {
  try {
    if (WIN) spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
    else process.kill(-pid, "SIGTERM");
  } catch {
    /* gone */
  }
}

/** The CLI's working folder of a run: its work line's `work/` (run.json `line`), else `<run>/work`. */
function workOf(work: string, runId: string): string {
  const meta = JSON.parse(readFileSync(join(work, "runs", runId, "run.json"), "utf8"));
  return meta.line ? join(meta.line, "work") : join(work, "runs", runId, "work");
}

function step(s: string): void {
  (report.steps as string[]).push(s);
  console.log(`- ${s}`);
}

function secretFile(name: string, value: string): string {
  const p = join(TMP, name);
  writeFileSync(p, value, { mode: 0o600 });
  chmodSync(p, 0o600);
  return p;
}

function start(name: string, cmd: string, argv: string[], opts: { cwd: string; env?: Record<string, string> }) {
  const log = join(TMP, `${name}.log`);
  const fd = openSync(log, "a");
  const child = spawn(cmd, argv, { cwd: opts.cwd, env: { ...process.env, ...opts.env }, stdio: ["ignore", fd, fd], detached: !WIN, windowsHide: true });
  if (!child.pid) throw new Error(`${name} did not start`);
  const pid = child.pid;
  // Stop by the PID we started (its process group, on Windows its process tree: wrangler runs
  // workerd as a child).
  children.push({ name, pid, kill: () => killTree(pid) });
  step(`${name} started (pid ${pid}, log ${log})`);
}

function stopAll(): void {
  for (const c of children.reverse()) c.kill();
}

async function sleep(ms: number) {
  await new Promise((r) => setTimeout(r, ms));
}

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
    await sleep(200);
  }
}

async function call(method: string, path: string, opts: { token?: string; body?: unknown; headers?: Record<string, string> } = {}) {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${BASE}${path}`, { method, headers, body: opts.body === undefined ? undefined : JSON.stringify(opts.body) });
  const text = await res.text();
  let data: any = null;
  try {
    data = JSON.parse(text);
  } catch {
    data = text;
  }
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${data?.error?.code ?? ""}`);
  return data;
}

const cleanup: (() => Promise<unknown>)[] = [];

async function main() {
  const server = join(ROOT, "server");
  let owner: string;
  let groupId: string;
  let alice: { token: string; member: { id: string } };
  let bot: { member: { id: string } };
  if (REMOTE) {
    if (!OWNER_TOKEN_FILE) throw new Error("--base-url needs --owner-token-file");
    owner = (await Bun.file(OWNER_TOKEN_FILE).text()).trim();
    await until("server health", async () => (await fetch(`${BASE}/v1/health`)).ok, 30_000);
    step("server is up");
    const invite = await call("POST", "/v1/invites", { token: owner, body: { name: `Alice ${LABEL}`, kind: "human", level: "member" } });
    alice = await call("POST", "/v1/auth/redeem", { body: { inviteCode: invite.code, deviceName: "e2e" } });
    bot = await call("POST", "/v1/bots", { token: owner, body: { name: `dev-${LABEL}`, role: "开发" } });
    const g = await call("POST", "/v1/groups", { token: owner, body: { name: `daemon ${LABEL}`, memberIds: [alice.member.id, bot.member.id] } });
    groupId = g.group.id;
  } else {
    ({ owner, groupId, alice, bot } = await localServer(server));
  }
  if (SERVER_ONLY) {
    mkdirSync(SERVER_ONLY, { recursive: true });
    writeFileSync(join(SERVER_ONLY, "url"), BASE);
    writeFileSync(join(SERVER_ONLY, "owner-token"), owner, { mode: 0o600 });
    step(`the server runs at ${BASE} until ${join(SERVER_ONLY, "stop")} appears`);
    await until("the stop file", () => existsSync(join(SERVER_ONLY, "stop")), 3 * 3600_000);
    return;
  }
  await call("PUT", `/v1/bots/${bot.member.id}/runner`, { token: owner, body: { runtime: "claude", labels: [LABEL] } });
  const machine = await call("POST", "/v1/machines", { token: owner, body: { name: `mac-${LABEL}`, labels: [LABEL] } });
  if (REMOTE) {
    cleanup.push(() => call("DELETE", `/v1/machines/${machine.machine.id}`, { token: owner }));
    cleanup.push(() => call("DELETE", `/v1/bots/${bot.member.id}/runner`, { token: owner }));
  }
  const tokenFile = secretFile("machine-token", machine.token);
  step("owner, member, bot runner and machine made");
  await runDaemonCheck(owner, groupId, alice, bot, machine, tokenFile);
}

async function localServer(server: string) {
  const state = join(TMP, "wrangler-state");
  const adminSecret = crypto.randomUUID();
  // A throwaway admin secret for this fresh local state only.
  const mig = spawnSync(BUNX[0], [...BUNX.slice(1), "wrangler", "d1", "migrations", "apply", "DB", "--local", "--persist-to", state], {
    cwd: server,
    encoding: "utf8",
    env: { ...process.env, CI: "1" },
  });
  if (mig.status !== 0) throw new Error(`d1 migrations failed: ${mig.stderr?.slice(-400)}`);
  step("local D1 migrated");
  start(
    "wrangler",
    BUNX[0],
    [...BUNX.slice(1), "wrangler", "dev", "--port", String(PORT), "--inspector-port", String(PORT + 1000), "--ip", "127.0.0.1", "--persist-to", state, "--var", `ADMIN_SECRET:${adminSecret}`],
    { cwd: server },
  );
  await until("server health", async () => (await fetch(`${BASE}/v1/health`)).ok, 90_000);
  step("server is up");

  // Members: owner (bootstrap), a human (invite), a runner-backed bot.
  const boot = await call("POST", "/v1/admin/bootstrap", { headers: { "x-admin-secret": adminSecret }, body: { ownerName: "Joye" } });
  const owner = boot.token as string;
  const groupId = boot.group.id as string;
  const invite = await call("POST", "/v1/invites", { token: owner, body: { name: "Alice", kind: "human", level: "member" } });
  const alice = await call("POST", "/v1/auth/redeem", { body: { inviteCode: invite.code, deviceName: "e2e" } });
  const bot = await call("POST", "/v1/bots", { token: owner, body: { name: "dev", role: "开发" } });
  return { owner, groupId, alice, bot };
}

async function runDaemonCheck(
  owner: string,
  groupId: string,
  alice: { token: string },
  bot: { member: { id: string } },
  machine: { machine: { id: string } },
  tokenFile: string,
) {

  // The daemon with the fake Claude Code CLI.
  const daemonBin = join(TARGET, "debug", `daemon${EXE}`);
  // Windows starts a Python fake through the launcher of the daemon's `test-fakes` feature.
  const fakeCli = join(TARGET, "debug", "fake-cli.exe");
  if (!existsSync(daemonBin) || (WIN && !existsSync(fakeCli))) {
    const b = spawnSync("cargo", ["build", "-p", "daemon", ...(WIN ? ["--features", "test-fakes"] : [])], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p daemon failed");
  }
  /** The `command` and `env` lines of a runtime whose CLI is the fake `script`. */
  // The scheduled task's daemon has the user's PATH, not this script's: name python by its path.
  const python = WIN ? ((spawnSync("where", ["python"], { encoding: "utf8" }).stdout ?? "").split(/\r?\n/)[0] ?? "").trim() : "";
  if (WIN && !python) throw new Error("python is not on PATH (the fake CLIs are Python scripts)");
  const fakeRuntime = (script: string, env: string[]) => {
    const all = WIN ? [`FAKE_CLI_SCRIPT = ${q(script)}`, `FAKE_CLI_PYTHON = ${q(python)}`, ...env] : env;
    return [`command = ${q(WIN ? fakeCli : script)}`, ...(all.length ? [`env = { ${all.join(", ")} }`] : [])];
  };
  const fake = join(ROOT, "apps/daemon/tests/fake/fake_claude.py");
  chmodSync(fake, 0o755);
  const fakeCodex = join(ROOT, "apps/daemon/tests/fake/fake_codex.py");
  chmodSync(fakeCodex, 0o755);
  const fakeAcp = join(ROOT, "apps/daemon/tests/fake/fake_acp.py");
  chmodSync(fakeAcp, 0o755);
  const work = join(TMP, "work");
  const claudeHome = join(TMP, "claude-home");
  mkdirSync(claudeHome, { recursive: true });
  const config = join(TMP, "daemon.toml");
  // What the machine is: the install command writes these itself.
  const identity = [`server_url = "${BASE}"`, `token_file = ${q(tokenFile)}`, `name = "mac-${LABEL}"`, `labels = ["${LABEL}"]`];
  const rest = [
    `work_root = ${q(work)}`,
    "max_parallel_runs = 2",
    "min_free_gb = 1",
    "",
    "[runtimes.claude]",
    'kind = "claude"',
    ...fakeRuntime(fake, [`CLAUDE_CONFIG_DIR = ${q(claudeHome)}`]),
    "",
    "[runtimes.codex]",
    'kind = "codex"',
    ...fakeRuntime(fakeCodex, []),
    'sandbox = "workspace-write"',
    'approval_policy = "never"',
    "",
    // An ACP agent that asks before its tools (its runs are gated by code).
    "[runtimes.acp]",
    'kind = "acp"',
    ...fakeRuntime(fakeAcp, []),
    "asks_permission = true",
    "",
    ...(VM
      ? [
          "[vm]",
          `kernel = ${q(VM_KERNEL)}`,
          `rootfs = ${q(VM_ROOTFS)}`,
          `dir = ${q(VM_DIR)}`,
          // The fake CLIs and the fake Claude config are not in the base image: share them.
          `shares = [${q(claudeHome)}, ${q(dirname(fake))}]`,
          "",
        ]
      : []),
  ];
  let service: Service | undefined;
  /** Stops and starts the daemon this script started (not the scheduled task of --service). */
  let daemon: Landed["daemon"];
  if (SERVICE) {
    service = installService(daemonBin, machine.machine.id, tokenFile, rest);
  } else {
    writeFileSync(config, [...identity, ...rest].join("\n"));
    start("daemon", daemonBin, ["run", "--config", config], { cwd: TMP });
    daemon = {
      stop: async () => {
        const i = children.findIndex((c) => c.name === "daemon");
        if (i < 0) throw new Error("no daemon to stop");
        const [c] = children.splice(i, 1);
        c.kill();
        await until("the daemon ended", () => !alive(c.pid), 30_000);
      },
      start: () => start("daemon", daemonBin, ["run", "--config", config], { cwd: TMP }),
    };
  }
  const healthy = await until("machine healthy", async () => {
    const list = (await call("GET", "/v1/machines", { token: owner })) as any[];
    return list.find((m) => m.id === machine.machine.id && m.healthy);
  });
  report.machine = { connected: healthy.connected, healthy: healthy.healthy, labels: healthy.labels, daemonVersion: healthy.daemonVersion };
  step("daemon connected and healthy");

  // Alice mentions the bot in a new topic of General.
  const events: any[] = [];
  const transients: any[] = [];
  const ws = new WebSocket(`${BASE.replace("http", "ws")}/v1/ws`, { headers: { authorization: `Bearer ${alice.token}` } } as any);
  ws.addEventListener("message", (e) => {
    const f = JSON.parse(String(e.data));
    if (f.t === "event") events.push(f.event);
    if (f.t === "transient") transients.push(f.event);
    if (f.t === "ack" || f.t === "welcome") events.push({ frame: f });
  });
  await until("socket open", () => ws.readyState === WebSocket.OPEN, 10_000);
  ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "e2e", version: "0" } }));
  await until("welcome", () => events.find((e) => e.frame?.t === "welcome"));
  ws.send(
    JSON.stringify({ t: "send", rid: "r1", groupId, clientMsgId: crypto.randomUUID(), body: "@dev 请总结一下 slow chatcard", mentions: [bot.member.id] }),
  );
  const ack = await until("send ack", () => events.find((e) => e.frame?.rid === "r1"));
  if (!ack.frame.ok) throw new Error("send failed");
  const topicId = ack.frame.result.message.topicId as string;
  step(`mentioned the bot in topic ${topicId}`);

  const allBotMsgs = () => events.filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === bot.member.id).map((e) => e.data.message);
  const CHAT_RUN_TITLE = "chat-run card";
  const botMsgs = () => allBotMsgs().filter((m) => m.cardV2?.title !== CHAT_RUN_TITLE);
  const statuses = () => events.filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status).map((e) => e.data.status);
  const ackReaction = await until("acknowledgement", () =>
    events.find((e) => e.type === "reaction.changed" && e.actorId === bot.member.id && e.data.messageId === ack.frame.result.message.id && e.data.on),
  );
  await until("topic in progress", () => statuses().includes("in_progress"));
  step("bot acknowledged with a reaction; topic in progress");
  const done = await until("topic done", () => statuses().at(-1) === "done", 60_000);
  const result = botMsgs().find((m) => m.cardV2?.intent === "done" && m.body.startsWith("reply 1:"));
  if (!result) throw new Error("no done card");
  // Progress stays in the run (plan, run log): the done card is the bot's only message.
  if (botMsgs().length !== 1 || botMsgs().some((m) => m.streaming)) throw new Error(`unexpected bot messages: ${botMsgs().length}`);
  const runs = events.filter((e) => e.type === "run.updated").map((e) => e.data.run);
  const runId = runs[0]?.id as string;
  const finalRun = runs.filter((r: any) => r.id === runId).at(-1);
  if (finalRun?.resultMessageId !== result.id) throw new Error("run has no resultMessageId");
  step("no progress message; done card posted; topic done");

  const log = await call("GET", `/v1/runs/${runId}/events`, { token: alice.token });
  // chat-run: the output events are in the run log, the patch body redacted; the socket is gone.
  const outputs = log.entries.filter((e: any) => e.t === "run.event" && e.event.type === "output");
  const output = outputs.find((e: any) => e.event.op === "card.patch");
  if (!output || outputs.filter((e: any) => e.event.op === "card.post").length !== 2) throw new Error("no chat-run output events in the run log");
  const outputBody = JSON.stringify(output.event.body);
  if (outputBody.includes("tm_fakefakefakefakefakefake01") || !outputBody.includes("<redacted>")) throw new Error("output body not redacted");
  const runDir = join(work, "runs", runId);
  const chatRun = JSON.parse(readFileSync(join(workOf(work, runId), "chatcard-1.json"), "utf8"));
  const sockets = readdirSync(runDir).filter((f) => f.endsWith(".sock"));
  if (sockets.length > 0) throw new Error("chat-run socket left after the run");
  // The card came from `chat-run card schema`; the post, its retry and the patch all worked.
  if (!chatRun.kinds?.includes("text")) throw new Error(`chat-run card schema gave no block types: ${JSON.stringify(chatRun.kinds)}`);
  const posted = chatRun.post.out;
  if (chatRun.post.code !== 0 || !posted?.messageId) throw new Error(`chat-run card post failed: ${JSON.stringify(chatRun.post)}`);
  if (chatRun.again.code !== 0 || chatRun.again.out?.messageId !== posted.messageId) throw new Error(`the retry posted another card: ${JSON.stringify(chatRun.again)}`);
  const copies = allBotMsgs().filter((m) => m.cardV2?.title === CHAT_RUN_TITLE);
  if (copies.length !== 1) throw new Error(`the topic holds the chat-run card ${copies.length} times`);
  const rev = posted.message.cardV2.rev as number;
  if (chatRun.patch.code !== 0 || chatRun.patch.out?.rev !== rev + 1) throw new Error(`chat-run card patch failed: ${JSON.stringify(chatRun.patch)}`);
  report.chatRun = { post: posted.messageId, retry: chatRun.again.out.messageId, patchedRev: chatRun.patch.out.rev, blockTypes: chatRun.kinds.length };
  step(`chat-run: card from the schema posted once (retry deduped), patched to rev ${chatRun.patch.out.rev}; outputs in the run log, redacted`);
  const session = await until("raw session uploaded", async () => {
    const res = await fetch(`${BASE}/v1/runs/${runId}/session`, { headers: { authorization: `Bearer ${owner}` } });
    return res.ok ? new Uint8Array(await res.arrayBuffer()) : undefined;
  });
  report.resume = await resumeCheck(groupId, topicId, runId, bot, work, events, ws);
  if (!REMOTE) report.codex = await codexCheck(owner, groupId, work, events, ws);
  const stop = await cancelCheck(groupId, alice, bot, work, events, ws);
  // The usable milestone's fixes (local runs only: they change team settings and need the
  // fake CLI's files on this computer).
  if (!REMOTE && !VM) report.landed = await landedChecks({ owner, groupId, alice: alice as Person, bot, work, events, ws, machineId: machine.machine.id, daemon });
  if (service) report.service = await serviceChecks(service, owner, groupId, alice, bot, machine.machine.id, work, events, ws);
  ws.close();
  report.cancel = stop;
  Object.assign(report, {
    ok: true,
    topicId,
    runId,
    acknowledgement: ackReaction.data.emoji,
    botMessages: botMsgs().length,
    result: result.body,
    resultCardBlocks: result.cardV2.blocks.map((b: any) => b.type),
    usage: finalRun.usage ?? null,
    durationMs: finalRun.durationMs ?? null,
    topicStatuses: statuses(),
    runStatuses: runs.map((r: any) => r.status),
    runLogEntries: log.entries.length,
    runLogTypes: log.entries.map((e: any) => (e.t === "run.event" ? e.event.type : e.t)),
    sessionGzipBytes: session.byteLength,
    sessionIsGzip: session[0] === 0x1f && session[1] === 0x8b,
    doneCard: result.cardV2?.intent === "done",
  });
  if (!report.sessionIsGzip) throw new Error("session is not gzip");
  step("run log and raw session checked");
}

/**
 * Resume: after the first run ended (its CLI exited), a member answers in the same topic. The
 * next run goes to the same machine with `run.assign.resumeSessionId`, and the daemon starts the
 * fake CLI with `--resume` of the first run's session.
 */
async function resumeCheck(groupId: string, topicId: string, firstRunId: string, bot: { member: { id: string } }, work: string, events: any[], ws: WebSocket) {
  const argvOf = (runId: string) => JSON.parse(readFileSync(join(workOf(work, runId), "argv.json"), "utf8")) as string[];
  const first = argvOf(firstRunId);
  const s1 = first[first.indexOf("--session-id") + 1];
  if (!s1) throw new Error("the first run has no --session-id");
  ws.send(JSON.stringify({ t: "send", rid: "r-resume", groupId, topicId, clientMsgId: crypto.randomUUID(), body: "@dev 再看一下", mentions: [bot.member.id] }));
  const ack = await until("send ack (resume)", () => events.find((e) => e.frame?.rid === "r-resume"));
  if (!ack.frame.ok) throw new Error("send failed");
  const runsOf = () => events.filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId && e.data.run.id !== firstRunId).map((e) => e.data.run);
  const second = await until("second run done", () => runsOf().find((r: any) => r.status === "done"), 60_000);
  const argv = argvOf(second.id);
  const i = argv.indexOf("--resume");
  if (i < 0 || argv[i + 1] !== s1 || argv.includes("--session-id")) throw new Error(`run 2 did not resume ${s1}: ${JSON.stringify(argv)}`);
  step(`answer after the CLI exited: run ${second.id} resumed session ${s1}`);
  return { firstRunId, secondRunId: second.id, sessionId: s1 };
}

/**
 * The Codex file transport: a second bot runs the fake Codex app-server (sandbox
 * workspace-write), whose `chat-run` uses the request folder `<work>/.chat-run-<run id>`
 * instead of the socket. It posts a card from `chat-run card schema` (and its retry, deduped) and
 * patches it; the folder is gone after the run and the topic is done.
 */
async function codexCheck(owner: string, groupId: string, work: string, events: any[], ws: WebSocket) {
  const cx = await call("POST", "/v1/bots", { token: owner, body: { name: "cx", role: "开发" } });
  await call("PUT", `/v1/bots/${cx.member.id}/runner`, { token: owner, body: { runtime: "codex", labels: [LABEL] } });
  ws.send(JSON.stringify({ t: "send", rid: "r-codex", groupId, clientMsgId: crypto.randomUUID(), body: "@cx chatcard", mentions: [cx.member.id] }));
  const ack = await until("send ack (codex)", () => events.find((e) => e.frame?.rid === "r-codex"));
  if (!ack.frame.ok) throw new Error(`send failed: ${JSON.stringify(ack.frame.error)}`);
  const topicId = ack.frame.result.message.topicId as string;
  const run = await until("codex run done", () => events.find((e) => e.type === "run.updated" && e.data.run.topicId === topicId && ["done", "failed"].includes(e.data.run.status))?.data.run, 60_000);
  if (run.status !== "done") throw new Error(`codex run ${run.status}`);
  const runWork = workOf(work, run.id);
  const r = JSON.parse(readFileSync(join(runWork, "chatcard.json"), "utf8"));
  if (VM) {
    // In a VM chat-run always uses the socket the agent relays.
    if (r.dir || r.socket !== "/run/chat-run.sock") throw new Error(`codex chat-run in a VM used no relayed socket: ${JSON.stringify({ dir: r.dir, socket: r.socket })}`);
  } else {
    if (r.socket || !r.dir?.endsWith(`.chat-run-${run.id}`)) throw new Error(`codex chat-run used no request folder: ${JSON.stringify({ dir: r.dir, socket: r.socket })}`);
    if (existsSync(r.dir)) throw new Error("the request folder is left after the run");
  }
  const posted = r.post.out;
  if (r.post.code !== 0 || !posted?.messageId) throw new Error(`codex chat-run card post failed: ${JSON.stringify(r.post)}`);
  if (r.again.out?.messageId !== posted.messageId) throw new Error("the codex retry posted another card");
  if (r.patch.code !== 0 || r.patch.out?.rev !== posted.message.cardV2.rev + 1) throw new Error(`codex chat-run card patch failed: ${JSON.stringify(r.patch)}`);
  const cards = events.filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.cardV2?.title === "chat-run codex card");
  if (cards.length !== 1) throw new Error(`the topic holds the codex card ${cards.length} times`);
  step(`codex: chat-run through ${VM ? "the VM's relayed socket" : "the request folder"} posted once (retry deduped) and patched to rev ${r.patch.out.rev}${VM ? "" : "; folder removed"}`);
  return { runId: run.id, topicId, patchedRev: r.patch.out.rev };
}

/** The first file named `name` under `dir`, at any depth. */
function findFile(dir: string, name: string): string | undefined {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isFile() && entry.name === name) return p;
    if (entry.isDirectory()) {
      const found = findFile(p, name);
      if (found) return found;
    }
  }
  return undefined;
}

function alive(pid: number): boolean {
  if (WIN) {
    const r = spawnSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], { encoding: "utf8" });
    return (r.stdout ?? "").includes(`"${pid}"`);
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * 停止: a member asks the bot for work that hangs (the fake CLI sleeps), then stops the run
 * through the client API (`chat_core::Client::cancel_run`, the call behind the desktop's 停止).
 * The run must end as cancelled at once, the daemon must get run.cancel and stop the fake CLI,
 * the bot must post one note and the topic must go back to open. A second stop is a conflict,
 * which the client takes as "already ended".
 */
async function cancelCheck(
  groupId: string,
  alice: { token: string; member?: { id: string } },
  bot: { member: { id: string } },
  work: string,
  events: any[],
  ws: WebSocket,
) {
  const example = join(TARGET, "debug", "examples", `cancel_run${EXE}`);
  if (!existsSync(example)) {
    const b = spawnSync("cargo", ["build", "-p", "chat-core", "--example", "cancel_run"], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p chat-core --example cancel_run failed");
  }
  const aliceTokenFile = secretFile("alice-token", alice.token);
  const cancel = (runId: string) => {
    const r = spawnSync(example, [BASE, aliceTokenFile, runId, join(TMP, "client-data")], { encoding: "utf8" });
    const line = (r.stdout ?? "").trim().split(/\r?\n/).at(-1) ?? "";
    let out: any = {};
    try {
      out = JSON.parse(line);
    } catch {
      out = { error: `no JSON (exit ${r.status})` };
    }
    if (r.status !== 0 || out.error) throw new Error(`client cancel_run failed: ${out.error ?? r.status}`);
    return out as { outcome: string; status: string | null };
  };

  const rmPid = findFile(work, "fake.pid");
  if (rmPid) rmSync(rmPid);
  ws.send(JSON.stringify({ t: "send", rid: "r2", groupId, clientMsgId: crypto.randomUUID(), body: "@dev hang", mentions: [bot.member.id] }));
  const ack = await until("send ack (hang)", () => events.find((e) => e.frame?.rid === "r2"));
  if (!ack.frame.ok) throw new Error("send failed");
  const topicId = ack.frame.result.message.topicId as string;
  const runsOf = () => events.filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run);
  const running = await until("run running", () => runsOf().find((r: any) => r.status === "running"), 60_000);
  // The file can show up before its content (a VM writes it over NFS): wait for the pid.
  const pidFile = await until(
    "fake CLI hangs",
    () => {
      const f = findFile(work, "fake.pid");
      return f && readFileSync(f, "utf8").trim() ? f : undefined;
    },
    60_000,
  );
  const pid = Number(readFileSync(pidFile, "utf8").trim());
  // In a VM the pid is the guest's: the CLI lives as long as the run's VM.
  const cliAlive = () => (VM ? existsSync(join(VM_DIR, running.id)) : alive(pid));
  if (!cliAlive()) throw new Error("the fake CLI is not running");
  step(`run ${running.id} works (fake CLI pid ${pid} hangs)`);

  const first = cancel(running.id);
  if (first.outcome !== "cancelled" || first.status !== "cancelled") throw new Error(`stop answered ${JSON.stringify(first)}`);
  step("a member stopped the run through the client API");
  const ended = await until("run.updated cancelled", () => runsOf().find((r: any) => r.id === running.id && r.status === "cancelled"));
  await until("daemon got run.cancel and stopped the fake CLI", () => !cliAlive(), 30_000);
  const statuses = () => events.filter((e) => e.type === "topic.updated" && e.data.topicId === topicId && e.data.status).map((e) => e.data.status);
  await until("topic back to open", () => statuses().at(-1) === "open");
  const notes = () =>
    events
      .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === bot.member.id)
      .map((e) => e.data.message)
      .filter((m: any) => m.body.includes("已停止"));
  const note = await until("bot note", () => notes()[0]);
  const again = cancel(running.id);
  if (again.outcome !== "already_ended" || again.status !== "cancelled") throw new Error(`second stop answered ${JSON.stringify(again)}`);
  if (notes().length !== 1) throw new Error(`the bot posted ${notes().length} stop notes`);
  step("run cancelled: fake CLI stopped, bot note posted, topic open; a second stop is already ended");
  return {
    topicId,
    runId: running.id,
    first,
    again,
    runStatuses: runsOf().map((r: any) => r.status),
    endedAt: ended.endedAt ?? null,
    cliStopped: !cliAlive(),
    note: note.body,
    topicStatuses: statuses(),
  };
}

// ---------------------------------------------------------------------------
// The usable milestone (docs/plans/usable.zh.md, line 12; 盯 5.3): one check per merged fix,
// each on what a person sees and on what the fake CLI was given.
// ---------------------------------------------------------------------------

type Person = { token: string; member: { id: string } };
type Landed = {
  owner: string;
  groupId: string;
  alice: Person;
  bot: { member: { id: string } };
  work: string;
  events: any[];
  ws: WebSocket;
  machineId: string;
  /** The daemon this script started; absent with --service. */
  daemon?: { stop: () => Promise<void>; start: () => void };
};

let landedRid = 0;

/** Sends a message as the owner of `ws` and returns the stored message. */
async function say(
  events: any[],
  ws: WebSocket,
  groupId: string,
  body: string,
  extra: { topicId?: string; mentions?: string[]; replyTo?: { messageId: string }; attachments?: unknown[] } = {},
) {
  const rid = `landed-${++landedRid}`;
  ws.send(JSON.stringify({ t: "send", rid, groupId, clientMsgId: crypto.randomUUID(), body, ...extra }));
  const ack = await until(`send ack (${rid})`, () => events.find((e) => e.frame?.rid === rid));
  if (!ack.frame.ok) throw new Error(`send failed: ${JSON.stringify(ack.frame.error)}`);
  return ack.frame.result.message as { id: string; topicId: string };
}

/** A client socket of `token` that keeps every event and ack (as `local-e2e` does for Alice). */
async function socket(token: string): Promise<{ ws: WebSocket; events: any[] }> {
  const events: any[] = [];
  const ws = new WebSocket(`${BASE.replace("http", "ws")}/v1/ws`, { headers: { authorization: `Bearer ${token}` } } as any);
  ws.addEventListener("message", (e) => {
    const f = JSON.parse(String(e.data));
    if (f.t === "event") events.push(f.event);
    if (f.t === "ack" || f.t === "welcome") events.push({ frame: f });
  });
  await until("socket open", () => ws.readyState === WebSocket.OPEN, 10_000);
  ws.send(JSON.stringify({ t: "hello", protocol: 1, client: { name: "e2e", version: "0" } }));
  await until("welcome", () => events.find((e) => e.frame?.t === "welcome"));
  return { ws, events };
}

const runsIn = (events: any[], topicId: string) => events.filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run);
const ENDED = ["done", "failed", "cancelled"];

/** The run of the topic that started after `others`, once it ended; it must be done. */
async function ranDone(events: any[], topicId: string, what: string, others: string[] = []) {
  const run = await until(`${what}: the run ends`, () => runsIn(events, topicId).find((r: any) => !others.includes(r.id) && ENDED.includes(r.status)), 60_000);
  if (run.status !== "done") throw new Error(`${what}: the run ended ${run.status}: ${run.summary ?? ""}`);
  return run as { id: string; resultMessageId?: string };
}

/** What the fake CLI got as turns in the run's working folder (prompts.jsonl), in order. */
function promptsOf(work: string, runId: string): { pid: number; n: number; text: string }[] {
  const p = join(workOf(work, runId), "prompts.jsonl");
  if (!existsSync(p)) return [];
  return readFileSync(p, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
}

/** The request section of a prompt (the topic lines and rules come before it). */
const requestPart = (prompt: string) => prompt.slice(prompt.lastIndexOf("Request from"));

/** Uploads a small image to the group and returns it as a message attachment. */
async function uploadImage(token: string, groupId: string, name: string) {
  const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...new TextEncoder().encode(name)]);
  const sha256 = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
  const c = await call("POST", "/v1/files", { token, body: { groupId, name, size: bytes.length, mime: "image/png", sha256 } });
  const put = await fetch(`${BASE}/v1/files/${c.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${token}` }, body: bytes });
  if (!put.ok) throw new Error(`file upload ${put.status}`);
  return { fileId: c.file.id as string, name, size: bytes.length, mime: "image/png", sha256 };
}

async function landedChecks(x: Landed) {
  const out: Record<string, unknown> = {};
  out.requestWhole = await requestWholeCheck(x);
  out.workLine = await workLineCheck(x);
  out.memoryAudience = await memoryAudienceCheck(x);
  out.approvalCards = await approvalCardCheck(x);
  out.teamModel = await teamModelCheck(x);
  out.routing = await routingCheck(x);
  out.gatesByCode = await gatesByCodeCheck(x);
  out.readCommands = await readCommandsCheck(x);
  if (x.daemon) out.offlineComputer = await offlineComputerCheck(x);
  return out;
}

/**
 * 接 2.1 (the request reaches the member whole): a request that quotes a message and carries an
 * image reaches the CLI with the quote and the file by id; a follow-up that is only an image,
 * sent while the CLI works, reaches it as the file's line (not an empty turn) and starts no
 * second run.
 */
async function requestWholeCheck({ alice, bot, groupId, work, events, ws }: Landed) {
  const quoted = await say(events, ws, groupId, "登录按钮点了没反应");
  const topicId = quoted.topicId;
  const first = await uploadImage(alice.token, groupId, "shot-1.png");
  // "hold": the fake CLI keeps the turn open until the daemon gives it the next input.
  await say(events, ws, groupId, "@dev 修一下 hold", { topicId, mentions: [bot.member.id], replyTo: { messageId: quoted.id }, attachments: [first] });
  const run = await until("request run running", () => runsIn(events, topicId).find((r: any) => r.status === "running"), 60_000);
  const taken = await until("the CLI took the request", () => promptsOf(work, run.id)[0], 30_000);
  const req = requestPart(taken.text);
  if (!req.includes("修一下")) throw new Error(`the request is not in the prompt: ${req.slice(0, 400)}`);
  if (!req.includes(`[file f ${first.fileId} · shot-1.png · image/png`)) throw new Error(`the prompt does not name the image by id: ${req.slice(0, 400)}`);
  if (!req.includes(`↪ quotes m ${quoted.id} (Alice: 登录按钮点了没反应)`)) throw new Error(`the prompt does not carry the quote: ${req.slice(0, 400)}`);
  const second = await uploadImage(alice.token, groupId, "shot-2.png");
  await say(events, ws, groupId, "", { topicId, attachments: [second] });
  const done = await ranDone(events, topicId, "request run");
  if (done.id !== run.id) throw new Error("the image follow-up started another run instead of reaching the running one");
  const held = JSON.parse(readFileSync(join(workOf(work, run.id), "held.json"), "utf8")) as { text: string | null };
  if (!held.text?.includes(`[file f ${second.fileId} · shot-2.png`)) throw new Error(`the image-only follow-up reached the CLI as ${JSON.stringify(held.text)}`);
  const turns = promptsOf(work, run.id);
  if (turns.length !== 2 || turns[1]?.text !== held.text) throw new Error(`the CLI had ${turns.length} turns`);
  const card = events.find((e) => e.type === "message.created" && e.data.message.id === done.resultMessageId)?.data.message;
  if (card?.cardV2?.intent !== "done") throw new Error("no done card for the request");
  if (runsIn(events, topicId).some((r: any) => r.id !== run.id)) throw new Error("a second run started in the topic");
  step("request with a quote and an image: the CLI got the quote and the file by id; an image-only follow-up reached the running CLI as its file line");
  return { runId: run.id, turns: turns.length };
}

/**
 * 机 (one work folder and branch per member and topic, PR #200): a member's second run in a
 * topic works in the first run's folder on its branch and finds its uncommitted file; another
 * topic gets another folder and branch.
 */
async function workLineCheck({ owner, groupId, work, events, ws }: Landed) {
  const repo = join(TMP, "repo");
  mkdirSync(repo, { recursive: true });
  const git = (...a: string[]) => {
    const r = spawnSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@localhost", ...a], { cwd: repo, encoding: "utf8" });
    if (r.status !== 0) throw new Error(`git ${a.join(" ")}: ${r.stderr}`);
  };
  git("init", "--quiet", "-b", "main");
  writeFileSync(join(repo, "README"), "e2e\n");
  git("add", "-A");
  git("commit", "--quiet", "-m", "init");
  const gb = await call("POST", "/v1/bots", { token: owner, body: { name: "git", role: "开发" } });
  await call("PUT", `/v1/bots/${gb.member.id}/runner`, { token: owner, body: { runtime: "claude", labels: [LABEL], repo: { path: repo } } });
  const meta = (runId: string) => JSON.parse(readFileSync(join(work, "runs", runId, "run.json"), "utf8")) as { line?: string; branch?: string };
  const seen = (runId: string) => JSON.parse(readFileSync(join(workOf(work, runId), "seefile.json"), "utf8")) as { kept: boolean; branch: string; status: string };

  const a = await say(events, ws, groupId, "@git keepfile", { mentions: [gb.member.id] });
  const a1 = await ranDone(events, a.topicId, "work line run 1");
  await say(events, ws, groupId, "@git seefile", { topicId: a.topicId, mentions: [gb.member.id] });
  const a2 = await ranDone(events, a.topicId, "work line run 2", [a1.id]);
  const b = await say(events, ws, groupId, "@git seefile", { mentions: [gb.member.id] });
  const b1 = await ranDone(events, b.topicId, "work line, other topic");

  const [m1, m2, mb] = [meta(a1.id), meta(a2.id), meta(b1.id)];
  if (!m1.line || !m1.branch?.startsWith("bot/")) throw new Error(`run 1 has no work line: ${JSON.stringify(m1)}`);
  if (m2.line !== m1.line || m2.branch !== m1.branch) throw new Error(`run 2 works elsewhere: ${JSON.stringify({ m1, m2 })}`);
  const s2 = seen(a2.id);
  if (!s2.kept || !s2.status.includes("kept.txt")) throw new Error(`run 2 does not see run 1's uncommitted file: ${JSON.stringify(s2)}`);
  if (s2.branch !== m1.branch) throw new Error(`run 2 is on ${s2.branch}, not ${m1.branch}`);
  if (mb.line === m1.line || mb.branch === m1.branch) throw new Error("another topic shares the work line");
  const sb = seen(b1.id);
  if (sb.kept || sb.branch !== mb.branch) throw new Error(`the other topic's folder is not its own: ${JSON.stringify(sb)}`);
  step("work line: run 2 in the topic used run 1's folder and branch and saw its uncommitted file; another topic got its own");
  return { branch: m1.branch, otherBranch: mb.branch };
}

/**
 * 盯 4.1 (memory audience, PR #198): an AI member's lesson learned in one group is readable by
 * that group's members only, and goes into a run's prompt only where everyone is in that group.
 */
async function memoryAudienceCheck({ owner, alice, bot, events, ws, work }: Landed) {
  const invite = await call("POST", "/v1/invites", { token: owner, body: { name: "Carol", kind: "human", level: "member" } });
  const carol = (await call("POST", "/v1/auth/redeem", { body: { inviteCode: invite.code, deviceName: "e2e" } })) as Person;
  const g1 = (await call("POST", "/v1/groups", { token: owner, body: { name: "学到的群", memberIds: [alice.member.id, bot.member.id] } })).group.id as string;
  const g2 = (await call("POST", "/v1/groups", { token: owner, body: { name: "别的群", memberIds: [carol.member.id, bot.member.id] } })).group.id as string;
  const marker = `lesson-${crypto.randomUUID().slice(0, 8)}`;
  const learn = await say(events, ws, g1, `@dev memo:${marker}`, { mentions: [bot.member.id] });
  const learned = await ranDone(events, learn.topicId, "memory: learn");
  const noted = JSON.parse(readFileSync(join(workOf(work, learned.id), "remember.json"), "utf8"));
  if (noted.code !== 0) throw new Error(`chat-run memory note failed: ${JSON.stringify(noted)}`);
  const list = async (token: string) =>
    ((await call("GET", `/v1/memories?scope=${encodeURIComponent(`member:${bot.member.id}`)}&status=all&limit=500`, { token })).memories ?? []) as any[];
  const item = (await list(alice.token)).find((m) => m.text === marker);
  if (!item) throw new Error("a member of the group cannot read the lesson learned there");
  if (item.fromGroup !== g1) throw new Error(`the lesson has fromGroup ${item.fromGroup}, not the group it was learned in`);
  if ((await list(carol.token)).some((m) => m.text === marker)) throw new Error("a person outside the group can list the lesson");
  const got = await call("GET", `/v1/memories/${item.id}`, { token: carol.token }).then(
    () => "read",
    (e) => String(e),
  );
  if (!got.includes("404")) throw new Error(`a person outside the group reads the lesson: ${got}`);

  // Injection: a run in the group carries it, a run for Carol in her group does not.
  const inGroup = await say(events, ws, g1, "@dev 看看", { mentions: [bot.member.id] });
  const r1 = await ranDone(events, inGroup.topicId, "memory: run in the group");
  if (!promptsOf(work, r1.id)[0]?.text.includes(marker)) throw new Error("the lesson is not in the prompt of a run in its group");
  const c = await socket(carol.token);
  try {
    const outside = await say(c.events, c.ws, g2, "@dev 看看", { mentions: [bot.member.id] });
    const r2 = await ranDone(c.events, outside.topicId, "memory: run for a person outside");
    const prompt = promptsOf(work, r2.id)[0]?.text;
    if (!prompt) throw new Error("the run outside the group has no prompt");
    if (prompt.includes(marker)) throw new Error("the lesson went into the prompt of a run for a person outside its group");
  } finally {
    c.ws.close();
  }
  step("memory audience: a lesson learned in a group is read and injected there, not for a person outside it");
  return { memoryId: item.id, fromGroup: item.fromGroup };
}

/**
 * PR #103: an approval card changes in place after 同意 (button) and after 拒绝 (text), and a card
 * nobody answered becomes 没用上 when its run ends.
 */
async function approvalCardCheck({ alice, bot, groupId, work, events, ws }: Landed) {
  const TITLE = "需要你确认：合并到 main";
  const cardsIn = (topicId: string) =>
    events.filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === bot.member.id && e.data.message.card?.title === TITLE).map((e) => e.data.message);
  const updateOf = (messageId: string) => events.filter((e) => e.type === "message.updated" && e.data.messageId === messageId && e.data.card).at(-1)?.data;
  const closed = async (messageId: string, title: string, line: string) => {
    const u = await until(`card closed as ${title}`, () => {
      const d = updateOf(messageId);
      return d?.card.title === title ? d : undefined;
    });
    if ((u.card.actions ?? []).length !== 0) throw new Error(`the closed card still has buttons: ${JSON.stringify(u.card.actions)}`);
    if (!u.body.includes(line)) throw new Error(`the closed card says ${JSON.stringify(u.body)}, not ${line}`);
  };

  // "gated twice": two gated commands; the first is approved with the button, the second denied in words.
  const t = await say(events, ws, groupId, "@dev gated twice", { mentions: [bot.member.id] });
  const c1 = await until("first approval card", () => cardsIn(t.topicId)[0], 60_000);
  const approve = c1.card.actions.find((a: any) => a.id.startsWith("approve:"));
  if (!approve) throw new Error(`the approval card has no 同意: ${JSON.stringify(c1.card.actions)}`);
  const rid = `landed-${++landedRid}`;
  ws.send(JSON.stringify({ t: "card.action", rid, groupId, messageId: c1.id, actionId: approve.id }));
  const pressed = await until("card.action ack", () => events.find((e) => e.frame?.rid === rid));
  if (!pressed.frame.ok) throw new Error(`pressing 同意 failed: ${JSON.stringify(pressed.frame.error)}`);
  await closed(c1.id, "已同意：合并到 main", "Alice 同意了");
  const c2 = await until("second approval card", () => cardsIn(t.topicId)[1], 60_000);
  await say(events, ws, groupId, "拒绝", { topicId: t.topicId });
  await closed(c2.id, "已拒绝：合并到 main", "Alice 拒绝了");
  const run = await ranDone(events, t.topicId, "approvals");
  const gate = JSON.parse(readFileSync(join(workOf(work, run.id), "gate-1.json"), "utf8")) as { commands: { command: string; denied: string | null }[] };
  const [merge12, ls, merge13] = gate.commands;
  if (merge12?.denied || ls?.denied || !merge13?.denied) throw new Error(`the CLI's commands were decided ${JSON.stringify(gate.commands)}`);

  // Nobody answers: the run is stopped and the card says it was not used.
  const u = await say(events, ws, groupId, "@dev gated", { mentions: [bot.member.id] });
  const c3 = await until("unanswered approval card", () => cardsIn(u.topicId)[0], 60_000);
  const waiting = runsIn(events, u.topicId).at(-1);
  await call("POST", `/v1/runs/${waiting.id}/cancel`, { token: alice.token });
  await closed(c3.id, "没用上：合并到 main", "这件事结束了，没等到回答");
  step("approval cards: 同意 and 拒绝 close the card in place; an unanswered one says 没用上 when the run ends");
  return { approved: c1.id, denied: c2.id, unused: c3.id };
}

/**
 * PR #103 (设置 › 模型): a member without a model of its own runs with the team's model and
 * 思考强度 for its runtime; a member's own model still wins.
 */
async function teamModelCheck({ owner, groupId, bot, work, events, ws }: Landed) {
  const argv = (runId: string) => JSON.parse(readFileSync(join(workOf(work, runId), "argv.json"), "utf8")) as string[];
  const arg = (a: string[], name: string) => (a.includes(name) ? a[a.indexOf(name) + 1] : undefined);
  await call("PUT", "/v1/settings/models/claude", { token: owner, body: { model: "team-model-e2e", effort: "high" } });
  try {
    const shown = ((await call("GET", "/v1/settings/models", { token: owner })).workers as any[]).find((w) => w.runtime === "claude");
    if (shown?.model !== "team-model-e2e") throw new Error(`设置 › 模型 shows ${JSON.stringify(shown)}`);
    const t = await say(events, ws, groupId, "@dev 你好", { mentions: [bot.member.id] });
    const r = await ranDone(events, t.topicId, "team model");
    const a = argv(r.id);
    if (arg(a, "--model") !== "team-model-e2e" || arg(a, "--effort") !== "high") throw new Error(`the run without its own model ran with ${JSON.stringify(a)}`);
    const own = await call("POST", "/v1/bots", { token: owner, body: { name: "own", role: "开发" } });
    await call("PUT", `/v1/bots/${own.member.id}/runner`, { token: owner, body: { runtime: "claude", labels: [LABEL], model: "own-model-e2e" } });
    const t2 = await say(events, ws, groupId, "@own 你好", { mentions: [own.member.id] });
    const r2 = await ranDone(events, t2.topicId, "own model");
    if (arg(argv(r2.id), "--model") !== "own-model-e2e") throw new Error(`a member's own model lost to the team's: ${JSON.stringify(argv(r2.id))}`);
  } finally {
    await call("PUT", "/v1/settings/models/claude", { token: owner, body: { model: null, effort: null } });
  }
  step("team model: a member without its own model ran with the team's model and 思考强度; its own model wins");
  return { model: "team-model-e2e", effort: "high" };
}

/** A new AI member on this run's machine with the runtime `runtime`; returns its member id. */
async function aiMember(owner: string, name: string, runtime: string): Promise<string> {
  const b = await call("POST", "/v1/bots", { token: owner, body: { name, role: "开发" } });
  await call("PUT", `/v1/bots/${b.member.id}/runner`, { token: owner, body: { runtime, labels: [LABEL] } });
  return b.member.id as string;
}

/** The stored message `id` as the socket saw it created. */
const messageOf = (events: any[], id: string | undefined) => events.find((e) => e.type === "message.created" && e.data.message.id === id)?.data.message;

/**
 * Routing (PR #215, 接 1.1, 接 1.4, 盯 3.1): a message that mentions two AI members starts a run
 * for each in the topic, each posts its own done card and the topic lists both as participants;
 * after another @, a follow-up without any @ from the requester goes to the member that ran last.
 */
async function routingCheck({ owner, groupId, bot, events, ws }: Landed) {
  const two = await aiMember(owner, "two", "claude");
  const ids = [bot.member.id, two];
  const m = await say(events, ws, groupId, "@dev @two 一起看看", { mentions: ids });
  const t = m.topicId;
  const both = await until(
    "routing: both members' runs end",
    () => {
      const ended = runsIn(events, t).filter((r: any) => ENDED.includes(r.status));
      const by = new Map(ended.map((r: any) => [r.botId as string, r]));
      return ids.every((id) => by.has(id)) ? ids.map((id) => by.get(id)) : undefined;
    },
    90_000,
  );
  for (const r of both) {
    if (r.status !== "done") throw new Error(`routing: the run of ${r.botId} ended ${r.status}`);
    const card = messageOf(events, r.resultMessageId);
    if (card?.authorId !== r.botId || card.cardV2?.intent !== "done") throw new Error(`routing: ${r.botId} has no done card of its own`);
  }
  const first = new Set(runsIn(events, t).map((r: any) => r.id as string));
  if (first.size !== 2) throw new Error(`routing: two mentions started ${first.size} runs`);
  await until("routing: the topic lists both members", () => {
    const p = events.filter((e) => e.type === "topic.updated" && e.data.topicId === t && e.data.participants).at(-1)?.data.participants as any[] | undefined;
    return p && ids.every((id) => p.some((x) => x.memberId === id)) ? p : undefined;
  });
  await say(events, ws, groupId, "@two 再看一眼", { topicId: t, mentions: [two] });
  const again = (await ranDone(events, t, "routing: @two again", [...first])) as any;
  if (again.botId !== two) throw new Error(`routing: @two reached ${again.botId}`);
  await say(events, ws, groupId, "还有一点", { topicId: t });
  const follow = (await ranDone(events, t, "routing: a follow-up without @", [...first, again.id])) as any;
  if (follow.botId !== two) throw new Error(`routing: a follow-up without @ went to ${follow.botId}, not the member that ran last`);
  step("routing: two mentions started a run for each member with its own done card, both listed in the topic; a follow-up without @ went to the member that ran last");
  return { topicId: t, runs: [...first, again.id, follow.id] };
}

/**
 * Gates by code for Codex and ACP (PR #202, 盯 2.1, 盯 2.10): a gated Codex run starts with
 * approval policy `untrusted`; the daemon answers `ls`, a file change and a GitHub MCP merge (no
 * built-in pattern) itself, and `git push origin main` waits for a person's card (拒绝 in words).
 * An ACP agent that asks before its tools gets a card for `gh pr merge` and none for a
 * file write.
 */
async function gatesByCodeCheck({ owner, groupId, work, events, ws }: Landed) {
  const TITLE = "需要你确认：合并到 main";
  const cardsIn = (topicId: string, author: string) =>
    events
      .filter((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.authorId === author && e.data.message.card?.title === TITLE)
      .map((e) => e.data.message);
  const closedAs = (messageId: string, title: string) =>
    until(`card closed as ${title}`, () => events.filter((e) => e.type === "message.updated" && e.data.messageId === messageId && e.data.card).at(-1)?.data.card.title === title);
  const press = async (card: any) => {
    const approve = card.card.actions.find((a: any) => a.id.startsWith("approve:"));
    if (!approve) throw new Error(`the approval card has no 同意: ${JSON.stringify(card.card.actions)}`);
    const rid = `landed-${++landedRid}`;
    ws.send(JSON.stringify({ t: "card.action", rid, groupId, messageId: card.id, actionId: approve.id }));
    const ack = await until("card.action ack", () => events.find((e) => e.frame?.rid === rid));
    if (!ack.frame.ok) throw new Error(`pressing 同意 failed: ${JSON.stringify(ack.frame.error)}`);
  };

  const gx = await aiMember(owner, "gx", "codex");
  const t = await say(events, ws, groupId, "@gx gated", { mentions: [gx] });
  const push = await until("codex: the push asks a person", () => cardsIn(t.topicId, gx)[0], 60_000);
  await say(events, ws, groupId, "拒绝", { topicId: t.topicId });
  await closedAs(push.id, "已拒绝：合并到 main");
  const run = await ranDone(events, t.topicId, "codex gates");
  const dir = workOf(work, run.id);
  const answers = JSON.parse(readFileSync(join(dir, "gated.json"), "utf8")) as any[];
  const decided = answers.map((a) => a?.decision ?? a?.action ?? null);
  if (JSON.stringify(decided) !== JSON.stringify(["accept", "accept", "decline", "accept"])) throw new Error(`codex: the requests were answered ${JSON.stringify(answers)}`);
  const policies = JSON.parse(readFileSync(join(dir, "policies.json"), "utf8")) as string[];
  if (policies.at(-1) !== "untrusted") throw new Error(`codex: a gated run started with approval policy ${JSON.stringify(policies)}`);
  if (cardsIn(t.topicId, gx).length !== 1) throw new Error(`codex: ${cardsIn(t.topicId, gx).length} cards for one gated request`);

  const ac = await aiMember(owner, "ac", "acp");
  const a = await say(events, ws, groupId, "@ac approve merge", { mentions: [ac] });
  const acpCard = await until("acp: gh pr merge asks a person", () => cardsIn(a.topicId, ac)[0], 60_000);
  await press(acpCard);
  await closedAs(acpCard.id, "已同意：合并到 main");
  const ar = await ranDone(events, a.topicId, "acp gates");
  const allowed = messageOf(events, ar.resultMessageId);
  if (!allowed?.body.includes("permission yes")) throw new Error(`acp: the approved merge answered ${JSON.stringify(allowed?.body)}`);
  const w = await say(events, ws, groupId, "@ac approve", { mentions: [ac] });
  const wr = await ranDone(events, w.topicId, "acp: a file write");
  if (cardsIn(w.topicId, ac).length !== 0) throw new Error("acp: a file write nobody gates asked a person");
  const written = messageOf(events, wr.resultMessageId);
  if (!written?.body.includes("permission yes")) throw new Error(`acp: the file write answered ${JSON.stringify(written?.body)}`);
  step("gates by code: Codex (untrusted) asked a person only for the push (拒绝); ACP asked for gh pr merge and not for a file write");
  return { codexRun: run.id, acpRuns: [ar.id, wr.id] };
}

/**
 * chat-run read commands (PR #199, 接 2.2, 接 2.3, 接 2.6, 接 4.1): a run makes a 产出 and a card
 * in one topic; a run in another topic reads the group's members, that topic's state and 产出,
 * the 产出 itself, the card and the group's decisions.
 */
async function readCommandsCheck({ alice, bot, groupId, work, events, ws }: Landed) {
  const made = await say(events, ws, groupId, "@dev artifact chatcard", { mentions: [bot.member.id] });
  const r1 = await ranDone(events, made.topicId, "read: a 产出 and a card");
  const madeDir = workOf(work, r1.id);
  const art = JSON.parse(readFileSync(join(madeDir, "chatrun-1.json"), "utf8"));
  if (art.code !== 0) throw new Error(`chat-run artifact create failed: ${JSON.stringify(art)}`);
  const cardId = JSON.parse(readFileSync(join(madeDir, "chatcard-1.json"), "utf8")).post.out?.messageId as string;
  if (!cardId) throw new Error("read: the first run posted no card");
  const ask = await say(events, ws, groupId, `@dev readback topic:${made.topicId} card:${cardId}`, { mentions: [bot.member.id] });
  const r2 = await ranDone(events, ask.topicId, "read: readback");
  const got = JSON.parse(readFileSync(join(workOf(work, r2.id), "readback.json"), "utf8"));
  const ok = (k: string) => {
    if (got[k]?.code !== 0 || !got[k].out) throw new Error(`chat-run ${k} failed: ${JSON.stringify(got[k])}`);
    return got[k].out;
  };
  const members = ok("members").members as any[];
  if (!members.some((x) => x.id === alice.member.id && x.ai === false) || !members.some((x) => x.id === bot.member.id && x.ai === true)) {
    throw new Error(`member list: ${JSON.stringify(members)}`);
  }
  const state = ok("state");
  if (state.topicId !== made.topicId || !String(state.text).includes("看板")) throw new Error(`topic state: ${JSON.stringify(state)}`);
  const listed = (ok("artifacts").artifacts as any[]).find((x) => x.title === "看板");
  if (!listed || listed.topicId !== made.topicId || listed.createdBy !== bot.member.id) throw new Error(`artifact list: ${JSON.stringify(got.artifacts.out)}`);
  const one = ok("artifact");
  if (one.artifact?.id !== listed.id || !(one.versions?.length >= 1) || !String(one.text ?? "").includes("app.js")) throw new Error(`artifact get: ${JSON.stringify(one).slice(0, 400)}`);
  const card = ok("card");
  if (card.topicId !== made.topicId || card.card?.title !== "chat-run card") throw new Error(`card get: ${JSON.stringify(card).slice(0, 400)}`);
  if (!Array.isArray(ok("decisions").decisions)) throw new Error(`decisions list: ${JSON.stringify(got.decisions)}`);
  step("chat-run read commands: a run in another topic read the members, that topic's state and 产出, the 产出's text, the card and the decisions");
  return { artifactId: listed.id, cardId };
}

/**
 * Waiting for an offline computer (PR #195, 机 2.2, 接 6.3): with the only computer offline, a
 * request queues with a note that names it instead of failing, and the run starts by itself
 * once the computer is back.
 */
async function offlineComputerCheck({ owner, groupId, bot, events, ws, machineId, daemon }: Landed) {
  if (!daemon) throw new Error("no daemon of this script to stop");
  await daemon.stop();
  await until(
    "the computer is offline",
    async () => !((await call("GET", "/v1/machines", { token: owner })) as any[]).find((x) => x.id === machineId)?.connected,
    60_000,
  );
  const t = await say(events, ws, groupId, "@dev 等电脑回来", { mentions: [bot.member.id] });
  const note = await until(
    "offline: the waiting note",
    () =>
      events.find(
        (e) => e.type === "message.created" && e.data.message.topicId === t.topicId && e.data.message.authorId === bot.member.id && e.data.message.body.includes("不在线，等它上线后自动开始"),
      )?.data.message,
    60_000,
  );
  const queued = runsIn(events, t.topicId).at(-1);
  if (!queued || ENDED.includes(queued.status) || queued.machineId) throw new Error(`offline: the run did not wait: ${JSON.stringify(queued)}`);
  daemon.start();
  const done = await ranDone(events, t.topicId, "offline: the run once the computer is back");
  if (done.id !== queued.id) throw new Error("offline: another run started instead of the waiting one");
  step("offline computer: the request waited with a note naming the computer and ran once the daemon was back");
  return { runId: done.id, note: note.body };
}

// ---------------------------------------------------------------------------
// --service: the daemon as the scheduled task of this user (Windows)
// ---------------------------------------------------------------------------

type Service = {
  exe: string;
  config: string;
  binDir: string;
  stateDir: string;
  taskName: string;
  image: string;
  asset: string;
  /** The daemon processes this script saw: none ran before it installed one, so they are its own. */
  pids: Set<number>;
};

function powershell(argv: string[]): { status: number | null; out: string } {
  const exe = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
  const r = spawnSync(exe, ["-NoProfile", "-ExecutionPolicy", "Bypass", ...argv], { encoding: "utf8" });
  return { status: r.status, out: `${r.stdout ?? ""}${r.stderr ?? ""}` };
}

/** The pids of the running daemons of the task (by image name). */
function daemonPids(s: Service): number[] {
  const r = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${s.image}`, "/FO", "CSV", "/NH"], { encoding: "utf8" });
  const pids = [...(r.stdout ?? "").matchAll(/^"[^"]+","(\d+)"/gm)].map((m) => Number(m[1]));
  for (const pid of pids) s.pids.add(pid);
  return pids;
}

/** The pids of the processes named `image` right now. */
function daemonPidsNow(image: string): number[] {
  const r = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${image}`, "/FO", "CSV", "/NH"], { encoding: "utf8" });
  return [...(r.stdout ?? "").matchAll(/^"[^"]+","(\d+)"/gm)].map((m) => Number(m[1]));
}

function taskExists(name: string): boolean {
  const schtasks = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "schtasks.exe");
  return spawnSync(schtasks, ["/Query", "/TN", name], { stdio: "ignore" }).status === 0;
}

function daemonLog(s: Service): string {
  try {
    return readFileSync(join(s.stateDir, "daemon.log"), "utf8");
  } catch {
    return "";
  }
}

/**
 * Installs the daemon with the PowerShell install command (files only), adds this run's runtimes
 * to the config it wrote, and registers the scheduled task. Everything is removed again when
 * the script ends.
 */
function installService(daemonBin: string, machineId: string, tokenFile: string, rest: string[]): Service {
  const product = JSON.parse(readFileSync(join(ROOT, "product.json"), "utf8")) as { slug: string; bundleId: string };
  const profile = process.env.USERPROFILE ?? "";
  const local = process.env.LOCALAPPDATA ?? "";
  if (!profile || !local) throw new Error("USERPROFILE and LOCALAPPDATA must be set");
  const configDir = join(profile, ".config", product.slug);
  const binDir = join(local, "Programs", product.slug);
  const stateDir = join(local, `${product.slug}-daemon`);
  const image = `${product.slug}-daemon.exe`;
  const s: Service = { exe: join(binDir, image), config: join(configDir, "daemon.toml"), binDir, stateDir, taskName: `${product.bundleId}.daemon`, image, asset: "", pids: new Set() };
  // This computer must not be a machine in any way: the task has one name per user, so an
  // install here would replace a real one and stop its daemon. A daemon can have its config
  // anywhere (`--config`, CHAT_DAEMON_CONFIG, under HOME instead of the profile), so the
  // task, its state folder and a running daemon are looked for too, not only the usual files.
  const running = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${image}`, "/FO", "CSV", "/NH"], { encoding: "utf8" }).stdout ?? "";
  const signs = [
    existsSync(configDir) && configDir,
    process.env.HOME && existsSync(join(process.env.HOME, ".config", product.slug)) && join(process.env.HOME, ".config", product.slug),
    process.env.CHAT_DAEMON_CONFIG && "CHAT_DAEMON_CONFIG is set",
    existsSync(s.exe) && s.exe,
    existsSync(stateDir) && stateDir,
    taskExists(s.taskName) && `the scheduled task ${s.taskName}`,
    running.includes(`"${image}"`) && `a running ${image}`,
  ].filter(Boolean);
  if (signs.length > 0) throw new Error(`this computer already has a daemon (${signs.join("; ")}): not touching it`);
  children.push({
    name: "service",
    pid: 0,
    kill: () => {
      // Only what this script made: its task (by the daemon it installed), the daemons it
      // saw start, and the three folders that were not there before.
      if (existsSync(s.exe)) spawnSync(s.exe, ["uninstall-service"], { stdio: "ignore" });
      const now = new Set(daemonPidsNow(image));
      for (const pid of s.pids) if (now.has(pid)) killTree(pid);
      for (const dir of [configDir, binDir, stateDir]) rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
    },
  });
  s.asset = join(TMP, `${product.slug}-daemon-0.0.0-x86_64-pc-windows-msvc.exe`);
  copyFileSync(daemonBin, s.asset);
  const install = powershell([
    "-File", join(ROOT, "scripts", "install-daemon.ps1"),
    "-ServerUrl", BASE, "-MachineId", machineId, "-Name", `mac-${LABEL}`, "-Labels", LABEL,
    "-DaemonFile", s.asset, "-TokenFile", tokenFile, "-NoService",
  ]);
  if (install.status !== 0 || !existsSync(s.config)) throw new Error(`install-daemon.ps1 failed (${install.status}): ${install.out.slice(-600)}`);
  step("install-daemon.ps1 installed the daemon and wrote its config and token");
  appendFileSync(s.config, `\n${rest.join("\n")}\n`);
  const reg = spawnSync(s.exe, ["install-service", "--config", s.config], { encoding: "utf8" });
  if (reg.status !== 0) throw new Error(`install-service failed (${reg.status}): ${(reg.stderr ?? "").slice(-600)}`);
  daemonPids(s);
  step(`the daemon is the scheduled task ${s.taskName}`);
  return s;
}

/**
 * What a machine owner's updates and a logoff do to the task's daemon, with runs going through
 * the server all the while:
 * 1. an update by the replace protocol while the daemon is idle: it restarts on the new file by
 *    itself and the next run works;
 * 2. the same while a run is active: the daemon keeps the run going on the old file for as long
 *    as it takes, and restarts only after the run ended;
 * 3. a logoff (the task and the daemon end at once) while a run is active: the run's CLI ends
 *    with the daemon; at the next logon (the task starts) the daemon reports the run as failed
 *    and takes new runs.
 * Then the uninstall: no task, no process.
 */
async function serviceChecks(
  s: Service,
  owner: string,
  groupId: string,
  alice: { token: string },
  bot: { member: { id: string } },
  machineId: string,
  work: string,
  events: any[],
  ws: WebSocket,
) {
  const out: Record<string, unknown> = {};
  let n = 0;
  const mention = async (body: string) => {
    const rid = `svc-${++n}`;
    ws.send(JSON.stringify({ t: "send", rid, groupId, clientMsgId: crypto.randomUUID(), body: `@dev ${body}`, mentions: [bot.member.id] }));
    const ack = await until(`send ack (${rid})`, () => events.find((e) => e.frame?.rid === rid));
    if (!ack.frame.ok) throw new Error(`send failed: ${JSON.stringify(ack.frame.error)}`);
    return ack.frame.result.message.topicId as string;
  };
  const runsOf = (topicId: string) => events.filter((e) => e.type === "run.updated" && e.data.run.topicId === topicId).map((e) => e.data.run);
  const quickRun = async (what: string) => {
    const topic = await mention("你好");
    const run = await until(`${what}: a run ends`, () => runsOf(topic).find((r: any) => ["done", "failed", "cancelled"].includes(r.status)), 90_000);
    if (run.status !== "done") throw new Error(`${what}: the run ended as ${run.status}`);
  };
  const hangingRun = async () => {
    for (let f = findFile(work, "fake.pid"); f; f = findFile(work, "fake.pid")) rmSync(f);
    const topic = await mention("hang");
    const run = await until("run running", () => runsOf(topic).find((r: any) => r.status === "running"), 60_000);
    const pidFile = await until("fake CLI hangs", () => {
      const f = findFile(work, "fake.pid");
      return f && readFileSync(f, "utf8").trim() ? f : undefined;
    }, 60_000);
    const cli = Number(readFileSync(pidFile, "utf8").trim());
    if (!alive(cli)) throw new Error("the fake CLI is not running");
    return { topic, id: run.id as string, cli, status: () => runsOf(topic).filter((r: any) => r.id === run.id).at(-1)?.status as string };
  };
  const connected = async () => ((await call("GET", "/v1/machines", { token: owner })) as any[]).find((m) => m.id === machineId)?.connected === true;
  const theDaemon = () => {
    const pids = daemonPids(s);
    if (pids.length !== 1) throw new Error(`${pids.length} daemons run: ${pids.join(", ")}`);
    return pids[0] as number;
  };
  /** Replaces the daemon's program file the way every updater does. */
  const update = () => {
    const r = powershell(["-File", join(ROOT, "scripts", "install-daemon.ps1"), "-Update", "-DaemonFile", s.asset]);
    if (r.status !== 0) throw new Error(`install-daemon.ps1 -Update failed (${r.status}): ${r.out.slice(-600)}`);
    if (readdirSync(s.binDir).filter((f) => f.includes(".old-")).length !== 1) throw new Error("the running program was not renamed aside");
  };
  const restarted = async (what: string, before: number, ms: number) => {
    const t0 = Date.now();
    const pid = await until(`${what}: the task started the new daemon`, () => {
      const pids = daemonPids(s);
      return pids.length === 1 && pids[0] !== before ? pids[0] : undefined;
    }, ms);
    await until(`${what}: the new daemon is connected`, connected, 90_000);
    await until(`${what}: the old program file is deleted`, () => readdirSync(s.binDir).filter((f) => f.includes(".old-")).length === 0, 60_000);
    return { pid, seconds: Math.round((Date.now() - t0) / 1000) };
  };
  try {
    // 1. Idle.
    const first = theDaemon();
    update();
    const idle = await restarted("update while idle", first, 300_000);
    await quickRun("after the idle update");
    out.idleUpdate = { before: first, after: idle.pid, seconds: idle.seconds };
    step(`update while idle: the daemon restarted on the new file by itself after ${idle.seconds} s and ran a run`);

    // 2. During a run: more than two looks at the program file (one a minute) pass and the
    // daemon and the run are still there.
    const busy = await hangingRun();
    const second = theDaemon();
    update();
    for (const t0 = Date.now(); Date.now() - t0 < 150_000; await sleep(5000)) {
      if (!alive(second)) throw new Error("the daemon restarted while a run was active");
      if (!alive(busy.cli) || busy.status() !== "running") throw new Error(`the run did not survive the update: ${busy.status()}`);
    }
    if (theDaemon() !== second) throw new Error("another daemon runs");
    const example = join(TARGET, "debug", "examples", "cancel_run.exe");
    const stop = spawnSync(example, [BASE, join(TMP, "alice-token"), busy.id, join(TMP, "client-data")], { encoding: "utf8" });
    if (stop.status !== 0) throw new Error(`cancel_run failed (${stop.status})`);
    await until("the run is cancelled", () => busy.status() === "cancelled", 30_000);
    await until("the fake CLI is stopped", () => !alive(busy.cli), 30_000);
    const after = await restarted("update during a run", second, 300_000);
    await quickRun("after the update during a run");
    out.busyUpdate = { before: second, after: after.pid, heldSeconds: 150, restartSeconds: after.seconds };
    step(`update during a run: the daemon kept the run for 150 s, and restarted ${after.seconds} s after the run was stopped`);

    // 3. Logoff and logon. A logoff ends every process of the session at once: the task and
    // the daemon are ended, not asked. The run's CLI must end with the daemon (nothing is left
    // to clean up at the next start).
    const lost = await hangingRun();
    const third = theDaemon();
    const schtasks = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "schtasks.exe");
    spawnSync(schtasks, ["/End", "/TN", s.taskName], { stdio: "ignore" });
    spawnSync("taskkill", ["/PID", String(third), "/F"], { stdio: "ignore" });
    await until("the daemon is gone", () => !alive(third), 15_000);
    await until("the run's CLI ended with the daemon", () => !alive(lost.cli), 15_000);
    // The logon: the task starts (its every-minute trigger may have done it already).
    spawnSync(schtasks, ["/Run", "/TN", s.taskName], { stdio: "ignore" });
    const back = await until("the task started the daemon again", () => {
      const pids = daemonPids(s);
      return pids.length === 1 && pids[0] !== third ? pids[0] : undefined;
    }, 120_000);
    await until("the daemon is connected again", connected, 90_000);
    const ended = await until("the lost run is reported", () => (["failed", "cancelled", "done"].includes(lost.status()) ? lost.status() : undefined), 120_000);
    if (ended !== "failed") throw new Error(`the run the logoff ended is ${ended}, not failed`);
    await quickRun("after the logon");
    out.logoff = { before: third, after: back, lostRun: ended };
    step("logoff during a run: the CLI ended with the daemon; after the logon the daemon reported the run as failed and ran a new one");

    // 4. Uninstall.
    const un = spawnSync(s.exe, ["uninstall-service"], { encoding: "utf8" });
    if (un.status !== 0) throw new Error(`uninstall-service failed (${un.status}): ${(un.stderr ?? "").slice(-400)}`);
    await until("no daemon runs", () => daemonPids(s).length === 0, 30_000);
    if (taskExists(s.taskName)) throw new Error("the task is still registered");
    await until("the machine is disconnected", async () => !(await connected()), 60_000);
    step("uninstall-service: no task, no daemon, the machine is offline");
    const log = daemonLog(s);
    if (log.includes(readFileSync(join(dirname(s.config), "machine-token"), "utf8").trim())) throw new Error("the machine token is in the daemon log");
    return out;
  } catch (err) {
    console.error(`--- daemon.log (last lines)\n${daemonLog(s).split(/\r?\n/).slice(-60).join("\n")}`);
    throw err;
  }
}

let code = 0;
try {
  await main();
} catch (err) {
  code = 1;
  report.ok = false;
  report.error = String(err);
  console.error(`FAILED: ${String(err)}`);
} finally {
  stopAll();
  for (const c of cleanup) await c().catch((e) => console.error(`cleanup: ${String(e)}`));
  const out = join(TMP, "report.json");
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`report: ${out}`);
  if (!KEEP && code === 0) {
    await sleep(500);
    rmSync(join(TMP, "work"), { recursive: true, force: true });
  }
}
process.exit(code);
