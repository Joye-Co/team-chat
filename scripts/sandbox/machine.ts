#!/usr/bin/env bun
// The sandbox's sample machine: speaks the machine protocol (docs/machine-protocol.md) with the
// token seed.ts made, like a daemon on a runner host would, so the owner's monitor window has a
// machine to watch. It boots a pretend VM for each run it gets, reports host and VM numbers with
// every heartbeat (metrics, "Metrics"), and ends each run after a few minutes with a short
// result. Codex is reported as not logged in, so its runs fail at once. Nothing here runs a CLI.
//
// Usage: bun scripts/sandbox/machine.ts --url <server> --token-file <file>
// up.ts starts it in the background (PID in .tmp/sandbox/machine.pid) and stops it with --stop.
import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const flag = (n: string) => {
  const i = args.indexOf(n);
  return i >= 0 ? args[i + 1] : undefined;
};
const url = flag("--url");
const tokenFile = flag("--token-file");
if (!url || !tokenFile) {
  console.error("usage: bun scripts/sandbox/machine.ts --url <server> --token-file <file>");
  process.exit(2);
}
const token = readFileSync(tokenFile, "utf8").trim();
const log = (l: string) => console.log(`${new Date().toISOString()} ${l}`);

const GIB = 1024 ** 3;
const HEARTBEAT_MS = 10_000;
const SLOTS = 12;
/** Feature "limits": how many runs at once as set in the app (`machine.limits`); the config's 12 until then. */
let slots = SLOTS;
const HOST_MEM = 67_216_998_400;
const HOST_OWN = 6.3 * GIB;
const DISK_TOTAL = 996_432_412_672;

interface Vm {
  runId: string;
  runtime: string;
  startedAt: number;
  bootMs: number;
  seq: number;
  mem: number;
  cpu: number;
  scratch: number;
  endsAt: number;
  ask: boolean;
}

const vms = new Map<string, Vm>();
// Feature "accounts" (machine-protocol.md "Accounts and keys"), played for the sandbox: Claude Code
// is signed in, Codex is not until the owner signs it in from 团队管理 › 订阅账号. A key the owner
// adds is sealed to this public key; the sample cannot open it, so it lists the name only.
const PUBLIC_KEY = "m1sCWjYp5PNXYviFXVLjvFI4pMAsv4xQ5MpYCD2+yeo=";
const logins: Record<string, { state: string; method?: string; email?: string; plan?: string }> = {
  claude: { state: "logged_in", method: "subscription", email: "owner@example.com", plan: "max" },
  codex: { state: "logged_out" },
};
const secrets = new Map<string, { last4: string; members: string[]; updatedAt: number }>();
const boots: number[] = [];
/** Every third run stops at a gated step and waits for a person (等你). */
let taken = 0;
let ws: WebSocket | null = null;
let cpu = 48;
let rx = 2.6e6;
let tx = 0.9e6;
const started = Date.now();

const jitter = (v: number, step: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v + (Math.random() - 0.5) * 2 * step));
const send = (f: unknown) => ws?.readyState === WebSocket.OPEN && ws.send(JSON.stringify(f));

function health() {
  const used = 96e9 + [...vms.values()].reduce((s, v) => s + v.scratch, 0);
  return {
    diskFreeBytes: Math.round(DISK_TOTAL - used),
    diskTotalBytes: DISK_TOTAL,
    load1: Math.round((cpu / 100) * 12 * 100) / 100,
    load5: Math.round((cpu / 100) * 11 * 100) / 100,
    load15: Math.round((cpu / 100) * 10 * 100) / 100,
    cpuCount: 12,
    activeRuns: vms.size,
  };
}

