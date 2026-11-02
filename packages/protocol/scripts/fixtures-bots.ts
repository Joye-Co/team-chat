// Bot run and machine fixtures. Called by gen-fixtures.ts. The machine frames come from the
// daemon's golden files (apps/daemon/tests/fixtures/machine/*.jsonl), so both sides share them.
import { readFileSync } from "node:fs";
import { join } from "node:path";

type Obj = Record<string, unknown>;
export type BotsCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  T0: number;
  ID: Record<string, string>;
  base: Obj;
};

function lines(name: string): Obj[] {
  const p = join(import.meta.dir, "../../../apps/daemon/tests/fixtures/machine", name);
  return readFileSync(p, "utf8")
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l) => JSON.parse(l) as Obj);
}

export function addBotsFixtures({ ok, bad, T0, ID, base }: BotsCtx) {
  const R = { run: "01JBX0RUN000000000000000AU", machine: "01JBX0MACHINE00000000000AV" };

  // Machine protocol frames (daemon golden files).
  const counts = new Map<string, number>();
  for (const [file, kind, dir] of [["up.jsonl", "MachineUpFrame", "up"], ["down.jsonl", "MachineDownFrame", "down"]] as const) {
    for (const f of lines(file)) {
      const t = String(f.t);
      const n = (counts.get(`${dir}:${t}`) ?? 0) + 1;
      counts.set(`${dir}:${t}`, n);
      ok(`machine/${dir}-${t}${n > 1 ? `-${n}` : ""}.json`, kind, f, "daemon golden frame");
      if (t === "run.event") {
        const ev = f.event as Obj;
        ok(`machine/run-event-${ev.type}${n > 1 ? `-${n}` : ""}.json`, "RunEvent", ev);
      }
    }
  }
  bad("machine/up-hello-wrong-protocol.json", "MachineUpFrame", { ...lines("up.jsonl")[0], protocol: 2 }, "unknown machine protocol");
  bad("machine/down-run-assign-missing-lease.json", "MachineDownFrame", { t: "run.assign", runId: "r1", botMemberId: ID.bot, groupId: ID.general, topicId: ID.topic, runtime: "codex", prompt: "x", envAllowlist: [] }, "missing leaseMs");

  // Runs
  const run = {
    id: R.run,
    botId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    machineId: R.machine,
    status: "done",
    requestedBy: ID.owner,
    createdAt: T0 + 10_000,
    startedAt: T0 + 11_000,
    endedAt: T0 + 70_000,
    updatedAt: T0 + 70_000,
    summary: "修好了 sync 测试",
  };
  const created = { id: R.run, botId: ID.bot, groupId: ID.general, topicId: ID.topic, status: "created", createdAt: T0, updatedAt: T0 };
  ok("entities/bot-run.json", "BotRun", run);
  ok("entities/bot-run-created.json", "BotRun", created, "no machine yet");
  bad("entities/bot-run-bad-status.json", "BotRun", { ...created, status: "sleeping" }, "unknown run status");
  ok("events/run.updated.json", "Event", { ...base, actorId: ID.bot, type: "run.updated", data: { run } });
  ok("events/run.updated-waiting.json", "Event", { ...base, actorId: ID.bot, type: "run.updated", data: { run: { ...run, status: "waiting", endedAt: undefined, summary: undefined } } }, "the run asked a question", );

  const entries = [
    { t: "run.event", runId: R.run, seq: 1, at: T0 + 11_000, event: { type: "status", state: "running" } },
    { t: "run.event", runId: R.run, seq: 2, at: T0 + 12_000, event: { type: "tool.call", itemId: "c1", tool: "bash", title: "cargo test", input: { command: "cargo test" } } },
    { t: "run.done", runId: R.run, seq: 3, at: T0 + 70_000, status: "succeeded", exitCode: 0, resultText: "修好了", summary: "修好了", durationMs: 59_000, usage: { inputTokens: 100, outputTokens: 20 } },
    { t: "run.session", runId: R.run, seq: 4, at: T0 + 71_000, ok: true, source: "cli", bytes: 1000, gzipBytes: 300 },
  ];
  ok("entities/run-log-entry.json", "RunLogEntry", entries[1]);
  bad("entities/run-log-entry-hello.json", "RunLogEntry", lines("up.jsonl")[0], "hello is not a run log entry");
  ok("http/run-events-response.json", "RunEventsResponse", { run, entries, hasMore: false, lastSeq: 4 });
  ok("http/run-events-response-empty.json", "RunEventsResponse", { run: created, entries: [], hasMore: false, lastSeq: 0 });

  // Runner-backed bots
  ok("http/set-bot-runner-request.json", "SetBotRunnerRequest", { runtime: "codex", labels: ["mac", "main"], repo: { url: "https://github.com/Joye-Co/team-chat" }, baseBranch: "main", model: "gpt-5.5", envAllowlist: ["GITHUB_TOKEN"] });
  ok("http/set-bot-runner-request-minimal.json", "SetBotRunnerRequest", { runtime: "claude" }, "scratch folder, any machine");
  bad("http/set-bot-runner-request-missing-runtime.json", "SetBotRunnerRequest", { labels: ["mac"] }, "missing runtime");
  ok("http/bot-runner.json", "BotRunner", { botId: ID.bot, runtime: "codex", labels: ["mac"], repo: { path: "/Users/joye/code/team-chat" }, baseBranch: "main", envAllowlist: [], enabled: true, updatedAt: T0 });

  // Machines
  const hello = lines("up.jsonl")[0] as { machine: Obj; health: Obj; clis: Obj[] };
  const machine = {
    id: R.machine,
    name: "mac-mini",
    labels: ["mac", "main"],
    createdBy: ID.owner,
    createdAt: T0,
    connected: true,
    healthy: true,
    lastHeartbeatAt: T0 + 30_000,
    daemonVersion: "0.1.0",
    os: "macos",
    arch: "aarch64",
    maxParallelRuns: 2,
    health: hello.health,
    clis: hello.clis,
    runningRuns: [R.run],
  };
  const offline = { id: R.machine, name: "laptop", labels: [], createdBy: ID.owner, createdAt: T0, connected: false, healthy: false, runningRuns: [] };
  ok("entities/machine.json", "Machine", machine);
  ok("entities/machine-never-connected.json", "Machine", offline, "made, daemon never connected");
  ok("http/create-machine-request.json", "CreateMachineRequest", { name: "mac-mini", labels: ["mac", "main"] });
  bad("http/create-machine-request-bad-label.json", "CreateMachineRequest", { name: "mac-mini", labels: ["has space"] }, "label with a space");
  ok("http/create-machine-response.json", "CreateMachineResponse", { machine: offline, token: "tm_MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM" }, "token is a fake placeholder");
  ok("http/machines-response.json", "MachinesResponse", [machine, { ...offline, revoked: true }]);
}
