// Usable fixtures: roles (所有者, personal machines), GitHub sign-in, relays and chains, run
// phase and take-over. Called by gen-fixtures.ts. Readers take the new states as open strings,
// so the "unknown value" fixtures are valid.
export type UsableCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string, reason?: string) => void;
  T0: number;
  ID: Record<string, string>;
  base: { seq: number; groupId: string; at: number; actorId: string };
  topic: Record<string, unknown>;
};

export function addUsableFixtures({ ok, bad, T0, ID, base, topic }: UsableCtx) {
  const machineId = "01JBX0MACHINE00000000000AV";
  const runId = "01JBX0RUN000000000000000AU";
  const run2 = "01JBX0RUN000000000000000AW";
  const bot2 = "01JBX0BOT000000000000000AX";
  const chainId = "01JBX0CHAIN0000000000000AY";
  const relayId = "01JBX0RELAY0000000000000AZ";

  // -------------------------------------------------------------------------
  // Roles: 所有者 and personal machines
  // -------------------------------------------------------------------------

  const owner = { id: ID.owner, kind: "human", name: "Joye", level: "owner" };
  ok("entities/member-team-owner.json", "Member", { ...owner, teamOwner: true }, "a 所有者: level owner and teamOwner");
  ok("entities/member-admin.json", "Member", { ...owner, id: ID.alice, name: "Alice" }, "an admin: level owner without teamOwner");
  bad("entities/member-team-owner-string.json", "Member", { ...owner, teamOwner: "yes" }, "teamOwner is a boolean");
  ok("http/update-member-request-team-owner.json", "UpdateMemberRequest", { teamOwner: true });
  ok("http/update-member-request-admin.json", "UpdateMemberRequest", { level: "owner", teamOwner: false }, "make an admin who is not a 所有者");
  ok("http/team-roster-team-owner.json", "TeamRoster", {
    members: [
      { memberId: ID.owner, joinedAt: T0, devices: [], teamOwner: true },
      { memberId: ID.alice, joinedAt: T0 + 1000, addedBy: ID.owner, devices: [] },
    ],
    invites: [],
  });

  const machineBase = {
    id: machineId,
    name: "Alice 的 MacBook",
    labels: ["mac"],
    createdBy: ID.alice,
    createdAt: T0,
    connected: true,
    healthy: true,
    runningRuns: [],
  };
  ok("entities/machine-personal.json", "Machine", {
    ...machineBase,
    ownerId: ID.alice,
    personalBots: [{ botId: ID.bot, repoPath: "/Users/alice/code/app" }, { botId: bot2 }],
  }, "my own machine, as its owner sees it");
  ok("entities/machine-personal-other.json", "Machine", {
    id: machineId,
    name: "Alice 的 MacBook",
    labels: [],
    createdBy: ID.alice,
    createdAt: T0,
    connected: false,
    healthy: false,
    runningRuns: [],
    ownerId: ID.alice,
  }, "someone else's personal machine: the minimal row");
  bad("entities/machine-personal-bot-no-id.json", "Machine", { ...machineBase, personalBots: [{ repoPath: "/tmp/x" }] }, "a personal bot needs botId");
  ok("http/create-machine-request-personal.json", "CreateMachineRequest", { name: "我的 Mac", personal: true });
  ok("http/update-machine-request-name.json", "UpdateMachineRequest", { name: "构建机" });
  ok("http/update-machine-request-mine.json", "UpdateMachineRequest", { owner: "me" }, "team -> my own machine");
  ok("http/update-machine-request-team.json", "UpdateMachineRequest", { owner: null }, "my own machine -> the team");
  bad("http/update-machine-request-empty.json", "UpdateMachineRequest", {}, "name or owner is required");
  ok("entities/machine-cli-resume.json", "Machine", {
    id: "01JBX0MACHINE00000000000AW",
    name: "mac-mini",
    labels: ["mac"],
    createdBy: ID.owner,
    createdAt: T0,
    connected: true,
    healthy: true,
    lastHeartbeatAt: T0 + 30_000,
    daemonVersion: "0.1.0",
    os: "macos",
    arch: "aarch64",
    maxParallelRuns: 2,
    health: { diskFreeBytes: 120_000_000_000, diskTotalBytes: 494_000_000_000, load1: 1.5, load5: 1.2, load15: 0.9, cpuCount: 10, activeRuns: 0 },
    clis: [
      { runtime: "claude", kind: "claude", command: "claude", available: true, efforts: ["low", "high"], gates: true, resume: true, aside: true },
      { runtime: "opencode", kind: "acp", command: "opencode", version: "1.17.11", available: true, gates: true, resume: true, aside: false },
      { runtime: "gemini", kind: "acp", command: "gemini", available: true, resume: false, aside: false },
    ],
    runningRuns: [],
    features: ["effort", "gates", "resume", "aside"],
  }, "Usable: per-CLI resume and aside; an ACP agent that loads sessions but has no read-only mode");
  bad("http/update-machine-request-other-owner.json", "UpdateMachineRequest", { owner: ID.alice }, "a machine cannot be given to someone else");
  bad("http/update-machine-request-long-name.json", "UpdateMachineRequest", { name: "名".repeat(65) }, "name is 1..64");
  ok("http/set-machine-bot-request.json", "SetMachineBotRequest", { repoPath: "/Users/alice/code/app" });
  ok("http/set-machine-bot-request-empty.json", "SetMachineBotRequest", {}, "check the bot, keep the folder");
  ok("http/set-machine-bot-request-clear.json", "SetMachineBotRequest", { repoPath: null }, "null clears the folder");
  bad("http/set-machine-bot-request-empty-path.json", "SetMachineBotRequest", { repoPath: "" }, "a folder is 1..1024 characters");

  // -------------------------------------------------------------------------
  // Relays, chains, participants, code position, 产出 parent versions
  // -------------------------------------------------------------------------

  const participants = [
    { memberId: ID.bot, working: false, lastRunAt: T0 + 1000 },
    { memberId: bot2, working: true, lastRunAt: T0 + 5000 },
  ];
  const relay = {
    id: relayId,
    startedBy: ID.owner,
    goal: "给设置页加深色模式",
    current: 1,
    state: "running",
    updatedAt: T0 + 5000,
    steps: [
      { memberId: ID.bot, note: "写", state: "done", runId },
      { memberId: bot2, note: "审", state: "working", runId: run2 },
      { memberId: ID.bot, note: "按意见改", state: "pending" },
    ],
  };
  ok("entities/topic-relay.json", "Topic", { ...topic, participants, relay }, "two AI members, a relay at step 2 of 3");
  ok("entities/topic-relay-stopped-unknown.json", "Topic", {
    ...topic,
    relay: { ...relay, state: "parked", reason: "a_future_reason", stoppedBy: ID.owner, steps: relay.steps.map((s, i) => (i === 1 ? { ...s, state: "skipped" } : s)) },
  }, "unknown relay, step and reason values are read as open strings");
  bad("entities/topic-relay-no-steps.json", "Topic", { ...topic, relay: { ...relay, steps: undefined } }, "a relay has steps");
  bad("entities/topic-participant-no-working.json", "Topic", { ...topic, participants: [{ memberId: ID.bot, lastRunAt: T0 }] }, "a participant says whether it is working");
  ok("http/topic-relay.json", "TopicRelay", relay, "answer of the relay stop and retry routes");
  ok("http/topic-relay-paused.json", "TopicRelay", { ...relay, state: "paused", reason: "step_blocked" });
  ok("events/topic.updated-participants-relay.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, status: "in_progress", ownerId: bot2, participants, relay } });
  ok("events/topic.updated-relay-cleared.json", "Event", { ...base, type: "topic.updated", data: { topicId: ID.topic, relay: null } }, "relay null = none any more");
  ok("http/chain-stop-response.json", "ChainStopResponse", { stoppedRuns: 2 });
  bad("http/chain-stop-response-negative.json", "ChainStopResponse", { stoppedRuns: -1 }, "a count is not negative");
  ok("http/error-response-relay-member-unavailable.json", "ErrorResponse", {
    error: { code: "invalid", message: "these members cannot take this work now", reason: "relay.member_unavailable", memberIds: [bot2] },
  }, "a refused arrow message names who cannot take it");

  const version = {
    artifactId: "01JBX0ART000000000000000AM",
    version: 3,
    entry: "index.html",
    files: [{ path: "index.html", fileId: "01JBX0FILE00000000000000AH", size: 120, mime: "text/html" }],
    createdBy: ID.bot,
    createdAt: T0,
  };
  ok("entities/artifact-version-parent.json", "ArtifactVersion", { ...version, parentVersion: 1 }, "version 3 made from version 1");
  bad("entities/artifact-version-parent-string.json", "ArtifactVersion", { ...version, parentVersion: "1" }, "parentVersion is a number");
  const addVersion = { entry: "index.html", files: [{ path: "index.html", fileId: "01JBX0FILE00000000000000AH" }] };
  ok("http/add-artifact-version-request-parent.json", "AddArtifactVersionRequest", { ...addVersion, parentVersion: 2 });
  bad("http/add-artifact-version-request-parent-zero.json", "AddArtifactVersionRequest", { ...addVersion, parentVersion: 0 }, "versions start at 1");

  // -------------------------------------------------------------------------
  // Runs: requestedFor, chain, code, phase and take-over
  // -------------------------------------------------------------------------

  const run = {
    id: runId,
    botId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    machineId,
    status: "running",
    requestedBy: ID.owner,
    createdAt: T0,
    startedAt: T0 + 1000,
    updatedAt: T0 + 9000,
  };
  const handback = { by: ID.owner, heldMs: 720_000, commands: 3, files: [".env.example"], headFrom: "a1b2c3d", headTo: "d4e5f6a" };
  ok("entities/bot-run-usable.json", "BotRun", {
    ...run,
    requestedFor: ID.owner,
    chainId,
    code: { branch: "tc/run-01jbx0", head: "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3", pushed: true },
    phase: "taken_over",
    pausedBy: ID.owner,
    holdKind: "takeover",
    heldAt: T0 + 5000,
    takeover: "commands",
    lastHandback: handback,
  }, "requestedFor, chainId, code, phase and a take-over");
  ok("entities/bot-run-waiting-computer.json", "BotRun", { ...run, status: "created", machineId: undefined, phase: "waiting_computer", waitingFor: "offline", takeover: "none" }, "waits for the requester's own machine");
  ok(
    "entities/bot-run-waiting-limit.json",
    "BotRun",
    { ...run, status: "created", machineId: undefined, phase: "waiting_computer", waitingFor: "limit", queuedAt: run.updatedAt, queuePosition: 1 },
    "waits for its machine's subscription limit to reset (模 3.4)",
  );
  ok("entities/bot-run-unknown-phase.json", "BotRun", { ...run, phase: "dreaming", waitingFor: "the_moon", takeover: "screen" }, "unknown phase values are read as open strings");
  bad("entities/bot-run-bad-code.json", "BotRun", { ...run, code: { branch: "x", head: "abc", pushed: "yes" } }, "code.pushed is a boolean");
  bad("entities/bot-run-handback-no-files.json", "BotRun", { ...run, lastHandback: { by: ID.owner, heldMs: 1, commands: 0 } }, "a hand-back lists files");
  ok("events/run.updated-usable.json", "Event", { ...base, actorId: ID.bot, type: "run.updated", data: { run: { ...run, requestedFor: ID.owner, chainId, phase: "working" } } });

  ok("http/run-command-request.json", "RunCommandRequest", { command: "git status --short && npm test" });
  bad("http/run-command-request-empty.json", "RunCommandRequest", { command: "" }, "a command is 1..4000 characters");
  bad("http/run-command-request-too-long.json", "RunCommandRequest", { command: "x".repeat(4001) }, "a command is 1..4000 characters");
  ok("http/run-command-response.json", "RunCommandResponse", { commandId: "c1" });
  bad("http/run-command-response-bad-id.json", "RunCommandResponse", { commandId: "c 1" }, "command ids are [A-Za-z0-9_-]");

  // -------------------------------------------------------------------------
  // GitHub sign-in
  // -------------------------------------------------------------------------

  const account = { login: "alice-dev", name: "Alice", status: "ok", connectedAt: T0 };
  const wait = { runId, groupId: ID.general, topicId: ID.topic, botId: ID.bot, reason: "not_connected" };
  ok("http/github-status-response.json", "GithubStatusResponse", { available: true, account, waits: [] }, "connected");
  ok("http/github-status-response-waits.json", "GithubStatusResponse", { available: true, waits: [wait] }, "not connected, a run waits for it");
  ok("http/github-status-response-unavailable.json", "GithubStatusResponse", { available: false, unavailableReason: "no_secret_key", waits: [] });
  ok("http/github-status-response-invalid.json", "GithubStatusResponse", { available: true, account: { login: "alice-dev", status: "invalid", connectedAt: T0 }, waits: [{ ...wait, reason: "invalid" }] }, "GitHub refused the token");
  bad("http/github-status-response-no-waits.json", "GithubStatusResponse", { available: true }, "waits is required");
  ok("http/github-device-response.json", "GithubDeviceResponse", { deviceId: "01JBX0DEVICE000000000000BA", userCode: "WDJB-MJHT", verificationUri: "https://github.com/login/device", interval: 5, expiresAt: T0 + 900_000 }, "the user code is a fake placeholder");
  ok("http/github-poll-response-pending.json", "GithubPollResponse", { state: "pending" });
  ok("http/github-poll-response-done.json", "GithubPollResponse", { state: "done", account });
  ok("http/github-poll-response-unknown.json", "GithubPollResponse", { state: "a_future_state" }, "unknown states are read as open strings");
  ok("transient/github.status.json", "TransientEvent", { type: "github.status", account }, "only to the person's connections with feature github/1");
  ok("transient/github.status-disconnected.json", "TransientEvent", { type: "github.status" }, "no account = disconnected");
  ok("http/machine-github-token-response.json", "MachineGithubTokenResponse", { token: "placeholder-not-a-real-token", login: "alice-dev" }, "the token is a fake placeholder");
  ok("http/machine-github-wait-request.json", "MachineGithubWaitRequest", { reason: "not_connected" });
  bad("http/machine-github-wait-request-empty.json", "MachineGithubWaitRequest", {}, "reason is required");

  // -------------------------------------------------------------------------
  // Machine protocol: features github and takeover, run.done.code
  // -------------------------------------------------------------------------

  const runEventFrame = (seq: number, event: Record<string, unknown>) => ({ t: "run.event", runId, seq, at: T0 + seq, event });
  ok("machine/up-hello-runs-in.json", "MachineUpFrame", {
    t: "hello",
    protocol: 1,
    machine: {
      name: "linux-host",
      labels: ["linux"],
      daemonVersion: "1.4.0",
      os: "linux",
      arch: "x86_64",
      maxParallelRuns: 4,
      features: ["pause", "github", "takeover", "takeover-isolated"],
      runsIn: "vm",
    },
    clis: [],
    health: { diskFreeBytes: 1, diskTotalBytes: 2, load1: 0.5, load5: 0.25, load15: 0.125, cpuCount: 8, activeRuns: 0 },
    runs: [],
  }, "a VM host with the usable features");
  ok("machine/up-run.done-code.json", "MachineUpFrame", {
    t: "run.done",
    runId,
    seq: 20,
    at: T0 + 20,
    status: "succeeded",
    durationMs: 64000,
    code: { branch: "tc/run-01jbx0", head: "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3", pushed: false },
  }, "the run's branch is not pushed");
  bad("machine/up-run.done-code-bad-head.json", "MachineUpFrame", {
    t: "run.done",
    runId,
    seq: 20,
    at: T0 + 20,
    status: "succeeded",
    durationMs: 64000,
    code: { branch: "main", pushed: true },
  }, "code.head is required");
  ok("machine/up-run.event-output-run-report.json", "MachineUpFrame", runEventFrame(22, {
    type: "output",
    outputId: "o3",
    op: "run.report",
    body: { state: "blocked", text: "需要 staging 的数据库密码才能接着做" },
  }), "chat-run report: the CLI says how its work ended (done, blocked, waiting)");
  ok("machine/run-event-github-used.json", "RunEvent", { type: "github", state: "used", login: "alice-dev" });
  ok("machine/run-event-github-missing.json", "RunEvent", { type: "github", state: "missing" });
  ok("machine/run-event-hand.command.json", "RunEvent", { type: "hand.command", commandId: "c1", command: "npm test", by: ID.owner });
  ok("machine/run-event-hand.output.json", "RunEvent", { type: "hand.output", commandId: "c1", text: "> app@1.0.0 test\n12 passed\n" });
  ok("machine/run-event-hand.result.json", "RunEvent", { type: "hand.result", commandId: "c1", ok: true, exitCode: 0, ms: 4200 });
  ok("machine/run-event-hand.result-refused.json", "RunEvent", { type: "hand.result", commandId: "c2", ok: false, ms: 0, error: "not_held" }, "the daemon refuses a command of someone who does not hold the run");
  ok("machine/run-event-hand.result-timeout.json", "RunEvent", { type: "hand.result", commandId: "c3", ok: false, exitCode: -1, ms: 600_000, timedOut: true });
  ok("machine/run-event-hand.summary.json", "RunEvent", { type: "hand.summary", ...handback });
  ok("machine/up-run.event-hand.result.json", "MachineUpFrame", runEventFrame(21, { type: "hand.result", commandId: "c1", ok: true, exitCode: 0, ms: 4200 }));
  ok("machine/down-run.assign-github.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "Topic: 修好登录\n\nJoye: @dev 修一下",
    envAllowlist: [],
    leaseMs: 600000,
    github: {
      source: "app",
      memberId: ID.owner,
      login: "joye-dev",
      author: { name: "Joye", email: "1234+joye-dev@users.noreply.github.com" },
      committer: { name: "dev", email: `ai-${ID.bot}@users.noreply.invalid` },
      rewriteSsh: true,
    },
  }, "a team machine: the person's GitHub, author and committer set");
  ok("machine/down-run.assign-github-machine.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "Topic: 修好登录",
    envAllowlist: [],
    leaseMs: 600000,
    github: { source: "machine", memberId: ID.owner, committer: { name: "dev", email: `ai-${ID.bot}@users.noreply.invalid` } },
  }, "the person's own machine: its own sign-in and git config");
  bad("machine/down-run.assign-github-no-committer.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "x",
    envAllowlist: [],
    leaseMs: 600000,
    github: { source: "app", memberId: ID.owner },
  }, "github.committer is required");

  // GitHub by computer (design-github-identity 3.2, 3.3, 3.6)
  const aiCommitter = { name: "dev", email: `ai-${ID.bot}@users.noreply.invalid` };
  ok("machine/down-run.assign-github-asked-by.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "Topic: 修好登录\n\n实习生: @dev 改一下 README",
    envAllowlist: [],
    leaseMs: 600000,
    github: { source: "machine", memberId: ID.alice, author: aiCommitter, committer: aiCommitter, askedBy: "实习生" },
  }, "the computer's own sign-in; author and committer are the AI member; askedBy goes into the PR body");
  ok("machine/run-event-github-missing-reason.json", "RunEvent", { type: "github", state: "missing", reason: "signed_out" }, "the host's gh is not signed in");
  ok("machine/up-heartbeat-github.json", "MachineUpFrame", {
    t: "heartbeat",
    at: T0,
    health: { diskFreeBytes: 1, diskTotalBytes: 2, load1: 0.5, load5: 0.25, load15: 0.125, cpuCount: 8, activeRuns: 0 },
    runs: [],
    metrics: { at: T0 - 100, runs: [], github: { state: "signed_in", login: "example-bot" } },
  }, "feature github-host: the host's own GitHub sign-in");
  ok("entities/machine-github.json", "Machine", {
    ...machineBase,
    github: { state: "signed_out" },
  }, "the computer's GitHub sign-in, to admins and the computer's owner");
  bad("entities/machine-github-no-state.json", "Machine", { ...machineBase, github: { login: "example-bot" } }, "github.state is required");
  ok("http/machine-monitor-github.json", "MachineMonitor", {
    machine: { id: machineId, name: "linux-host", connected: true, lastHeartbeatAt: T0 },
    series: { range: "15m", from: T0 - 900_000, stepMs: 30_000, cpuPct: [], memPct: [], diskPct: [], netRxBps: [], netTxBps: [] },
    capacity: { slots: 4, running: 0, canTake: 4 },
    queue: { count: 0 },
    today: { finished: 0, succeeded: 0, failed: 0, finishedYesterday: 0, tokens: 0 },
    clis: [],
    runs: [],
    members: [],
    github: { state: "no_gh" },
    runsIn: "vm",
  }, "usable: the computer's GitHub sign-in and where its runs execute");
  ok("http/github-status-response-off.json", "GithubStatusResponse", { available: false, unavailableReason: "off", waits: [] }, "switched off: GitHub is the computer's own sign-in");
  ok("machine/down-run.pause-takeover.json", "MachineDownFrame", { t: "run.pause", runId, by: ID.owner, hold: "takeover" });
  ok("machine/down-run.command.json", "MachineDownFrame", { t: "run.command", runId, commandId: "c1", command: "git status", by: ID.owner });
  ok("machine/down-run.command.cancel.json", "MachineDownFrame", { t: "run.command.cancel", runId, commandId: "c1" });
  bad("machine/down-run.command-no-by.json", "MachineDownFrame", { t: "run.command", runId, commandId: "c1", command: "ls" }, "by is required");

  // -------------------------------------------------------------------------
  // Failure kinds, the model a run used and a confirmed sign-out (模 2.2, 2.3, 3.1, 3.2)
  // -------------------------------------------------------------------------

  ok("entities/bot-run-failed-model.json", "BotRun", {"id": "01JBX0RUN000000000000000AU", "botId": "01JBX0BOT000000000000000AC", "groupId": "01JBX0GENERAL000000000000A", "topicId": "01JBX0TOPIC0000000000000AD", "machineId": "01JBX0MACHINE00000000000AV", "status": "failed", "requestedBy": "01JBX0OWNER0000000000000AA", "createdAt": 1762128000000, "startedAt": 1762128001000, "endedAt": 1762128005000, "updatedAt": 1762128005000, "model": "claude-test-1", "effort": "high", "failure": {"kind": "signed_out"}}, "Usable: the model and 思考强度 the run really used, and why it failed");
  ok("entities/run-log-entry-done-failure.json", "RunLogEntry", {"t": "run.done", "runId": "01JBX0RUN000000000000000AU", "seq": 9, "at": 1762128060000, "status": "failed", "error": "You've hit your session limit · resets 3pm", "durationMs": 4100, "failure": {"kind": "limit", "resetsAt": 1762142400000}}, "Usable: a failed run.done with its failure kind and the time the limit resets");
  ok("entities/run-log-entry-done-failure-unknown.json", "RunLogEntry", {"t": "run.done", "runId": "01JBX0RUN000000000000000AU", "seq": 9, "at": 1762128060000, "status": "failed", "durationMs": 10, "failure": {"kind": "quota_paused"}}, "Usable: a failure kind this reader does not know is kept as an open string");
  bad("entities/run-log-entry-done-failure-no-kind.json", "RunLogEntry", {"t": "run.done", "runId": "01JBX0RUN000000000000000AU", "seq": 9, "at": 1762128060000, "status": "failed", "durationMs": 10, "failure": {"resetsAt": 1762142400000}}, "Usable: a failure without a kind");
  ok("entities/run-log-entry-session-effort.json", "RunLogEntry", {"t": "run.event", "runId": "01JBX0RUN000000000000000AU", "seq": 1, "at": 1762128001000, "event": {"type": "session", "sessionId": "s-1", "runtime": "claude", "model": "claude-test-1", "effort": "high"}}, "Usable: the session event names the model and the 思考强度 the CLI runs with");
  ok("machine/up-heartbeat-signed-out.json", "MachineUpFrame", {"t": "heartbeat", "at": 1793627520000, "health": {"diskFreeBytes": 120000000000, "diskTotalBytes": 494000000000, "load1": 0.5, "load5": 0.4, "load15": 0.3, "cpuCount": 10, "activeRuns": 0}, "runs": [], "metrics": {"at": 1793627519900, "runs": [], "logins": [{"runtime": "claude", "state": "logged_out", "confirmed": true}, {"runtime": "codex", "state": "logged_out"}]}}, "usable: a confirmed logged_out (the CLI's own check said so) and one from the credential files only");

  // -------------------------------------------------------------------------
  // Resume fallback (design-vm-state 3.2)
  // -------------------------------------------------------------------------

  ok("machine/down-run.assign-full-prompt.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "Joye: 另一个测试也修一下",
    envAllowlist: [],
    leaseMs: 600000,
    resumeSessionId: "0b6a3c1e-2f4d-4e5a-9b7c-1d2e3f4a5b6c",
    fullPrompt: "Topic: 修好登录\n\nJoye: @dev 修一下\n\nJoye: 另一个测试也修一下",
  }, "a resumed run with the full prompt for when its session is not found");
  ok("machine/up-run.done-resume-missed.json", "MachineUpFrame", {
    t: "run.done",
    runId,
    seq: 20,
    at: T0 + 20,
    status: "succeeded",
    sessionId: "5c1d2e3f-4a5b-4c6d-8e7f-9a0b1c2d3e4f",
    durationMs: 12000,
    resumeMissed: true,
  }, "the session was not found, so the run used the full prompt");
}