function metrics() {
  const now = Date.now();
  const runs = [...vms.values()].map((v) => {
    const booting = now - v.startedAt < v.bootMs;
    return {
      runId: v.runId,
      phase: booting ? "booting" : "running",
      startedAt: v.startedAt,
      ...(booting
        ? {}
        : {
            bootMs: v.bootMs,
            cpuCores: Math.round(v.cpu * 100) / 100,
            memBytes: Math.round(v.mem),
            scratchBytes: Math.round(v.scratch),
          }),
    };
  });
  const vmMem = runs.reduce((s, r) => s + (r.memBytes ?? 0), 0);
  const recent = boots.slice(-50);
  return {
    at: now,
    uptimeSecs: Math.round((now - started) / 1000),
    cpuPct: Math.round(cpu * 10) / 10,
    cpuThreads: 12,
    memTotalBytes: HOST_MEM,
    memAvailableBytes: Math.max(0, Math.round(HOST_MEM - HOST_OWN - vmMem)),
    netRxBps: Math.round(rx),
    netTxBps: Math.round(tx),
    vmSlots: slots,
    ...(recent.length
      ? { boot: { avgMs: Math.round(recent.reduce((a, b) => a + b, 0) / recent.length), maxMs: Math.max(...recent), samples: recent.length } }
      : {}),
    runs,
    logins: Object.entries(logins).map(([runtime, l]) => ({ runtime, ...l })),
    secrets: [...secrets.entries()].map(([name, x]) => ({ name, ...x })),
    usage: [
      {
        runtime: "claude",
        at: Date.now() - 60_000,
        windows: [
          { kind: "session", label: "Current session · resets in 2 hr 13 min", usedPct: 42 },
          { kind: "week", label: "Current week (all models) · resets Thu 09:00", usedPct: 31 },
          { kind: "week_model", label: "Current week (Opus) · resets Thu 09:00", usedPct: 58 },
        ],
      },
      ...(logins.codex.state === "logged_in"
        ? [{ runtime: "codex", at: Date.now() - 30_000, windows: [{ kind: "primary", label: "5 hours", usedPct: 12, resetsAt: Date.now() + 3 * 3600_000 }, { kind: "secondary", label: "7 days", usedPct: 23, resetsAt: Date.now() + 4 * 86_400_000 }] }]
        : []),
    ],
  };
}

function event(v: Vm, ev: unknown) {
  v.seq += 1;
  send({ t: "run.event", runId: v.runId, seq: v.seq, at: Date.now(), event: ev });
}

/**
 * A sample run's usage with its models, as a 1.3 daemon reports it: Claude Code with its own cost
 * per model (mostly cache reads), Codex with tokens only (the server prices them).
 */
function sampleUsage(runtime: string, tokens: number) {
  const input = Math.round(tokens * 0.85);
  const output = Math.round(tokens * 0.15);
  if (runtime === "claude") {
    const cost = Math.round(tokens * 0.0009) / 100;
    const main = { model: "claude-sonnet-4-5-20250929", inputTokens: Math.round(input * 0.1), outputTokens: output - 400, cacheReadTokens: Math.round(input * 0.8), cacheWriteTokens: input - Math.round(input * 0.1) - Math.round(input * 0.8), costUsd: Math.round(cost * 95) / 100 };
    const small = { model: "claude-haiku-4-5-20251001", inputTokens: 0, outputTokens: 400, costUsd: Math.round(cost * 5) / 100 };
    return { inputTokens: input, outputTokens: output, costUsd: cost, models: [main, small] };
  }
  return { inputTokens: input, outputTokens: output, models: [{ model: "gpt-5.1-codex", inputTokens: Math.round(input * 0.3), outputTokens: output, cacheReadTokens: input - Math.round(input * 0.3) }] };
}

/**
 * What a real run's log holds, so the run log (工作记录, 实时查看) has something to draw: what
 * the member said in Markdown with its acceptance block, a step that went well with a long
 * output, changed files and a step that failed.
 */
