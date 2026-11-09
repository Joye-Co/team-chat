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
// Needs: `bun install`, and the daemon binary (built here with cargo when missing).
// Tokens stay in memory or in files of mode 600 inside the temp folder; nothing is printed.
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
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
const BASE = REMOTE ?? `http://127.0.0.1:${PORT}`;
const TMP = mkdtempSync(join(tmpdir(), "bots-e2e-"));
// A label of its own on a shared env, so no other machine takes the run.
const LABEL = REMOTE ? `e2e-${crypto.randomUUID().slice(0, 8)}` : "e2e";
const report: Record<string, unknown> = { base: BASE, tmp: TMP, steps: [] as string[] };
const children: { name: string; pid: number; kill: () => void }[] = [];

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
  const child = spawn(cmd, argv, { cwd: opts.cwd, env: { ...process.env, ...opts.env }, stdio: ["ignore", fd, fd], detached: true });
  if (!child.pid) throw new Error(`${name} did not start`);
  const pid = child.pid;
  // Stop by the PID we started (its process group: wrangler runs workerd as a child).
  children.push({ name, pid, kill: () => { try { process.kill(-pid, "SIGTERM"); } catch { /* gone */ } } });
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
  const mig = spawnSync("bunx", ["wrangler", "d1", "migrations", "apply", "DB", "--local", "--persist-to", state], {
    cwd: server,
    encoding: "utf8",
    env: { ...process.env, CI: "1" },
  });
  if (mig.status !== 0) throw new Error(`d1 migrations failed: ${mig.stderr?.slice(-400)}`);
  step("local D1 migrated");
  start(
    "wrangler",
    "bunx",
    ["wrangler", "dev", "--port", String(PORT), "--inspector-port", String(PORT + 1000), "--ip", "127.0.0.1", "--persist-to", state, "--var", `ADMIN_SECRET:${adminSecret}`],
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
  const daemonBin = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"), "debug", "daemon");
  if (!existsSync(daemonBin)) {
    const b = spawnSync("cargo", ["build", "-p", "daemon"], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p daemon failed");
  }
  const fake = join(ROOT, "apps/daemon/tests/fake/fake_claude.py");
  chmodSync(fake, 0o755);
  const fakeCodex = join(ROOT, "apps/daemon/tests/fake/fake_codex.py");
  chmodSync(fakeCodex, 0o755);
  const work = join(TMP, "work");
  const claudeHome = join(TMP, "claude-home");
  mkdirSync(claudeHome, { recursive: true });
  const config = join(TMP, "daemon.toml");
  writeFileSync(
    config,
    [
      `server_url = "${BASE}"`,
      `token_file = "${tokenFile}"`,
      `name = "mac-${LABEL}"`,
      `labels = ["${LABEL}"]`,
      `work_root = "${work}"`,
      "max_parallel_runs = 2",
      "min_free_gb = 1",
      "",
      "[runtimes.claude]",
      'kind = "claude"',
      `command = "${fake}"`,
      `env = { CLAUDE_CONFIG_DIR = "${claudeHome}" }`,
      "",
      "[runtimes.codex]",
      'kind = "codex"',
      `command = "${fakeCodex}"`,
      'sandbox = "workspace-write"',
      'approval_policy = "never"',
      "",
      ...(VM
        ? [
            "[vm]",
            `kernel = "${VM_KERNEL}"`,
            `rootfs = "${VM_ROOTFS}"`,
            `dir = "${VM_DIR}"`,
            // The fake CLIs and the fake Claude config are not in the base image: share them.
            `shares = ["${claudeHome}", "${dirname(fake)}"]`,
            "",
          ]
        : []),
    ].join("\n"),
  );
  start("daemon", daemonBin, ["run", "--config", config], { cwd: TMP });
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
  const chatRun = JSON.parse(readFileSync(join(runDir, "work", "chatcard-1.json"), "utf8"));
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
  const argvOf = (runId: string) => JSON.parse(readFileSync(join(work, "runs", runId, "work", "argv.json"), "utf8")) as string[];
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
  const runWork = join(work, "runs", run.id, "work");
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
  const example = join(process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"), "debug", "examples", "cancel_run");
  if (!existsSync(example)) {
    const b = spawnSync("cargo", ["build", "-p", "chat-core", "--example", "cancel_run"], { cwd: ROOT, stdio: "inherit" });
    if (b.status !== 0) throw new Error("cargo build -p chat-core --example cancel_run failed");
  }
  const aliceTokenFile = secretFile("alice-token", alice.token);
  const cancel = (runId: string) => {
    const r = spawnSync(example, [BASE, aliceTokenFile, runId, join(TMP, "client-data")], { encoding: "utf8" });
    const line = (r.stdout ?? "").trim().split("\n").at(-1) ?? "";
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
