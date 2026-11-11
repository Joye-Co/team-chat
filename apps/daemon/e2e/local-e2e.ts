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
  if (SERVICE) {
    service = installService(daemonBin, machine.machine.id, tokenFile, rest);
  } else {
    writeFileSync(config, [...identity, ...rest].join("\n"));
    start("daemon", daemonBin, ["run", "--config", config], { cwd: TMP });
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
    // The daemon reports the run as ended by its restart, and the server continues the work in
    // a new run of the same topic (the old one shows as cancelled: restarted), not as failed.
    const ended = await until("the lost run is reported", () => (["failed", "cancelled", "done"].includes(lost.status()) ? lost.status() : undefined), 120_000);
    if (ended !== "cancelled") throw new Error(`the run the logoff ended is ${ended}, not continued`);
    const resumed = await until("the work goes on in a new run", () => runsOf(lost.topic).find((r: any) => r.id !== lost.id && r.status === "running"), 120_000);
    const stopResumed = spawnSync(example, [BASE, join(TMP, "alice-token"), resumed.id, join(TMP, "client-data")], { encoding: "utf8" });
    if (stopResumed.status !== 0) throw new Error(`cancel_run failed (${stopResumed.status})`);
    await until("the continued run is cancelled", () => runsOf(lost.topic).filter((r: any) => r.id === resumed.id).at(-1)?.status === "cancelled", 30_000);
    await quickRun("after the logon");
    out.logoff = { before: third, after: back, lostRun: ended, continuedIn: resumed.id };
    step("logoff during a run: the CLI ended with the daemon; after the logon the work went on in a new run, and a new run ran");

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