function sampleSteps(v: Vm) {
  const turnId = "t1";
  event(v, { type: "turn.started", turnId });
  event(v, { type: "message", itemId: "m1", role: "assistant", turnId, text: "先看一下现在的测试覆盖了哪些情况，再补缺的。" });
  event(v, { type: "tool.call", itemId: "s1", turnId, tool: "Bash", title: "Bash: bun run test -- test/login.test.ts", input: { command: "bun run test -- test/login.test.ts", description: "跑登录相关的测试" } });
  event(v, {
    type: "tool.result",
    itemId: "s1",
    turnId,
    ok: true,
    output: ["$ vitest run test/login.test.ts", "", " RUN  v4.1.12", "", " ✓ test/login.test.ts (6 tests) 412ms", "   ✓ 邀请码登录", "   ✓ 粘贴凭证登录", "   ✓ 服务器地址默认值", "", " Test Files  1 passed (1)", "      Tests  6 passed (6)", "   Duration  1.92s"].join("\n"),
  });
  event(v, { type: "tool.call", itemId: "s2", turnId, tool: "Edit", title: "Edit: test/login.test.ts", input: { file_path: "test/login.test.ts" } });
  event(v, { type: "tool.result", itemId: "s2", turnId, ok: true, output: "The file test/login.test.ts has been updated." });
  event(v, { type: "file.change", itemId: "s2:file", turnId, changes: [{ path: "test/login.test.ts", kind: "modify" }] });
  event(v, { type: "tool.call", itemId: "s3", turnId, tool: "Bash", title: "Bash: bun run test -- test/login.test.ts", input: { command: "bun run test -- test/login.test.ts" } });
  event(v, {
    type: "tool.result",
    itemId: "s3",
    turnId,
    ok: false,
    output: ["Exit code 1", " ✗ test/login.test.ts > 邀请码过期时提示重新要一个", "   AssertionError: expected '邀请码无效' to be '邀请码已过期'", "    at test/login.test.ts:88:31", "", " Test Files  1 failed (1)", "      Tests  1 failed | 6 passed (7)"].join("\n"),
  });
  event(v, {
    type: "message",
    itemId: "m2",
    role: "assistant",
    turnId,
    text: [
      "补了一条用例，发现一个**真问题**：邀请码过期时，界面提示的是「邀请码无效」。",
      "",
      "- **现状**：6 条用例通过，新加的 `邀请码过期` 这条失败。",
      "- **原因**：`login.rs` 把两种错误合成了一种。",
      "- **下一步**：拆开错误类型，再跑一遍。",
      "",
      "```acceptance",
      '[{ "text": "补齐登录页端到端测试", "state": "pending" }]',
      "```",
    ].join("\n"),
  });
}

function finish(v: Vm, ok: boolean, error?: string) {
  v.seq += 1;
  const tokens = Math.round(40_000 + Math.random() * 260_000);
  send({
    t: "run.done",
    runId: v.runId,
    seq: v.seq,
    at: Date.now(),
    status: ok ? "succeeded" : "failed",
    ...(ok
      ? { resultText: "（样例机器）做完了：这是沙盒里的演示结果。", summary: "（样例机器）做完了", usage: sampleUsage(v.runtime, tokens) }
      : { error: error ?? "failed" }),
    durationMs: Date.now() - v.startedAt,
  });
  v.seq += 1;
  send({ t: "run.session", runId: v.runId, seq: v.seq, at: Date.now(), ok: false, error: "no session on the sample machine" });
  vms.delete(v.runId);
  log(`run ${v.runId} ${ok ? "done" : "failed"}`);
}

function take(runId: string, runtime: string, seq = 0) {
  const now = Date.now();
  const boot = Math.round(1000 + Math.random() * 1100);
  const v: Vm = {
    runId,
    runtime,
    startedAt: now,
    bootMs: boot,
    seq,
    mem: (1.1 + Math.random() * 0.6) * GIB,
    cpu: 0.3 + Math.random(),
    scratch: (0.2 + Math.random() * 0.5) * GIB,
    endsAt: now + (1 + Math.random() * 3) * 60_000,
    ask: runtime !== "codex" && taken++ % 3 === 1,
  };
  vms.set(runId, v);
  if (runtime === "codex") {
    setTimeout(() => {
      if (vms.has(runId)) finish(v, false, "codex is not logged in on this machine (sample)");
    }, 3000);
    return;
  }
  setTimeout(() => {
    if (!vms.has(runId)) return;
    boots.push(boot);
    event(v, { type: "status", state: "running" });
    event(v, { type: "session", sessionId: `sample-${runId.slice(-8)}`, runtime: v.runtime });
    event(v, { type: "plan", items: [{ text: "读需求", state: "done" }, { text: "动手做", state: "current" }, { text: "自查", state: "pending" }] });
    sampleSteps(v);
    if (v.ask) {
      event(v, { type: "approval.request", approvalId: `sample-${runId.slice(-6)}`, title: "使用 GITHUB_TOKEN", detail: "（样例机器）演示：这一步等你批准" });
    }
  }, boot);
}

function tick() {
  cpu = jitter(cpu, 6, 18, 88);
  rx = jitter(rx, 0.9e6, 0.2e6, 8e6);
  tx = jitter(tx, 0.4e6, 0.05e6, 3e6);
  const now = Date.now();
  for (const v of [...vms.values()]) {
    v.cpu = jitter(v.cpu, 0.3, 0.05, 2.2);
    v.mem = jitter(v.mem, 0.05 * GIB, 0.8 * GIB, 2.2 * GIB);
    v.scratch += Math.random() * 20e6;
    if (!v.ask && now > v.endsAt) finish(v, Math.random() > 0.08);
  }
  send({ t: "heartbeat", at: now, health: health(), runs: [...vms.values()].map((v) => ({ runId: v.runId, lastSeq: v.seq, done: false })), metrics: metrics() });
}

