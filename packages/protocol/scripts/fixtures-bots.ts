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

  // Run plans
  const plan = [
    { text: "写标题和正文", state: "done" },
    { text: "生成 3 张配图", state: "done" },
    { text: "等 Joye 确认", state: "waiting" },
    { text: "周四 20:00 发出", state: "pending" },
  ];
  const planned = { ...created, status: "running", startedAt: T0, plan, updatedAt: T0 + 5_000 };
  ok("entities/bot-run-plan.json", "BotRun", planned, "an external bot's run with a plan, no machine");
  bad("entities/bot-run-plan-bad-state.json", "BotRun", { ...planned, plan: [{ text: "x", state: "skipped" }] }, "unknown plan state");
  ok("events/run.updated-plan.json", "Event", { ...base, actorId: ID.bot, type: "run.updated", data: { run: planned } }, "a plan update replaces the whole plan");
  ok("entities/run-plan.json", "RunPlan", plan);
  ok("entities/run-plan-empty.json", "RunPlan", [], "clears the plan");
  ok("http/start-run-request.json", "StartRunRequest", { groupId: ID.general, topicId: ID.topic, plan });
  ok("http/start-run-request-minimal.json", "StartRunRequest", { groupId: ID.general, topicId: ID.topic });
  bad("http/start-run-request-missing-topic.json", "StartRunRequest", { groupId: ID.general }, "missing topicId");
  ok("http/set-run-plan-request.json", "SetRunPlanRequest", { plan });
  ok("http/set-run-plan-request-clear.json", "SetRunPlanRequest", { plan: [] }, "an empty plan clears it");
  ok("http/set-run-plan-request-text-max.json", "SetRunPlanRequest", { plan: [{ text: "字".repeat(200), state: "current" }] }, "200 code points");
  bad("http/set-run-plan-request-text-too-long.json", "SetRunPlanRequest", { plan: [{ text: "字".repeat(201), state: "current" }] }, "item text over 200 code points");
  bad("http/set-run-plan-request-empty-text.json", "SetRunPlanRequest", { plan: [{ text: "", state: "pending" }] }, "empty item text");
  bad("http/set-run-plan-request-bad-state.json", "SetRunPlanRequest", { plan: [{ text: "x", state: "in_progress" }] }, "provider state names are not plan states");
  bad("http/set-run-plan-request-too-many.json", "SetRunPlanRequest", { plan: Array.from({ length: 31 }, (_, i) => ({ text: `step ${i + 1}`, state: "pending" })) }, "more than 30 items");
  bad("http/set-run-plan-request-missing-plan.json", "SetRunPlanRequest", {}, "missing plan");
  ok("http/end-run-request.json", "EndRunRequest", { status: "done", summary: "已排期，周四 20:00 发出" });
  ok("http/end-run-request-minimal.json", "EndRunRequest", { status: "failed" });
  bad("http/end-run-request-running.json", "EndRunRequest", { status: "running" }, "only done, failed or cancelled end a run");

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

  // Run context
  const manifest = { resumed: false, chars: 1830, firstSeq: 1, lastSeq: 212, messages: 41, omitted: 170, files: 2, cards: 1, refs: 0 };
  ok("http/run-context.json", "RunContext", { prompt: "You are 小克, a member of a team chat. …", manifest });
  ok("http/run-context-resumed.json", "RunContext", { prompt: "Request from 张三:\n继续", manifest: { resumed: true, chars: 0, messages: 0, omitted: 0, files: 0, cards: 0, refs: 0 } }, "nothing new since the last run");
  bad("http/run-context-missing-manifest.json", "RunContext", { prompt: "x" }, "manifest is required");
  const cm = { id: ID.msg, seq: 150, authorId: ID.owner, authorName: "张三", at: T0, text: `[#150 m ${ID.msg} · 张三 · 11-08 14:02] 看下这张图\n  [file f 01JBX0FILE00000000000000AA · 设计稿.png · image/png · 210 KB · 1280×720]` };
  ok("http/topic-history-response.json", "TopicHistoryResponse", { messages: [cm], hasMore: true });
  ok("http/topic-message-response.json", "TopicMessageResponse", { message: cm });
  ok("http/context-search-response.json", "ContextSearchResponse", { hits: [{ messageId: ID.msg, topicId: ID.topic, topicTitle: "发布", seq: 150, authorName: "张三", at: T0, snippet: "看下这张图" }] });
  bad("http/context-search-response-missing-snippet.json", "ContextSearchResponse", { hits: [{ messageId: ID.msg, topicId: ID.topic, seq: 1, authorName: "x", at: T0 }] }, "snippet is required");

  // Runner-backed bots
  ok("http/set-bot-runner-request.json", "SetBotRunnerRequest", { runtime: "codex", labels: ["mac", "main"], repo: { url: "https://github.com/Joye-Co/team-chat" }, baseBranch: "main", model: "gpt-5.5", envAllowlist: ["GITHUB_TOKEN"] });
  ok("http/set-bot-runner-request-minimal.json", "SetBotRunnerRequest", { runtime: "claude" }, "scratch folder, any machine");
  bad("http/set-bot-runner-request-missing-runtime.json", "SetBotRunnerRequest", { labels: ["mac"] }, "missing runtime");
  ok("http/bot-runner.json", "BotRunner", { botId: ID.bot, runtime: "codex", labels: ["mac"], repo: { path: "/Users/me/code/team-chat" }, baseBranch: "main", envAllowlist: [], enabled: true, updatedAt: T0 });

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
  // Front desk
  const decision = { messageId: ID.msg, topicId: ID.topic, authorId: ID.owner, action: "dispatch", reason: "needs the repo", confidence: 0.9, brief: { goal: "修好登录页的测试", doneWhen: ["cargo test 通过"] }, model: "deepseek-chat", ms: 1800, inputTokens: 900, outputTokens: 60, at: T0 };
  ok("http/front-desk-decisions-response.json", "FrontDeskDecisionsResponse", { decisions: [decision, { ...decision, action: "reply", reason: "a greeting", brief: undefined, reply: "你好", replyMessageId: ID.msg2 }] });
  ok("http/front-desk-decisions-response-fallback.json", "FrontDeskDecisionsResponse", { decisions: [{ ...decision, confidence: 0, reason: "", brief: undefined, fallback: "model error: 500 overloaded" }] }, "the model failed: the request became a run");
  bad("http/front-desk-decisions-response-bad-action.json", "FrontDeskDecisionsResponse", { decisions: [{ ...decision, action: "escalate" }] }, "unknown action");

  // Models and effort (feature "effort")
  const codexCli = {
    runtime: "codex",
    kind: "codex",
    command: "codex",
    version: "codex-cli 0.159.0",
    available: true,
    models: [
      { id: "gpt-6-astra", name: "GPT-6-Astra", efforts: ["low", "medium", "high", "xhigh", "max"], defaultEffort: "low", isDefault: true },
      { id: "gpt-6-sol", name: "GPT-6-Sol", efforts: ["low", "medium", "high"], defaultEffort: "medium" },
    ],
    efforts: ["low", "medium", "high", "xhigh", "max"],
  };
  const claudeCli = { runtime: "claude", kind: "claude", command: "claude", available: true, models: [{ id: "fable" }, { id: "opus", isDefault: true }, { id: "sonnet" }, { id: "haiku" }], efforts: ["low", "medium", "high", "xhigh", "max"] };
  ok("entities/machine-models.json", "Machine", { ...machine, features: ["effort"], clis: [codexCli, claudeCli] }, "CLIs report models and efforts");
  ok("http/set-bot-runner-request-effort.json", "SetBotRunnerRequest", { runtime: "claude", model: "opus", effort: "high" });
  ok("http/set-bot-runner-request-effort-clear.json", "SetBotRunnerRequest", { runtime: "claude", effort: null }, "null clears the effort");
  bad("http/set-bot-runner-request-bad-effort.json", "SetBotRunnerRequest", { runtime: "claude", effort: "" }, "effort is not empty");
  ok("http/bot-runner-effort.json", "BotRunner", { botId: ID.bot, runtime: "claude", labels: [], model: "opus", effort: "high", envAllowlist: [], enabled: true, updatedAt: T0 });
  ok("http/create-machine-request.json", "CreateMachineRequest", { name: "mac-mini", labels: ["mac", "main"] });
  bad("http/create-machine-request-bad-label.json", "CreateMachineRequest", { name: "mac-mini", labels: ["has space"] }, "label with a space");
  ok("http/create-machine-response.json", "CreateMachineResponse", { machine: offline, token: "tm_MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM" }, "token is a fake placeholder");
  ok("http/machines-response.json", "MachinesResponse", [machine, { ...offline, revoked: true }]);
}