function connect() {
  const wsUrl = `${url!.replace(/^http/, "ws").replace(/\/+$/, "")}/v1/machine/ws`;
  const sock = new WebSocket(wsUrl, { headers: { authorization: `Bearer ${token}` } } as unknown as string[]);
  ws = sock;
  sock.addEventListener("open", () => {
    send({
      t: "hello",
      protocol: 1,
      machine: { name: "runner-1", labels: ["sandbox"], daemonVersion: "1.2.7-sample", os: "linux", arch: "x86_64", maxParallelRuns: SLOTS, features: ["accounts", "limits"], publicKey: PUBLIC_KEY },
      clis: [
        { runtime: "claude", kind: "claude", command: "claude", version: "2.1.0 (Claude Code)", available: true },
        { runtime: "codex", kind: "codex", command: "codex", version: "codex-cli 0.159.0", available: true },
      ],
      health: health(),
      runs: [...vms.values()].map((v) => ({ runId: v.runId, lastSeq: v.seq, done: false })),
    });
  });
  sock.addEventListener("message", (e) => {
    const f = JSON.parse(String(e.data));
    switch (f.t) {
      case "welcome":
        log(`connected as ${f.machineId}`);
        // Runs the server still has open (a restart of this script): keep them going.
        for (const r of f.runs ?? []) if (!vms.has(r.runId)) take(r.runId, "claude", r.ackedSeq);
        tick();
        break;
      case "run.assign":
        if (vms.size >= slots) {
          send({ t: "run.reject", runId: f.runId, reason: "busy", message: "all VM slots are in use" });
          break;
        }
        send({ t: "run.accept", runId: f.runId });
        if (!vms.has(f.runId)) take(f.runId, f.runtime);
        log(`run ${f.runId} accepted (${f.runtime})`);
        break;
      case "run.approval": {
        const v = vms.get(f.runId);
        if (v) {
          event(v, { type: "approval.resolved", approvalId: `sample-${f.runId.slice(-6)}`, decision: f.decision });
          v.ask = false;
          v.endsAt = Date.now() + 60_000;
        }
        break;
      }
      case "run.cancel": {
        const v = vms.get(f.runId);
        if (v) {
          v.seq += 1;
          send({ t: "run.done", runId: v.runId, seq: v.seq, at: Date.now(), status: "cancelled", durationMs: Date.now() - v.startedAt });
          vms.delete(v.runId);
        }
        break;
      }
      case "account.login": {
        const status = (state: string, extra: Record<string, unknown> = {}) =>
          send({ t: "account.login.status", loginId: f.loginId, runtime: f.runtime, state, ...extra });
        if (f.op === "start") {
          status("starting");
          setTimeout(() => {
            if (f.runtime === "codex") {
              status("device_code", { url: "https://auth.openai.com/codex/device", userCode: "SAMP-12345" });
              // The sample signs in by itself after a while, as if the code was entered.
              setTimeout(() => {
                logins.codex = { state: "logged_in", method: "subscription", plan: "chatgpt" };
                status("done", { account: { plan: "chatgpt" } });
                tick();
              }, 12_000);
            } else {
              status("open_url", { url: "https://claude.com/cai/oauth/authorize?code=true&sample=1" });
            }
          }, 1500);
        } else if (f.op === "code") {
          status("checking");
          setTimeout(() => status("done", { account: { email: "owner@example.com", plan: "max" } }), 2000);
        } else if (f.op === "cancel") {
          status("cancelled");
        }
        break;
      }
      case "secret.set":
        secrets.set(f.name, { last4: "", members: f.members ?? [], updatedAt: Date.now() });
        log(`key ${f.name} kept (sealed; the sample cannot open it)`);
        tick();
        break;
      case "machine.limits":
        slots = f.maxParallelRuns ?? SLOTS;
        log(`up to ${slots} runs at once`);
        tick();
        break;
      case "secret.delete":
        secrets.delete(f.name);
        tick();
        break;
      case "error":
        log(`error: ${f.error?.code} ${f.error?.message}`);
        break;
    }
  });
  sock.addEventListener("close", () => {
    ws = null;
    log("link closed; reconnecting in 3 s");
    setTimeout(connect, 3000);
  });
  sock.addEventListener("error", () => {});
}

connect();
setInterval(tick, HEARTBEAT_MS);
