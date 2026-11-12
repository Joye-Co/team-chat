# Machine protocol (version 1)

The contract between the machine daemon (`apps/daemon`) and the server's **MachineLink**
Durable Object (one per machine). The daemon runs on a team machine (a shared Mac, a member's
laptop, later a container), connects out to the server (no inbound ports), receives runs for
bot members, runs a coding CLI for each run in its own folder, and streams normalized progress
back. **BotDesk** (one per bot member) decides what to run and turns run events into topic
messages; it talks to MachineLink, never to the daemon.

The shared types are `packages/protocol/src/machine.ts` (Zod, used by the server) and
`crates/chat-protocol/src/machine.rs` (serde, used by the daemon through
`apps/daemon/src/proto.rs`). `apps/daemon/tests/fixtures/machine/{up,down}.jsonl` hold one
golden frame per line; the daemon test parses each one and serializes it back to an equal
value, and `bun run gen:protocol` copies them into `protocol/fixtures/machine/`, which both
the Zod and the Rust fixture tests check. Change the doc, both type modules and the golden
files together. Conventions are those of [protocol.md](protocol.md):
JSON, UTF-8, camelCase, unknown fields ignored, times in ms since the epoch, IDs are ULIDs
made by the server (except the ids below that the daemon or the CLI makes), one frame at most
1 MiB.

## Machine token

- The owner adds a machine in settings (机器) or with `bun scripts/admin.ts machine --name
  <n> [--labels a,b]` (the token goes to a mode-600 file, never to the terminal):
  `POST /v1/machines`. The server makes a row in D1 (`machines`: id, name, labels, token hash,
  created by and at, revoked at, last seen) and a token `tm_` + 43 characters of base64url
  (32 random bytes), shown once. The server stores only the SHA-256 hex digest, like member
  tokens. `DELETE /v1/machines/:id` revokes it: the link closes with
  `error.code = "unauthorized"` and new upgrades get 401.
- The daemon reads the token from `token_file` (mode 600; the daemon refuses group- or
  world-readable files) and never logs or prints it.

## Connection

`GET {base}/v1/machine/ws` with header `Authorization: Bearer <machine token>`. The Worker
rejects a missing or bad token with HTTP 401 before the upgrade and routes the upgrade to the
MachineLink of that machine (`idFromName(machineId)`, location hint `oc`). One machine has at
most one live link: a new link replaces the old one (MachineLink closes the old socket with
code 4002).

Every frame has a `t` field. The first daemon frame is `hello`; the server answers `welcome`.
Before `welcome` the daemon sends nothing else, and the server sends nothing but `welcome` or
`error`.

## Daemon to server

```ts
{ t: "hello", protocol: 1,
  machine: { id?: string, name: string, labels: string[], daemonVersion: string,
             os: string, arch: string, maxParallelRuns: number,
             features?: string[] /* 1.1, see "1.1 features" */, takeoverUrl?: string /* 1.1 */ },
  clis: CliInfo[], health: Health, runs: RunCursor[] }
  // machine.id is the configured id (optional); the token is authoritative.
  // runs = every run the daemon still holds (running, or finished with frames not yet acked)

{ t: "heartbeat", at, health: Health, runs: RunCursor[],
  metrics?: MachineMetrics }                                     // every 30 s; renews leases; metrics: see "Metrics"
{ t: "run.accept", runId }
{ t: "run.reject", runId, reason: "busy" | "runtime_unavailable" | "disk_low" | "invalid" | "draining",
  message: string }
{ t: "run.event",   runId, seq, at, event: RunEvent }
{ t: "run.done",    runId, seq, at, status: "succeeded" | "failed" | "cancelled" | "timeout",
  exitCode?: number, resultText?: string /* <= 64 KiB */, summary?: string /* first line, <= 280 chars */,
  error?: string /* <= 4 KiB */, sessionId?: string, usage?: Usage, durationMs: number }
{ t: "run.session", runId, seq, at, ok: boolean, source?: "cli" | "stream",
  bytes?: number, gzipBytes?: number, sha256?: string, error?: string }

CliInfo   = { runtime: string, kind: "codex" | "claude" | "acp" | "pty", command: string,
              version?: string, available: boolean }
              // runtime = the name configured on the machine; run.assign.runtime names one
Health    = { diskFreeBytes, diskTotalBytes, load1, load5, load15, cpuCount, activeRuns }
RunCursor = { runId, lastSeq: number /* highest seq produced */, done: boolean,
              active?: boolean /* usable 盯 1.4: the processes the CLI started used CPU since the
                                  last sample (100 ms in 15 s); absent = not known */ }
Usage     = { inputTokens: number, outputTokens: number, costUsd?: number,
              models?: ModelUsage[] /* per model the run used; older daemons omit it */ }
ModelUsage = { model: string /* the id the CLI reported */, inputTokens /* not cached */,
              outputTokens, cacheReadTokens?, cacheWriteTokens?,
              costUsd? /* the CLI's own figure for this model (Claude Code), else none */ }
```

## Server to daemon

```ts
{ t: "welcome", protocol: 1, machineId: string, serverTime: number, runs: KnownRun[] }
  // runs = every run the server has assigned to this machine and not yet closed
  //        (no run.done stored, lease not expired), with the highest seq it stored
KnownRun = { runId, ackedSeq: number }

{ t: "heartbeat.ack", at }                                       // answer to every heartbeat
{ t: "run.assign", runId, botMemberId, groupId, topicId,
  runtime: string,                 // a CliInfo.runtime of this machine
  prompt: string,                  // full prompt, including the topic context BotDesk chose
  model?: string,
  effort?: string,                 // feature "effort", see "Models and effort"
  repo?: { url?: string, path?: string, baseBranch?: string },   // url xor path; none = scratch folder
  envAllowlist: string[],          // names of daemon env vars the CLI may see (see Environment)
  leaseMs: number,                 // e.g. 600000
  maxDurationMs?: number,          // the daemon cancels the run after this; status "timeout"
                                   // (usable: the team limit + 30 min, a backstop; BotDesk saves
                                   // and continues the run at the limit itself)
  sessionUpload?: { url: string, headers?: Record<string, string> },
  kind?: "aside",                  // 1.1, feature "aside": a read-only side chat run
  resumeSessionId?: string,        // 1.1, feature "resume": continue this CLI session (<= 128)
  gates?: RunGate[] }              // feature "gates": kinds the run stops at, see "Gated actions"
{ t: "run.input",    runId, inputId, text, authorId?,            // a follow-up from the topic
  answer?: RunAnswer, artifactEvent?: RunArtifactEvent }         // 1.1, feature "answer"
{ t: "run.approval", runId, approvalId, decision: "approve" | "deny",
  scope?: "run" }                                                // feature "gates": 这件事里都同意
{ t: "run.cancel",   runId, reason? }
{ t: "run.pause",    runId, by? }                                // 1.1, feature "pause"
{ t: "run.resume",   runId, note? /* <= 2000 */ }                // 1.1, feature "pause"
RunAnswer = { messageId, rev /* 1..4294967295 */, actionId, inputs?: Record<string, JSON>,
              editedValues?: Record<string, string>, actorId }
RunArtifactEvent = { artifactId, version, name, payload?: JSON, actorId }
{ t: "ack",          runId, seq }                                // cumulative: every frame <= seq is stored
{ t: "error",        error: { code: ErrorCode, message } }       // connection-level, as in protocol.md
```

## Run events (the item model)

A run is one assignment. Inside it the CLI has one **session** (its own id, e.g. the Codex
thread id or the Claude session id), one or more **turns** (one per prompt or follow-up), and
**items** inside each turn (messages, reasoning, tool calls, file changes). Ids come from the
CLI where it has them; otherwise the daemon makes them (`t1`, `t2` for turns, `m1` for items).
`message.delta` and the final `message` of one item share its `itemId`; a consumer appends
deltas and replaces the text with the final `message`.

```ts
RunEvent =
  | { type: "session",          sessionId, runtime, model? }
  | { type: "turn.started",     turnId }
  | { type: "turn.completed",   turnId, status: "completed" | "failed" | "interrupted", error? }
  | { type: "message.delta",    itemId, turnId?, text }                 // append
  | { type: "message",          itemId, turnId?, role: "assistant" | "user", text }
  | { type: "reasoning",        itemId, turnId?, text }
  | { type: "tool.call",        itemId, turnId?, tool, title, input?: unknown }
  | { type: "tool.result",      itemId, turnId?, ok: boolean, output? /* <= 16 KiB */, exitCode? }
  | { type: "file.change",      itemId, turnId?, changes: { path, kind: "add" | "modify" | "delete", diff? }[] }
  | { type: "approval.request", approvalId, itemId?, title, detail?,    // answer with run.approval
      kind?, command? }                                                 // feature "gates": a stopped command
  | { type: "approval.resolved", approvalId, decision: "approve" | "deny" }
  | { type: "input.accepted",   inputId }                               // the CLI took this run.input
  | { type: "plan",             items: RunPlanItem[] }                  // replaces the run plan (protocol.md); the daemon sets startedAt / endedAt when a step becomes current / done
  | { type: "status",           state: "preparing" | "running" | "retrying" | "stalled" | "error", message? }
  | { type: "acceptance",       items: AcceptanceItem[] }               // 1.1: replaces BotRun.acceptance
  | { type: "paused",           paused: boolean, by? }                  // 1.1: after run.pause / run.resume
```

- `status: "preparing"` comes first (worktree setup), then `running`.
- `status: "stalled"` is sent once after `stall_minutes` (default 5) without any event, or
  after 30 minutes while a `tool.call` has no `tool.result` yet (the CLI waits for a step); the
  next event clears it, and so do the CLI's tools while they use CPU (usable 盯 1.4). BotDesk
  takes it as a signal only.
- `tool.result.itemId` equals the `tool.call.itemId` it answers.
- `plan` carries the CLI's own to-do list, mapped by the daemon: Claude Code's `TodoWrite` tool
  input (`todos[].content`, `status` `completed` -> `done`, `in_progress` -> `current`,
  `pending` -> `pending`; the task tools `TaskCreate` and `TaskUpdate` build the same list one
  task at a time) and Codex's `turn/plan/updated` notification (`plan[].step`, `status`
  `completed` -> `done`, `inProgress` -> `current`, `pending` -> `pending`). The daemon cuts each
  text to 200 characters, keeps the first 30 items, drops empty texts, and sends a plan only
  when it differs from the last one it sent for the run. The tool call itself is still sent as
  `tool.call`.

## Sequence numbers, acks and replay

- Every `run.event`, `run.done` and `run.session` of one run carries `seq`: 1, 2, 3, ... made
  by the daemon, strictly increasing. `run.done` is the last frame except `run.session`.
- The server stores frames idempotently by `(runId, seq)` and sends `ack` with the highest
  seq it stored (at least once per second while frames arrive, or every 100 frames).
- The daemon keeps every unacked frame. **A run keeps going while the link is down**: frames
  are buffered (and also written to `<run>/events.jsonl`). If one run buffers more than
  20,000 frames offline, the daemon drops its buffered `message.delta` frames (the final
  `message` of each item is kept), so a server must accept gaps in `seq` and must never wait
  for a missing seq.
- On reconnect the daemon sends `hello` with `runs` (its cursors). The server answers
  `welcome` with `runs` (its `ackedSeq` per run). For each run in `welcome.runs` the daemon
  resends every buffered frame with `seq > ackedSeq`, in order. A run the daemon holds that is
  **not** in `welcome.runs` is closed on the server: the daemon cancels it if it still runs and
  discards its frames.
- A run is forgotten by the daemon once `run.session` (or `run.done` when there is no upload)
  is produced and acked. A later `run.assign` with the same `runId` is rejected (`invalid`).
- **Daemon restart.** `run.json` records the CLI's process group. A daemon that starts after
  a crash stops the process groups of unfinished runs (when the group leader still runs the
  recorded CLI), then rebuilds those runs from `events.jsonl` and appends `run.done`
  (`failed`, error `daemon restarted`, `sessionId` the last `session` event of the run, or the
  session the run was assigned to resume when it logged none) and a failed `run.session`. Their frames, and those of runs that finished in the last hour, are
  offered in `hello` and replayed after `welcome` as above. BotDesk does not record such a run
  as failed: it continues the work in a new run on the same machine that resumes that CLI
  session, with the recovery note (see "Leases"); only after 3 such resumes in a row, or for a
  run it cannot start again (a side chat run), does the run fail, with a note that says the
  computer's program restarted. On a graceful stop (a computer restart, a service restart,
  Ctrl+C) the daemon stops its runs and ends each one the server did not cancel the same way
  (`failed`, error `daemon restarted`, with its session), so BotDesk continues it when the
  daemon is back; it waits up to 10 s for the server to ack their last frames.
- `run.assign` is idempotent: when the daemon already holds the run it answers `run.accept`
  again. When the server gets neither `run.accept` nor `run.reject` within 30 s, or the link
  drops first, it sends the same `run.assign` again on the next link.

## Server side (MachineLink and BotDesk)

- MachineLink stores, per run: the assign frame, its state (`assigned`, `accepted`, `done`
  waiting for `run.session`, `closed`, `lost`), the highest stored seq and the lease. Frames
  of one link are handled in order. Run frames go to the bot's BotDesk in batches of at most
  100; the `ack` is sent only after BotDesk stored them, so an ack always means "stored".
  When BotDesk cannot store a batch after 3 tries, MachineLink closes the link (code 1011) and
  the daemon replays after the next `welcome`.
- `run.input`, `run.approval` and `run.cancel` for a run are queued in MachineLink while the
  link is down and sent after the next `welcome`, after the resent `run.assign` frames.
- An alarm every 30 s (while anything is open) closes an idle link (90 s), expires leases,
  resends unanswered `run.assign` frames after 30 s, and closes `done` runs whose
  `run.session` did not come within an hour.
- Run events are read leniently: a `run.event` whose `event.type` this server does not know is
  stored as is, never rejected (a rejected frame would be resent forever).
- BotDesk (one per bot member) picks the machine (docs/protocol.md, "Bot runs": labels,
  runtime, health, free disk, load) and stores the run log by `(runId, seq)` idempotently.
  `run.reject` makes it try the next qualifying machine; with none left the run fails with a
  note in the topic.

## Metrics

Since 1.2.7 every heartbeat carries `metrics`, the numbers of the owner's monitor (protocol.md,
"Machine monitor"). Old servers ignore the field; the server reads it apart from the heartbeat, so
metrics it cannot parse are dropped and the heartbeat still renews the leases. Every number is
optional: a platform may not have it.

```ts
MachineMetrics = { at /* sample time, machine clock */, uptimeSecs?, cpuPct? /* 0-100 */, cpuThreads?,
                   memTotalBytes?, memAvailableBytes?, netRxBps?, netTxBps?, vmSlots?,
                   boot?: { avgMs, maxMs, samples }, runs: VmRunMetrics[] /* <= 64 */, logins?: CliLogin[],
                   homes?: HomeUse[] /* <= 500, feature "homes" */,
                   problems?: DaemonProblem[] /* <= 20, oldest first; usable, any daemon */ }
DaemonProblem  = { at /* when logged, machine clock */, text /* 1..500 characters, redacted */ }
VmRunMetrics   = { runId, phase: "booting" | "running" | "paused", startedAt, bootMs?, cpuCores?, memBytes?, scratchBytes? }
CliLogin       = { runtime, state: "logged_in" | "logged_out" | "unknown", method?: "subscription" | "api_key" }
HomeUse        = { memberId, usedBytes /* on the host disk */, sizeBytes? /* absent = no limit */ }
```

- **Host**: CPU busy since the previous sample over all threads (Linux `/proc/stat`, macOS
  `host_statistics`), `MemTotal` and `MemAvailable` (`/proc/meminfo`; macOS free + inactive +
  speculative pages), uptime, and bytes per second received and sent on the host's own interfaces
  (`/proc/net/dev` without `lo`, `tap*`, `fc*`, `veth*`, `docker*`, `virbr*`, `br-*`; Linux only).
- **VMs** (`[vm]`): the executor records each run's VM as it boots (`booting`), answers
  (`running`, with `bootMs`: Firecracker start to a connected guest agent), is paused by 接管
  (`paused`) and goes. `cpuCores` is the Firecracker process's CPU time since the previous sample
  divided by the wall time (1.0 = one thread); `memBytes` its `VmRSS` (the guest pages it
  touched); `scratchBytes` the blocks the sparse scratch disk uses. `vmSlots` is
  `max_parallel_runs`; `boot` covers the last 50 boots. Without `[vm]`, `runs` lists the daemon's
  unfinished runs with `phase: "running"` and their start, nothing else.
- **Homes** (`[vm]` with home images): each AI member's home image (see "MicroVM runs"), its size
  and the blocks it takes on the host disk.
- **Logins**: for `claude` and `codex` runtimes only, checked for presence, never read out:
  Claude Code is `logged_in` (`subscription`) with `claudeAiOauth` tokens in
  `$CLAUDE_CONFIG_DIR/.credentials.json` (on macOS, where the credential is in the Keychain, an
  `oauthAccount` in `.claude.json`), else (`api_key`) when the CLI gets `ANTHROPIC_API_KEY` (the
  runtime's `env`, or `env_passthrough` with the daemon's own value); Codex is `logged_in` with
  `tokens` (`subscription`) or `OPENAI_API_KEY` (`api_key`) in `$CODEX_HOME/auth.json`, or the
  same variable as above. Otherwise `logged_out`.
- **Problems** (usable, 机 5.4): the last 20 warning and error lines the daemon logged since it
  started (the link going down, a server error, a key or home it could not save, a failed self
  update, a run that failed, by status only), kept in memory and redacted with every value the
  daemon knows when the heartbeat is made. Absent while there are none.

## Accounts and keys (feature `accounts`)

The owner signs the machine's CLIs in and gives it API keys from the app (团队管理 › 订阅账号 and
API 密钥), never through a shell on the machine. A daemon that can do this reports feature
`accounts` and its public key; the server sends these frames only to such a daemon.

```ts
hello.machine += { publicKey?: string }   // X25519, base64 of 32 bytes
// server -> daemon
{ t: "account.login", loginId, runtime, op: "start" | "code" | "cancel", code? /* <= 2048 */ }
{ t: "secret.set", name /* [A-Z_][A-Z0-9_]{0,63} */, sealed /* base64 sealed box */, members: string[],
  ask?: true /* usable, feature "ask-keys": runs get it only after a person approves */ }
{ t: "secret.delete", name }
// daemon -> server
{ t: "account.login.status", loginId, runtime, state, url?, userCode?, account?: { email?, plan? }, error? }
MachineMetrics += { secrets?: SecretInfo[], usage?: CliUsage[] }   // CliLogin += { email?, plan? }
SecretInfo = { name, last4, members: string[] /* empty = every run */, updatedAt, ask?: true }
CliUsage   = { runtime, at, windows: { kind, label?, usedPct, resetsAt? }[] }
```

- **Machine key**: made once next to the token file (`machine.key`, mode 600; a group- or
  world-readable file turns the feature off). The app seals a key's value to it with a libsodium
  sealed box (X25519 + XSalsa20-Poly1305, `crypto_box_seal`), so the server only relays
  ciphertext.
- **Keys**: `secrets.json` next to it (mode 600) keeps each value, its members and when it
  changed. A name that a run variable or the base environment uses (`CHAT_*`, `PATH`, `HOME`, …)
  is refused. Runs of a listed member (every run when the list is empty) get the keys as
  environment variables, under the runtime's own fixed `env`; side chat runs get none. Every
  value the daemon has held is masked like the machine token in all it sends. The heartbeat
  reports names and the last 4 characters only (none for values shorter than 12).
- **Keys that ask first** (usable, feature `ask-keys`, 机 4.3): a key saved with `ask: true` is
  never in a run's environment. Runs of its members get it as a gated secret, like a `gated_env`
  variable (see "Environment"): `chat-run secret <NAME>` asks a person to approve and, once
  approved, prints the value for 30 minutes. A daemon without the feature never gets `ask`.
- **Sign-in** (`op: "start"`): the daemon runs the CLI's own login with the runtime's
  environment and `BROWSER=true`, reads its output and reports each step. Claude Code:
  `claude auth login --claudeai` prints a link (`state: "open_url"`, `url`); the person signs in
  in their own browser and pastes the code it shows; `op: "code"` writes it to the CLI's standard
  input (`state: "checking"`). Codex: `codex login --device-auth` prints a link and a one-time code
  (`state: "device_code"`, `url`, `userCode`); the person enters the code there. When the CLI exits
  0 the state is `done` with the account (`claude auth status --json`, `codex login status`), else
  `failed` with the CLI's last line (masked). `op: "cancel"` stops it (`cancelled`); after 15 minutes
  it fails. A new `start` for the same `loginId` replaces the old one. Credentials stay in the CLI's
  own files on the machine (`$CLAUDE_CONFIG_DIR`, `$CODEX_HOME`), which the VMs share.
- **Usage** (every 10 minutes and after a sign-in): Claude Code's windows from `claude -p /usage
  --output-format json --tools "" --strict-mcp-config --setting-sources "" --no-session-persistence`
  (a local command: no model request, no new window), parsed from its `Current session: N% used ·
  resets …` and `Current week (…): N% used` lines (`kind` `session`, `week`, `week_model`; the reset
  stays in `label`). Codex's from `GET https://chatgpt.com/backend-api/wham/usage` with the
  machine's own Codex sign-in (`rate_limit.primary_window` / `secondary_window`: `used_percent`,
  `reset_at`). Only percentages and reset times leave the machine.

## Leases

- `run.assign.leaseMs` is the time the server waits without hearing about a run before it
  declares the run lost. Every `heartbeat` lists the daemon's runs and renews each listed
  run's lease to `now + leaseMs`; any `run.*` frame for a run also renews it.
- When a lease expires while the machine is away (no heartbeat), BotDesk keeps waiting: the
  run stays open with a new lease and stays in `welcome.runs`, so a daemon that comes back
  still holding it replays its frames and goes on. The topic gets one note that the run waits
  for its computer, and one when it is back; a waiting run is not counted as silent. After
  30 minutes away the run moves to another healthy machine that can take it (a new run, a new
  session); with no such machine it waits on. A removed machine is not waited for.
- When a lease expires while the machine is connected (the daemon no longer reports the run),
  the server marks the run lost and leaves it out of later `welcome.runs`, so the daemon stops
  it. BotDesk continues the work in a new run (preferring that machine, where the session
  resumes) or sets the run and the topic to `blocked` with one short note.
  Late frames of a lost or closed run are acked and dropped.
- **A run that starts again continues** (盯 1.2): every restart (a stall, a daemon restart, a
  lost or moved run) is a new run that keeps the work line, resumes the old run's CLI session
  when it lands on the machine that has it (`resumeSessionId`; elsewhere a new session with the
  full topic), and whose prompt ends with a recovery note written by the server: why it starts
  again, the try number, the last plan, the last 20 steps (`tool.call` titles) and every outward
  action of the earlier tries (pushes, pull request and release commands, publishes and deploys
  from `tool.call`; cards, 产出 data, hand-offs and the desk note from `output` events; 产出
  versions and the topic's pull requests from the room), with the instruction to check each one
  before repeating anything. Work that already acted outward is never moved to another machine
  by a stall: after the same-session retry the run turns `blocked` instead.
- **A run that moves to another machine** (after the 30 minute wait, or a stall) is closed on
  the old machine's link: a connected daemon gets `run.cancel` now, and a daemon that is away
  finds the run missing from its next `welcome.runs`, so it stops the old copy first thing and
  discards its frames. Two machines never keep working on the same run.
- The daemon's own `stalled` status is a signal only: the server does not restart a run for
  it. A silent run gets one nudge, then is restarted in the same session on the same machine,
  then on another machine when one can take it, and only then is it marked `blocked`.
- The server's lease is 10 minutes (`leaseMs = 600000`) and `maxDurationMs` is the team's run
  time limit (default 2 hours) plus 30 minutes (docs/protocol.md, "Run time limit").
- The server closes the link when it hears nothing for 90 s. The daemon drops the link when
  it receives nothing for 3 heartbeat intervals and reconnects with exponential backoff
  (0.5 s to 30 s, with jitter).

## Follow-up input, approvals and cancel

- `run.input` goes to the running CLI: Codex gets `turn/steer` while a turn runs (a new
  `turn/start` after it), Claude Code gets one more stream-json user line (a new turn), ACP
  agents get the next `session/prompt`, the PTY gets the text and a newline. The daemon
  answers with `input.accepted`.
- Input that reaches the daemon after the run's last turn has ended and the CLI exits is not
  taken: no `input.accepted` comes, `run.done` follows. BotDesk must then start a new run for
  that input (with the topic context in its prompt).
- `approval.request` comes from `chat-run secret` (see **Environment**) and from gated
  commands (see "Gated actions"; a CLI's own permission request that matches no gate is
  answered by the daemon at once, without one). The
  server answers `run.approval`; the daemon answers the CLI (or keeps the secret's answer, or
  lets the waiting command run or not) and emits `approval.resolved`. Since 1.2.1
  an AI member always has every permission and nothing in `daemon.toml` narrows it: Claude Code
  runs with `--permission-mode bypassPermissions` and `--allowedTools` for the shell, edit, web,
  todo and task tools and `Bash(chat-run:*)` (`-p` has nobody to ask); Codex runs with sandbox
  `danger-full-access` and approval `never` (`untrusted` when the run has gates, see "Gated
  actions"; the daemon then answers every request itself). Only a side chat run (aside) is
  read-only.
- `run.cancel` interrupts the CLI (Codex `turn/interrupt`, ACP `session/cancel`), then sends
  SIGTERM to the CLI's whole process group and to the tool groups below it (see **pause**),
  SIGKILL after 3 s, and ends with
  `run.done status = "cancelled"`. `maxDurationMs` does the same with `status = "timeout"`.

## 1.1 features

A 1.1 daemon lists what it supports in `hello.machine.features`. The server sends a 1.1 frame
or field to a daemon only when it reports the feature; an older daemon logs and drops unknown
server frames and ignores unknown fields, so it must never get work it cannot do safely (an
`aside` run on an old daemon would run with full permissions).

| Feature | What the daemon does |
| --- | --- |
| `pause` | `run.pause`: interrupts the CLI turn where the CLI supports it, else sends SIGSTOP to the run's process group, stops the stall timer and the `maxDurationMs` clock, and emits `paused { paused: true, by }`. `run.resume`: SIGCONT, restarts both clocks (the deadline moves by the paused time), emits `paused { paused: false }` and gives `note` to the CLI as its next input when set. A cancel while paused resumes first. |
| `answer` | `run.input.answer` and `run.input.artifactEvent` reach the CLI as typed, untrusted data (a tool result of the CLI's own ask, or a user line with a fenced JSON block). `text` holds the same content as text for daemons without the feature. |
| `aside` | `run.assign.kind = "aside"`: the run is read-only (Codex sandbox `read-only` with no MCP servers or web tools, Claude Code permission mode `plan` with only the read tools, no repo write) whatever `daemon.toml` or the CLI's own settings say. ACP agents and the PTY have no read-only mode: their `CliInfo.aside` is false and the daemon rejects such a run. |
| `acceptance` | The daemon forwards the CLI's acceptance criteria as the `acceptance` run event. |
| `resume` | `run.assign.resumeSessionId`: the run continues that CLI session (Claude Code `--resume`, Codex `thread/resume`, ACP `session/load`) instead of starting a new one; `CliInfo.resume` says which CLIs can. |
| `effort` | Reports `CliInfo.models` and `efforts` and runs with `run.assign.effort`, see "Models and effort". |
| `context` | Answers the read-only topic context ops of `chat-run` (`topic.history`, `topic.message`, `context.search`, `file.get`, and from this version `topic.state`, `artifact.list`, `artifact.get`, `card.get`, `decisions.list`, `member.list`), see "Topic context". |
| `gates` | Reports `CliInfo.gates`, stops the commands of `run.assign.gates` for a person's approval and honors `run.approval.scope`, see "Gated actions". |

How the 1.1 daemon does each one:

- **resume**: BotDesk keeps each run's session id (`run.done.sessionId`, else the `session`
  run event) and its runtime. A new run in a topic whose last ended run (same kind, same bot)
  has a session goes to that run's machine when the machine can take it, and then carries
  `resumeSessionId` when the machine reports `resume` and the runtime is the same; on another
  machine the run starts a new session as before. The daemon passes only ids of 1-128
  `[A-Za-z0-9_-]`. Claude Code: the session file
  `$CLAUDE_CONFIG_DIR/projects/*/<id>.jsonl` is copied into the project folder of the new
  working directory and the CLI starts with `--resume <id>` instead of `--session-id`; with no
  such file the run starts a new session. Codex: `thread/resume { threadId, ...the thread/start
  params }`; an error answer falls back to `thread/start`. ACP: when the agent's `initialize`
  answer has `agentCapabilities.loadSession`, `session/load { sessionId, cwd, mcpServers: [] }`
  with the new run's folder as `cwd`; the history the agent replays while it loads is not
  reported again, and an error answer falls back to `session/new` with a line telling the agent
  that its earlier conversation is gone. The daemon asks each ACP agent (`initialize` only, at
  most 15 s) when it reports its CLIs and sets `CliInfo.resume` from the answer; the server sends
  `resumeSessionId` only for a CLI with `resume: true` (absent: kinds `claude` and `codex`), so a
  CLI that cannot continue gets the full prompt, as a first run does. With `resumeSessionId` the prompt
  carries only what the session has not seen (protocol.md, "Run context"), and `run.done.sessionId`
  names the session the run used. In the 群聊 a run continues the last session only when its request
  quotes a message of that bot or follows up its run; other requests start a new session.

- **pause**: the daemon stops the CLI's process group (`SIGSTOP`, the group from `run.json`) for
  every runtime kind, also Codex: one mechanism, no CLI-specific interrupt. CLIs start tool
  commands in process groups or sessions of their own (Claude Code's Bash tool, Codex exec), so
  the daemon then finds the CLI's descendants (libproc on macOS, `/proc` on Linux) and stops
  each of their groups, scanning again until a scan finds nothing new. A tool that daemonized
  (its parent exited, so pid 1 or a subreaper is its parent and it is no longer below the CLI) is found by
  `CHAT_RUN_FOLDER=<run folder>` in its environment (`KERN_PROCARGS2` on macOS, `/proc/<pid>/environ` on
  Linux; only processes of the daemon's own user) and gets the same signals, also when the run
  ends or is cancelled after the CLI exited. macOS does not show the environment of its own
  platform binaries (`/bin/sleep`), so such an orphan is not found there. It never signals its own
  group, pid 1, or a process outside the run's tree. Resume and cancel send `SIGCONT` to the
  same groups. The daemon keeps each target's start time (libproc, or the `starttime` field of
  `/proc/<pid>/stat`) and checks it before each later signal, so a pid that now belongs to
  another process gets no signal. A pause that comes
  while the run is still `preparing` stops the CLI as soon as it starts. While paused, no
  `stalled` status is sent and the `maxDurationMs` clock stands still. `run.resume` sends
  `SIGCONT`, moves the deadline by the paused time, emits `paused { paused: false }`, and sends
  `note` (cut to 2000 characters) as the next input with `inputId` `resume-<n>`. A
  `run.cancel` while paused sends `SIGCONT` first (a stopped process cannot handle
  `SIGTERM`), emits `paused { paused: false }`, then cancels as usual. A repeated pause
  signals nothing but emits `paused { paused: true, by }` again, so the server counts every
  pause it sent and the next `paused { paused: false }` clears `pausedBy` (a resume the daemon
  never got leaves the CLI stopped). A resume of a run that is not paused does nothing.
  Checked with the real Claude Code CLI (`apps/daemon/e2e/real-cli-pause.ts`, a 10-minute pause
  of a shell loop in its Bash tool): every process of the run stays stopped, nothing is written,
  the run keeps `pausedBy` and is not marked stalled, and after the resume the loop goes on and
  the run ends `done`. A CLI's own tool timeout counts the paused time: Claude Code's Bash
  tool gave up waiting right after the resume and the loop went on in the background.
- **answer**: the CLI gets `text` (when not empty) followed by a fenced `json` block for
  `answer` and one for `artifactEvent`, each after a line that labels it as untrusted data from a
  member of the group. The fence is longer than any backtick run in the data, so the data cannot
  close it. It is the CLI's next turn in the same session (Claude Code: one more stream-json user
  line; Codex: `turn/steer`, or the next `turn/start`). When the CLI has already ended, the input
  is not taken (see "Follow-up input"), and BotDesk starts a new run.
- **aside**: the run's checkout (worktree or scratch folder) is made read-only (`a-w`, without
  following symlinks) before the CLI starts, so a write fails with `EACCES` whatever the CLI's
  own modes allow; the owner gets write access back when the run ends, and cleanup does that
  first for a run that a crash left read-only. `run.assign.kind = "aside"` is accepted only for runtimes of kind `claude` and
  `codex`; others are rejected with `runtime_unavailable` (ACP agents and the PTY have no
  read-only mode the daemon can force). A `kind` this daemon does not know is rejected with
  `invalid`. Claude Code gets `--permission-mode plan --tools Read,Grep,Glob --disallowedTools
  "Bash Edit Write MultiEdit NotebookEdit WebFetch WebSearch mcp__*" --strict-mcp-config
  --mcp-config '{"mcpServers":{}}' --setting-sources "" --settings <run folder>/.aside-settings.json`:
  no MCP servers and no settings files (the user's hooks could run commands; the project and
  local settings come from the repo). The settings file holds only `env` (string values),
  `apiKeyHelper` and `model` of the user's `settings.json` (`$CLAUDE_CONFIG_DIR`, default
  `~/.claude`), so the run keeps its auth and model; it is mode 0600, never on the command line,
  left out when there is nothing to keep, and removed when the run ends. The tool set keeps a
  user allow rule for a command or edit tool from widening the run. Codex gets
  `approvalPolicy: "never"` and `sandbox: "read-only"` (no writes, no network for commands; on Windows Codex enforces
  it only through its own Windows sandbox, so the daemon adds `-c windows.sandbox="unelevated"`:
  no writes, but commands still reach the network),
  and `-c` overrides that turn off web search, apps, plugins, browser and computer use, and
  each MCP server in `$CODEX_HOME/config.toml` and the `.codex/config.toml` files of the
  working directory and the folders above it (MCP tools run outside the sandbox): each one is
  set to a disabled definition with a placeholder of its own transport
  (`mcp_servers.<name>={command="true",enabled=false}`, or `url=...` for a URL server), which
  is valid also when Codex does not load that file (an untrusted project). A server
  name that `-c` cannot address fails the run. The
  runtime's `args`, `allowed_tools`, `permission_mode`, `sandbox` and `approval_policy` from
  `daemon.toml` are ignored, and the CLI gets none of the `envAllowlist` variables. The run
  folder is made as for any run; the answer is the run's `run.done.resultText`.
- **acceptance**: until the daemon has a tool server, the CLI reports its criteria as a fenced
  block with the info string `acceptance` in an assistant message, holding a JSON array of
  `AcceptanceItem` (or `{ "items": [...] }`). The last block of a message wins. The daemon
  cuts texts to the limits (text 500, evidence text 1000 characters, at most 20 items), reads
  `passed`/`met`/`done` as `pass` and `failed`/`unmet` as `fail`, drops unknown states and
  malformed evidence, and sends the `acceptance` event only when the list changed. The message
  itself is sent unchanged. The server's `run.assign.prompt` for a machine with this feature
  says how to write the block (format, states and limits); side chat prompts do not.

`takeoverUrl` comes from `daemon.toml` (`takeover_url`, for example `vnc://<host>`). It is not
how a person takes over a run: that is feature `takeover` (see "Usable features").
It must be `scheme://host...`, at most 2048 bytes, with no spaces and no password (`daemon
doctor` reports a bad one, and the daemon does not send it). A value that redaction would
change is not sent either.
The owner turns on macOS Screen Sharing by hand; the daemon never changes system settings, and
no credentials pass through chat. The server shows the address to owners and run requesters.

### Interaction loop (features `interact` and `output`)

```ts
// server -> daemon (feature "interact"; an old daemon gets run.input with the text envelope instead)
{ t: "run.answer", runId, inputId, source: { kind: "card", messageId, askId, answerId, rev },
  actionId?, blockId?, event, value?, inputs?, editedValues?, actor: { id, name }, eventId?, block? /* the block at rev */,
  tally? /* counts-only poll: votes per option now */ }
{ t: "run.interaction", runId, inputId,
  source: { kind: "card", messageId, blockId?, actionId?, rev } | { kind: "artifact", artifactId, version, eventId, ask },
  event, value?, actor: { id, name }, eventId?, block? /* card events: the block or action at rev */ }
// daemon -> server (feature "output", `chat-run`): a normal run.event with seq and ack
RunEvent += { type: "output", outputId, op: "card.post" | "card.patch" | "interaction.resolve" | "artifact.data" | "artifact.reply" | "desk.note" | "run.report", body: JSON }
// server -> daemon
{ t: "run.output.result", runId, outputId, ok, result?, error? { code, reason?, message } }
run.input.answer += { answerId?, askId?, eventId?, event?, blockId?, value? }
```

On a private card (a non-public poll, an anonymous questionnaire; protocol.md "Private
answers"), `actor` is `{ id: "anonymous", name: "anonymous" }`, and `run.input.authorId` too.
The voter's id is not in the room's hand-off to the run either. A vote of a poll that shows
counts only carries no `value`, `inputs` or `editedValues`: it has `tally` (votes per option
id now) instead, in the envelope too.

The daemon gives `run.answer` and `run.interaction` to the CLI as one user turn: a
`<teamchat-event v="1" id="{inputId}" kind="card.answer|card.event|artifact.emit|artifact.ask">`
envelope with the JSON, followed by the line that it is data, not an instruction. It answers
`input.accepted {inputId}` as for `run.input`. BotDesk performs each `output` op as the bot member
with the rules of the matching WebSocket frame. The `output` body is that frame without `t`, `rid` and
`groupId` (BotDesk takes the group and topic from the run):

| `op` | `body` | `chat-run` command |
| --- | --- | --- |
| `card.post` | `{ cardV2, body?, causeEventId?, clientMsgId? }` (a `send`) | `chat-run card post --file card.json [--cause ev]` |
| `card.patch` | `{ messageId, baseRev, ops, cause? }` | `chat-run card patch --message M --base-rev N --ops ops.json [--cause ev]` |
| `interaction.resolve` | `{ eventId, result: "ignored" \| "failed", note? }` | `chat-run resolve --event ev --result ignored\|failed [--note …]` |
| `artifact.data` | `{ artifactId, version, data, baseDataRev?, cause? }` | `chat-run artifact data --artifact A --version V --file data.json [--base-data-rev N] [--cause ev]` |
| `artifact.reply` | `{ eventId, ok: true, payload? }` or `{ eventId, ok: false, error: { code: "declined", message } }` | `chat-run artifact reply --event ev --file reply.json` or `--decline "…"` |
| `desk.note` (1.1) | `{ text }`: the bot's desk note in the run's group, with the rules of `PUT /v1/desk-note` (protocol.md "Desk notes"); result `{ seq }` | `chat-run desk note --text "…"` (`""` clears) |
| `member.handoff` | `{ member, goal, doneWhen?, notes? }`: 转交 to another AI member (protocol.md "Delegation"); result `{ member, topicId, runId }` | `chat-run handoff --member <name> --goal "…" [--done-when "a; b"] [--notes "…"]` |
| `member.assign` | the same body: 分出去 into a new topic; result `{ member, topicId, runId }` | `chat-run assign --member <name> --goal "…" [--done-when "a; b"] [--notes "…"]` |
| `run.report` (usable) | `{ state: "done" \| "blocked" \| "waiting", text }` (`RunReport`, text 1-2000 characters): how the work ended, in the CLI's own words; result `{ state }`. See "Run report" below | `chat-run report --state done\|blocked\|waiting --text "…"` |

`chat-run` is the daemon binary started by the name `chat-run` (argv0). The daemon links it as
`<run folder>/bin/chat-run` and puts that folder first on the CLI's `PATH`. Side chat runs
(`aside`) get no `chat-run`. One request is one JSON document `{ op, body }`; the daemon sends the
`output` run event (redacted like every frame) and answers one JSON document `{ ok, result? ,
error? }` when `run.output.result` comes. With no result in 15 s it answers `{ ok: false, error:
{ code: "timeout", reason: "output.unconfirmed" } }` ("unconfirmed") when the server acked the
`output` event (it got it, and the result was lost with the link after it applied the output, or
the server is too old to know `output`), else `{ ok: false, error: { code: "unavailable", reason:
"server.not_connected" } }` ("not connected": the daemon's link to the server was down; the result
is unknown, because a lost ack does not show that the server never got the output, and the server
refuses the output as `output.expired` if it comes later). Both messages say that a retry of the
same command is safe: the daemon puts an idempotency key in the body of `card.post`
(`clientMsgId`), `card.patch` (`clientPatchId`) and `artifact.data` (`clientPushId`) unless the
CLI gave one: `k` + 40 hex of SHA-256 over the run id, the op and the body. The same command in the
same run gets the same key, so the server applies it once and answers a retry with the first
result (`duplicate`); another run never gets the same key. `interaction.resolve` and
`artifact.reply` are keyed by their event id. (Daemons before this rule answered
`server.too_old` for the acked case.) A
daemon that does not answer `chat-run` in 20 s gives `{ code: "timeout", reason:
"daemon.no_answer" }`. The transport depends on the runtime:

- Socket (Claude Code, ACP agents, the PTY): `CHAT_RUN_SOCKET` names a unix socket of the run
  (`<run folder>/chat-run.sock`, or a short path in `$TMPDIR` when that one is too long for a
  socket address), mode 0600, accepting only the daemon's own user. A request and its answer
  are one JSON line each. The socket is removed when the run ends.
- Files (Codex): Codex's sandbox counts a unix socket connect as network, and the daemon does
  not open the network (`sandbox_workspace_write.network_access`) for it; it never adds a
  sandbox or network override to the user's Codex args. `CHAT_RUN_DIR` names
  `<working folder>/.chat-run-<run id>`, a new folder inside the writable workspace (never one
  that is already there, so a folder of the repository is never used), mode 0700, with a
  `.gitignore` of `*`. `chat-run` writes `<id>.req` atomically (a temp file renamed into place, mode 0600);
  the daemon watches the folder, takes (reads and removes) the request and writes `<id>.res`
  the same way; `chat-run` reads and removes it, waiting at most 20 s. A request nobody takes
  is removed and reported as `unavailable`. When the run ends the daemon removes only the
  transport's own files (`.gitignore`, `<id>.req`, `<id>.res` and their temp files), then the
  folder if it is empty. A daemon that crashed cannot do this, so at start (and every 30 minutes
  for runs whose CLI is not running) it does the same for each `runs/<id>/work/.chat-run-<id>`. Codex runs
  in the `read-only` sandbox cannot write the folder and get no note.

A connect or write that the CLI's sandbox refuses gives `{ code: "unavailable", reason:
"sandbox.blocked" }`. `chat-run` prints the result JSON (exit 0) or the error JSON with its
`reason` (exit 1). `chat-run card schema` prints the JSON Schema of a Card v2 document
(`protocol/schema/CardV2.schema.json`, made from the protocol package's Zod with
`bun run gen:protocol` and built into the daemon) without asking the daemon; it holds the shape,
and the server checks the write rules too. The CLI learns about `chat-run` from a short note
that names the commands, the card fields, every block type of that schema and
`chat-run card schema`: Claude Code
`--append-system-prompt`, Codex `thread/start.developerInstructions`, ACP the text before the
first prompt; the PTY gets none.

The envelope JSON holds `messageId`, `rev`, `askId`, `answerId`, `eventId`, `event`, `actionId`,
`blockId`, `value`, `inputs`, `editedValues`, `actor` and `block` (card), or `artifactId`,
`version`, `eventId`, `event`, `value` and `actor` (page). `<` is written as `\u003c`, so the
data cannot close the envelope. References to a part (`blockRefs`) do not change
this protocol: they travel inside `prompt` and `run.input.text`.

Server rules (BotDesk):

- A daemon takes `run.answer` and `run.interaction` when it reports feature `interact` or
  `daemonVersion` 1.1.0 or newer. Another daemon gets `run.input` with `text` = a short summary
  line and the same envelope (and the typed `answer` with feature `answer`).
- Live card events are coalesced per card: the first event goes at once, then only the newest
  event per block in each 3 s window. Page `ask` events are never merged.
- With no active run in the topic, a stored answer starts a run (as before), and a live event
  starts a short follow-up run whose prompt carries the envelope: at most 1 per card at a time
  (later events join it), 6 per hour per card, 30 per hour per member; over the limit the ack's
  `expect.mode` is `none`.
- `output` bodies are the fields of the matching client frame without `t`, `rid` and `groupId`
  (always the run's group): `card.post {card | cardV2, cause?, body?}` (in the run's topic),
  `card.patch {messageId, baseRev, ops, cause?}`, `interaction.resolve {eventId, result, note?}`,
  `artifact.data {artifactId, version, data, baseDataRev?, cause?}`, `artifact.reply {eventId, ok,
  payload? | error?}` or `{eventId, decline: "…"}`. `card.post` takes an optional `clientMsgId`
  (1-64 characters, the daemon's idempotency key) as the `send` key. Without `clientMsgId` /
  `clientPatchId` / `clientPushId` in the body, the key is `run:{runId}:out:{outputId}` (posts) or
  `o:{runId}:{outputId}`, so a later run's `o1` is never a repeat. An unknown op gets `invalid` with reason
  `output.unknown_op`; an aside run gets `forbidden` (`output.read_only`). The server measures an
  `output`'s age on its own clock: the frame's `at` moved by the clock offset of the last
  `heartbeat` (or the arrival time before the first heartbeat), never later than the arrival. An
  `output` older than 12 s when it is applied is not performed; it gets `ok: false` with code
  `conflict` and reason `output.expired`, so an output that `chat-run` reported as timed out (15 s)
  is never done later, and a machine clock behind the server drops nothing.

### Files and 产出 (`chat-run`, daemon over HTTP)

A run delivers pages and files as 产出 of its runner member. These `chat-run` ops are answered by
the daemon itself, not sent as `output` run events (files can be larger than a frame): it reads
the files, redacts text files like run events, and calls the server with the machine token as
the run's member. The server gives the run's group, topic and `runId`, so the done card lists the
产出 as proof. `chat-run` waits up to 5 minutes for them.

| `op` | `body` | `chat-run` command |
| --- | --- | --- |
| `file.upload` | `{ path, name?, mime? }` | `chat-run file upload --file <path> [--name N] [--mime T]` → `{ file: FileMeta }` |
| `artifact.create` | `{ title, dir \| file, entry?, kind?, note?, events?, media?, data? }` | `chat-run artifact create --title T (--dir D \| --file F) [--entry P] [--kind html\|file] [--note …] [--events e.json] [--media m.json] [--data d.json]` → `ArtifactVersionResponse` |
| `artifact.version` | `{ artifactId, dir \| file, entry?, title?, note?, events?, media?, data? }` | `chat-run artifact version --artifact A (--dir D \| --file F) [...]` → `ArtifactVersionResponse` |

- Paths are made absolute by `chat-run` and must be inside the run's working folder after
  symbolic links are resolved; anything else gets `forbidden` with reason
  `file.outside_workspace`. `--dir` sends every regular file below the folder that is not
  hidden (a name starting with `.`), at most 500, with `/` paths relative to it. The entry is
  `--entry`, else `index.html` when the folder has one, else the only file. `kind` defaults to
  `html` for an `.html`/`.htm` entry and `file` otherwise. The type of each file comes from its
  extension.

```ts
PUT  /v1/machine/runs/:id/files      // machine token; body = the file's bytes
     headers: Content-Length, Content-Type (the file's type), X-File-Name (URI-encoded UTF-8), X-File-Sha256
     -> FileMeta (ready, createdBy = the run's member, in the run's group and topic)
POST /v1/machine/runs/:id/artifacts  MachineCreateArtifactRequest -> ArtifactVersionResponse   // version.runId = the run
POST /v1/machine/runs/:id/artifacts/:artifactId/versions  AddArtifactVersionRequest -> ArtifactVersionResponse
MachineCreateArtifactRequest = CreateArtifactRequest without { groupId, topicId, runId }
```

The run must be on the calling machine (`not_found` otherwise), active (`conflict`, reason
`run.ended`) and not a side chat (`forbidden`, `output.read_only`); its member must still be in
the group. A version is added only to a 产出 of the run's group, and its `runId` is always the
run. The member's file limits and quota apply. The audit log has `file.create`, `file.upload`,
`artifact.create` and `artifact.version.add` with the member as the actor and
`machine:{machineId}` as the credential.

### Models and effort (feature `effort`)

```ts
CliInfo += { models?: CliModel[] /* <= 50 */, efforts?: string[] /* <= 10 */ }
CliModel = { id, name?, efforts?: string[], defaultEffort?, isDefault?: boolean }
run.assign += { effort?: string }   // only to a daemon with feature "effort"
```

The daemon reports what each CLI can run, so settings offer a list instead of a text field. It
finds the list when it connects (each `hello`), with a 15 s limit per CLI; a CLI it cannot ask
reports no `models`, and settings then let the owner type a model id.

- **Codex**: a short `codex app-server` session: `initialize`, `initialized`, `model/list`. Each
  model that is not `hidden` becomes a `CliModel` (`id`, `displayName` as `name`,
  `supportedReasoningEfforts[].reasoningEffort` as `efforts`, `defaultReasoningEffort`,
  `isDefault`). `efforts` of the CLI is the union of its models' efforts.
- **Claude Code**: no list command. `models` holds the aliases `--model` takes (`fable`, `opus`,
  `sonnet`, `haiku`), and the `model` of the user's `settings.json` (`$CLAUDE_CONFIG_DIR`, default
  `~/.claude`) as `isDefault` when it is set. `efforts` is the list `claude --help` prints for
  `--effort` (`low, medium, high, xhigh, max` today), or none when the flag is missing.
- **daemon.toml** may set `models = ["id", …]` and `efforts = […]` on a runtime: they replace what
  the daemon found (for ACP agents and the PTY, which report none otherwise).

`run.assign.effort` names one of the CLI's efforts: Claude Code gets `--effort <effort>`, Codex
`turn/start.effort` on every turn. An effort the CLI did not report is not passed (the run goes on
with the CLI's default and the daemon logs it). Ids and efforts are passed as one argument, never
through a shell. The `session` run event's `model` reports the model the CLI really used.

### Gated actions (feature `gates`)

```ts
CliInfo += { gates?: boolean }       // runs of this CLI stop at gated commands
run.assign += { gates?: RunGate[] }  // <= 16; only to a daemon with "gates" whose CLI says gates: true
RunGate = { kind: string /* [A-Za-z][A-Za-z0-9_-]{0,31} */, patterns: string[] /* 1..64, each <= 200 */ }
approval.request += { kind?: string /* <= 32 */, command?: string /* <= 4000, redacted */ }
run.approval += { scope?: "run" }
```

An AI member runs with every permission; only the few kinds the team checked need a person
(protocol.md, "Gated actions"), and only at the moment a command would run.

- **Matching**: each pattern is a regular expression, matched case-insensitively against each
  simple command of the tool's shell line (split at newlines, `;`, `&`, `|`; whitespace runs as
  one space), so a pattern cannot span two commands. Each command is matched as written and as
  its plain words: the program without its folder, quotes and `.exe` / `.cmd` / `.bat` /
  `.com` / `.ps1`, the arguments without quotes (`"C:\Program Files\GitHub CLI\gh.exe" pr merge 5`,
  `gh.exe pr merge 5` and `'gh' pr merge 5` are all `gh pr merge 5`), so a pattern holds for
  every spelling a shell accepts. An MCP tool call is matched as
  `<tool name> <input JSON>`. The first kind (assignment order) with a matching pattern wins. A
  pattern the daemon cannot compile is left out and logged; a kind left with none is dropped.
- **Claude Code** (`CliInfo.gates = true`): the run gets `--settings <run folder>/.gate-settings.json`
  with a PreToolUse hook (matcher `^(Bash|PowerShell|Monitor|mcp__.*)$`: the tools that run a
  shell line, and every MCP tool; anchored: Claude Code reads a matcher with other characters
  than letters, digits and `|` as an unanchored regex) that runs
  `<run folder>/bin/chat-run gate` (timeout 24 h instead of the 600 s default; the run's own
  deadline ends it sooner). On Windows the hook is in the exec form
  (`command: <run folder>/bin/chat-run.exe`, `args: ["gate"]`: started without a shell, because
  Claude Code runs a shell line in Git Bash or, without it, in PowerShell, and no quoted path
  with an argument is a command in both) with `onFailure: "block"`; a Windows daemon reports
  `gates` only for a Claude Code of 2.1.322 or later, the version its tests run against
  (`onFailure` for command hooks came with 2.1.295), and a run whose gates reach a CLI that was
  not reported as gated fails before the CLI starts. Claude Code merges hooks of all settings files, so the user's own
  stay. Hooks can be turned off outside the run. The run's settings say `disableAllHooks: false`,
  which wins over the user's and the project's settings. Managed settings rank above the run's
  (`allowManagedHooksOnly`, or `disableAllHooks` there), so the settings also install a
  SessionStart hook, `chat-run gate-alive` (op `gate.alive`): the CLI runs it when it starts,
  before it takes input, and the daemon writes a gated run's prompt only after it ran. When it
  has not run within 45 s the run fails ("the approval hook did not start") and no command of it
  ever runs. The hook reads the hook input on standard input and sends `gate.check
  { tool, command }` over the run's `chat-run` socket. No match: it exits 0 at once with no
  output. A match: the daemon emits `approval.request { approvalId: "daemon-gate-<n>", title: <the
  kind's 设置 name>, detail: command, kind, command }` and the hook waits. `run.approval approve`
  lets the command run; `deny` (or the run ending) prints the PreToolUse `deny` decision with a
  reason the CLI reads ("a person declined it … do the rest another way"). The daemon emits
  `approval.resolved`. Hooks run before the permission flow, so the deny holds although the run
  uses `bypassPermissions` (Claude Code hooks reference, "PreToolUse decision control"; checked
  with the fake CLI and with the real CLI against a fake model in `tests/integration.rs` and
  `tests/real_cli.rs`, on Linux and Windows CI).
- **One question at a time**: the server keeps one open approval per run, so a second gated
  command waits until the first is answered, then asks (or passes, see scope).
- **scope "run"** (这件事里都同意): with `approve`, later matches of the same kind in this run pass
  without asking. The server leaves the kind out of `gates` of the run's follow-ups and retries.
- **While a command waits** the daemon sends no `stalled` status and the deadline moves by the
  time waited (usable 盯 1.8; also while a `chat-run secret` approval waits);
  `run.cancel` stops the CLI as usual (the hook ends with it).
- **When it cannot ask**: a gated run whose `chat-run` socket or hook settings cannot be set up
  fails before the CLI starts. If the hook cannot reach the daemon during the run (not inside a
  run, the socket gone, the daemon restarting), the command is refused on every system: the
  hook prints the deny decision with a reason that tells the CLI to wait a few seconds and run
  the same command again, and the reason goes to the hook's standard error. On Windows a hook
  that cannot start, times out or ends with an unexpected code also blocks the command
  (`onFailure`).
- **Codex** (`CliInfo.gates = true` on every system): a run with gates starts its thread with
  approval policy `untrusted` (sandbox still `danger-full-access`), so the app-server asks for
  every command that is not a known read-only one and for every file change. The daemon answers
  each request itself, with the same matching: `item/commandExecution/requestApproval` is
  matched as the script inside Codex's shell wrapper (`/bin/zsh -lc '<script>'`, `bash -c`,
  `powershell -Command`, unquoted as a shell reads it); an MCP tool approval
  (`mcpServer/elicitation/request` with `_meta.codex_approval_kind = "mcp_tool_call"`) as
  `mcp__<server>__<tool> <tool_params JSON>`. No match: `accept` at once, no card. A match: the
  same `approval.request` (`daemon-gate-<n>`) and wait as for Claude Code, then `accept` or
  `decline`. File changes (`item/fileChange/requestApproval`) and requested permissions
  (`item/permissions/requestApproval`) are granted at once. Checked with the real `codex
  app-server` (0.159): the requests come with no delay a person would notice, an accepted
  command and a file change run as before, a declined one does not run and the turn goes on.
  A run without gates keeps approval `never`.
- **ACP** (`CliInfo.gates = true` only when its `[runtimes.<name>]` sets `asks_permission =
  true`): each `session/request_permission` is matched as the tool call's `rawInput.command` (a
  string or a list of words), else `<title> <rawInput JSON>`. No match (or a run without
  gates): the first `allow_*` option at once, no card. A match: the same `approval.request` and
  wait, then the first `allow_*` or `reject_*` option. An agent that never asks (OpenCode by
  default, any agent whose own permissions allow everything) would not be stopped, so without
  `asks_permission` the daemon does not report `gates` for it and the server keeps the plain
  "Team rule" paragraph in its prompt.
- **Shims** (every gated run; see below for the shells that are not covered): the runtime's own gate sees only the line as
  the CLI wrote it, so the daemon also puts shims first on the run's `PATH`: links to its own
  binary in `<run folder>/bin` (`.exe` on Windows; in a VM the tools disk's `/opt/runner/bin`)
  named `git`, `gh`, `wrangler`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, `bunx`, `cargo`,
  `vercel`, `netlify`, `fly`, `flyctl`, `twine`, `psql`, `mysql`, `sqlite3` (on the host only
  those the run's `PATH` has), and sets `CHAT_RUN_GATES=1`. Started by such a name, the binary
  sends `gate.check { tool: "shim", via: "shim", command }` over the run's `chat-run` transport
  (socket or request folder) and then runs the real program found after the tools folder on
  `PATH` (on Unix in its place), or refuses: exit code 1, the reason on standard error. When
  the daemon cannot be asked it refuses too, like the hook. `command` is decided on the parsed
  program and arguments, not on a shell line: the invocation as plain words (each argument
  with newlines, `;`, `&` and `|` made spaces, so one invocation is one command), then one line
  per fact: for git the command with its options before the subcommand dropped and aliases put
  in (`git push …`), and for a push one line `git push <remote> <source>:refs/heads/<branch>`
  per remote branch it would update (a plain `git push` from the checked-out branch, the
  remote, `remote.<name>.push` and `push.default`; `HEAD`, `@`, `--all`, `--mirror`,
  `--delete`, `:branch`; the shell already put in variables), and for `npm`/`pnpm`/`yarn`/`bun`
  running a package script the script's lines (`pre`, the script, `post`) from the
  `package.json` it runs in (its programs come from `node_modules/.bin`, where no shim is). The
  same patterns decide. So a command inside a script, `git -C . push origin main`, a plain
  `git push` while `main` is checked out and `B=main; git push origin HEAD:$B` all stop.
  Claude Code runs Bash with the `PATH` of the user's shell profile, so a gated Claude Code run
  on Unix also gets `CLAUDE_ENV_FILE=<run folder>/.claude-env.sh`, which puts the shims first
  again before each command. Codex (and an ACP agent) runs each command in the user's login
  shell (`zsh -lc`); on macOS `/etc/zprofile` runs `path_helper`, which puts the system folders
  before the inherited `PATH`. So a gated Codex or ACP run on the host (Unix) gets
  `ZDOTDIR=<run folder>/zsh`, whose `.zshenv`, `.zprofile`, `.zshrc` and `.zlogin` each read the
  user's own file of that name (from `CHAT_RUN_USER_ZDOTDIR`, the user's own `ZDOTDIR`, else
  `$HOME`) and then put the shims first again.
- **Waiting at a shim**: a shim waits inside the CLI's own command, which the CLI kills after
  its command timeout (Claude Code's Bash tool: two minutes by default). So a shim's check waits
  at most 90 seconds (the shim's call 100). With no answer by then the command does not run: the
  answer is `deny` with `waiting: true` and a reason that tells the model to go on and run the
  same command again later. The card stays open, and the one-question turn is free again. A
  retry of the same command (same kind and `command`) before the answer waits for that card
  again; an answer that comes when no shim waits is kept and taken by the next retry of that
  command, once (approve: it runs, no second card; decline: it is refused). Another command's
  card waits until the open one is answered (one open approval per run).
- **One decision per action**: a check without `via` is the runtime's own (the hook, a Codex or
  ACP request, matched or not) and starts the runtime's next command. A kind a person approves
  there holds for the shims of that command: they pass it without a second card, however often
  (a script that pushes twice). The runtime's next command ends that. Another kind in the same
  command asks at its shim; what a shim asks and a person approves holds the same way.
- **Not a security boundary**: this guards against mistakes. A program started by its full
  path, a login shell other than zsh whose system profile resets `PATH` (Codex with `bash` as
  the user's shell, a login shell inside the VM image, Git Bash on Windows with `/mingw64/bin`
  ahead of the shims), or a merge through a web page is not seen. The floor is the repository's
  branch protection and the production environment's own required approval.
- Side chat runs and the PTY runtime are never gated (the server sends no gates to a CLI that
  does not report `gates`).

### Topic context (`chat-run`, daemon over HTTP; feature `context`)

The prompt carries a budget of the topic (see protocol.md, "Run context"), not all of it. A run
reads the rest with these `chat-run` ops. Like files, the daemon answers them itself over HTTP
with the machine token (results can be larger than a frame); they only read.

| `op` | `body` | `chat-run` command |
| --- | --- | --- |
| `topic.history` | `{ before?, after?, limit?, topicId? }` | `chat-run topic history [--before SEQ] [--after SEQ] [--limit N] [--topic T]` → `{ messages: ContextMessage[], hasMore }` |
| `topic.message` | `{ messageId, topicId? }` | `chat-run topic message --id M [--topic T]` → `{ message: ContextMessage }` |
| `context.search` | `{ query, scope?, limit? }` | `chat-run search --query Q [--group] [--limit N]` → `{ hits: ContextHit[] }` |
| `file.get` | `{ fileId, out? }` | `chat-run file get --id F [--out PATH]` → `{ path, name, mime, size }` |
| `topic.state` | `{ topicId? }` | `chat-run topic state [--topic T]` → `TopicStateResponse` |
| `artifact.list` | `{ scope?, topicId? }` | `chat-run artifact list [--group \| --topic T]` → `RunArtifactsResponse` |
| `artifact.get` | `{ artifactId, version?, out? }` | `chat-run artifact get --id A [--version N] [--out PATH]` → `RunArtifactResponse` (with `path` when `out` is set) |
| `card.get` | `{ messageId }` | `chat-run card get --message M` → `RunCardResponse` |
| `decisions.list` | `{ limit? }` | `chat-run decisions list [--limit N]` → `RunDecisionsResponse` |
| `member.list` | `{}` | `chat-run member list` → `RunMembersResponse` |

```ts
GET /v1/machine/runs/:id/topic/messages?before=&after=&limit=&topic=   -> { messages: ContextMessage[], hasMore: boolean }
GET /v1/machine/runs/:id/topic/messages/:messageId?topic=               -> { message: ContextMessage }
GET /v1/machine/runs/:id/topic/state?topic=                     -> TopicStateResponse
GET /v1/machine/runs/:id/artifacts?scope=topic|group&topic=     -> RunArtifactsResponse
GET /v1/machine/runs/:id/artifacts/:artifactId?version=         -> RunArtifactResponse
GET /v1/machine/runs/:id/cards/:messageId                       -> RunCardResponse
GET /v1/machine/runs/:id/decisions?limit=                       -> RunDecisionsResponse
GET /v1/machine/runs/:id/members                                -> RunMembersResponse
TopicStateResponse = { topicId, title, text /* <= 2000 characters */ }
RunArtifactsResponse = { artifacts: Artifact[] }
RunArtifactResponse = { artifact: Artifact, versions: ArtifactVersion[], version, text?, truncated? }
RunCardResponse = { topicId, message: ContextMessage, card /* CardV2 or the 1.0 card */,
                    ask?: { status, from: string[] /* names */ },
                    answers: { actorId, actorName, at?, value?, inputs? }[], answerCount? }
RunDecisionsResponse = { decisions: (GroupDecision & { answeredByName })[], hasMore,
                         recorded?: (RecordedDecision & { recordedByName })[] }
RunMembersResponse = { members: { id, name, role?, ai: boolean }[] }
GET /v1/machine/runs/:id/search?q=&scope=topic|group&limit=     -> { hits: ContextHit[] }
GET /v1/machine/runs/:id/files/:fileId                          -> the file's bytes
    headers: Content-Type, Content-Length, X-File-Name (URI-encoded UTF-8)
ContextMessage = { id, seq, authorId, authorName, at /* ms */, text }
ContextHit = { messageId, topicId, topicTitle?, seq, authorName, at, snippet }
```

- The run must be on the calling machine and active. These ops only read, so a side chat run
  (单独问) may call them too (its group is the side chat's). Everything is of the run's own group:
  `topicId` (`--topic`) names another topic of that group, and a topic, 产出, card or file of
  another group is `not_found`, like an unknown one.
- `topic.history` reads the run's topic (or `topicId`), oldest first. `before` and `after` are message seqs
  (`ContextMessage.seq`, also printed in the prompt) and exclusive. Without either it returns the
  newest messages. `limit` is 1-100, default 30. `hasMore` says that more messages lie past the page
  in the direction of the read (older without `after`, newer with it). Deleted messages are left out.
- `text` is the message as one compact line (protocol.md, "Run context"): the body, then a line for
  each attachment, 产出 reference, quote and card. In `topic.history` the body is cut to 4000
  characters; `topic.message` gives the whole body. A message of another topic is `not_found`.
- `context.search` searches the run's topic, or with `scope: "group"` (`--group`) every topic of the
  run's group the bot member can read. `limit` 1-50, default 20, newest first.
- `file.get` gives a file of the run's group (ready, not deleted). The daemon writes it to `out`,
  which must be inside the run's working folder after symbolic links are resolved (`forbidden`,
  `file.outside_workspace` otherwise); without `out`, to `<run folder>/downloads/<fileId>/<name>`,
  outside the working folder, so a download never ends up in the member's branch when the
  daemon commits what a run left uncommitted. The result says where: `{ path, name, mime, size,
  inWorkFolder }` (`inWorkFolder: false` for the default). Daemons before this wrote the default
  to `<working folder>/.chat-files/<fileId>/<name>` (with a `.gitignore` of `*`). It refuses files
  over 100 MiB with `too_large`. `chat-run` waits up to 5 minutes for it.
- `topic.state` is one compact state card of a topic, at most 2000 characters: who did what (the
  newest authors with their last message), the 产出 with their latest versions, open cards and who
  they wait on, the latest decisions. Members work together through 产出 and state; this is how
  one reads where the others are.
- `artifact.list` gives the 产出 of the run's topic (or `topicId`), or with `scope: "group"`
  (`--group`) of the whole group, newest first (at most 100). `artifact.get` gives one 产出 with
  all its versions; `version` (default the current one) is the one read, and `text` is its entry
  file when that is text (`text/*`, JSON, XML, YAML, SVG; up to 200,000 characters, else
  `truncated`). Other files are read by id with `file.get`. With `out` the daemon also writes the
  entry file there, under the same rules as `file.get`.
- `card.get` gives a card message of the group whole: the card document, the open ask with the
  names it waits on, and the live answers (a private card shows only `answerCount`, as to members).
- `decisions.list` gives the group's decisions (群里的决定), newest first, `limit` 1-100, default 20,
  with the decisions recorded by hand. `member.list` gives the group's active members, people
  first: name, 岗位 (`role`) and `ai` for AI members.
- A daemon with this feature reports `context`; the server's prompt then names these commands for
  messages it left out. The `chat-run` note lists them.

### Run report (`chat-run report`, usable)

Before it ends, the CLI says how its work ended with `chat-run report --state <state> --text
<text>` (output op `run.report`): `done` (the request is done), `blocked` (it cannot go on without
something: a key, an access, a decision) or `waiting` (it asked a person and waits for the
answer). A run may report more than once; the latest report counts when the run ends. The server
answers `{ state }`; a body that is not a `RunReport` is `invalid`.

When the run ends with `run.done` `succeeded`:

| Latest report | Topic | The result in the topic | `BotRun` |
| --- | --- | --- | --- |
| none, or `done` | `done` | the done card (or a text answer), as before | `status: "done"`, `phase: "done"` |
| `blocked` | `blocked`, `waitingOn` the requester | the CLI's last message (else the report text) as a text message that mentions the requester; no done card | `status: "done"`, `phase: "needs_reply"`, `waitingFor: "stuck"` |
| `waiting` | `waiting_answer`, `waitingOn` the requester | the same | `status: "done"`, `phase: "needs_reply"`, `waitingFor: "report"` |

A `needs_reply` run reads `phase: "done"` again once a later run of the AI member starts in the
topic (the server sends `run.updated` for it then). A run that fails or is stopped ignores its
report. Without a report a run that ended normally is done, as before. The 盯活 continuation of a
run that only promised work is skipped when the run reported `blocked` or `waiting`.

When the CLI posted its own Card v2 with intent `done` (`card.post`) in the run, that card is the
run's result: the server does not post the CLI's last message again, and `resultMessageId` names
the card.

### Memory notes (`chat-run`, daemon over HTTP; feature `context`)

A run can keep something for later (docs/protocol.md, "Memory"). The daemon answers `chat-run
memory note` itself over HTTP with the machine token, like the context ops; it checks scope, kind
and lengths before it calls the server.

| `op` | `body` | `chat-run` command |
| --- | --- | --- |
| `memory.note` | `MachineMemoryNoteRequest` | `chat-run memory note --scope member\|person\|group --kind K --text T [--when W] [--source <messageId>]` → `MemoryItem` |
| `memory.search` | `{ query, limit? }` | `chat-run memory search --query <text> [--limit <1-50>]` → `MemoriesResponse` |

```ts
POST /v1/machine/runs/:id/memories   body MachineMemoryNoteRequest -> MemoryItem (status "pending")
MachineMemoryNoteRequest = { scope: "member" | "person" | "group", kind: MemoryKind, text /* 1..500 */, when? /* <= 300 */, sourceMessageId? }
GET  /v1/machine/runs/:id/memories?q=<text>&limit=<1-50, default 10>   -> MemoriesResponse
```

- **Search** (`memory.search`): the prompt carries only part of what is remembered (8 KB, chosen
  for the request). The search reads exactly what the run's prompt could carry: the team's, the
  run's group's, the member's (its own pending notes too) and the requester's active memories,
  each memory learned in a group only when everyone in the run's group is in that group. Hits
  match the query's words or Chinese two-character words, best first; each hit counts one use
  (protocol.md, "Memory"). A side chat run may search. An empty `q` is `invalid`.
- A run that names a memory's id (as `[id]`) in its result or progress counts one use of it, when
  it may read it.

- A note (`memory.note`): the run must be on the calling machine, active and not a side chat, as for files.
- `member` is the run's AI member (its lessons and playbooks), read only in the run's group
  (`fromGroup`), `person` the person who asked for the run (their preferences), `group` the run's
  group (conventions people said in the group; a direct conversation has none: `forbidden`). A
  `group` note cites the person's message it comes from (`--source`, `sourceMessageId`); without
  one, or when that message is not a person's, it goes to the member's own memory of this group.
  Teams' memory is never written by a run.
- The memory is `pending` with source `{ kind: "run", ids: [runId] }` until the daily
  consolidation. The member's own pending lessons are already in its next runs' prompts.
- Text is redacted before it is stored. At most 10 notes per run (`rate_limited` after that).

## Usable features

Additive, like the 1.1 features: the server sends a usable frame or field only to a daemon that
reports its feature, and every new value is an open string (an unknown one is treated as the
safe default named below). Protocol side: protocol.md, "Usable extensions".

| Feature | What the daemon does |
| --- | --- |
| `github` | Reports `hello.machine.runsIn`. Takes `run.assign.github`: sets the commit identity and, with `source: "app"`, lets git and `gh` ask it for the person's token for each use; emits the `github` run event. |
| `takeover` | Takes `run.pause.hold = "takeover"`, `run.command` and `run.command.cancel`; emits the `hand.*` run events. |
| `takeover-isolated` | With `takeover` (VM runs, `runsIn: "vm"`): a person's commands run as another user than the run, so they cannot read the run's own credentials. |
| `homes` | VM hosts (`[vm]`): reports `metrics.homes` (each AI member's home folder) and takes `home.clear`. |
| `ask-keys` | With `accounts`: takes `secret.set.ask` and keeps such a key out of runs' environments until a person approves (see "Accounts and keys"). |

```ts
// daemon -> server
hello.machine += { runsIn?: "host" | "vm" }       // where runs execute; required with feature "github"
CliInfo += { resume?: boolean,                    // with feature "resume": runs of this CLI continue resumeSessionId
             aside?: boolean }                    // with feature "aside": the CLI has a read-only mode for side chats
                                                  // absent (an older daemon): true for kinds "claude" and "codex" only
run.done += { code?: { branch, head /* hex */, pushed: boolean } }  // any daemon; absent = not known
run.done += { failure?: RunFailure }              // any daemon; with status "failed"; absent = not sorted
RunFailure = { kind: "signed_out" | "limit" | "model" | "network" | "not_installed" | "other",  // open: unknown = "other"
               resetsAt?: number /* limit: ms since the epoch, when the CLI said */ }
RunEvent.session += { effort?: string }            // the 思考强度 the CLI runs with; absent = its default or not known
RunEvent +=
  | { type: "github", state: "used" | "missing" | "invalid", login? }        // feature "github"
  | { type: "hand.command", commandId, command /* 1..4000 */, by }           // feature "takeover"
  | { type: "hand.output",  commandId, text /* <= 16 KiB per event */ }
  | { type: "hand.result",  commandId, ok, exitCode?, ms, timedOut?,
      error?: "not_held" | "busy" | "unsupported" }
  | { type: "hand.summary", by, heldMs, commands, files /* <= 20 */, headFrom?, headTo? }

// server -> daemon
run.assign += { github?: RunGithub }              // feature "github"
RunGithub = {
  source: "app" | "machine",     // "app": git and gh get the person's token from the daemon; "machine": the machine's own sign-in; unknown = "machine"
  memberId,                      // the person whose GitHub the run uses
  login?,                        // with "app" when connected
  author?: { name, email },      // absent = the machine's own git config
  committer: { name, email },    // the AI member
  rewriteSsh?: boolean           // rewrite git@github.com: and ssh://git@github.com/ to HTTPS, drop SSH_AUTH_SOCK
}
{ t: "run.pause", runId, by?, hold?: "takeover" }                  // feature "takeover"; absent hold = a pause
{ t: "run.command", runId, commandId, command /* 1..4000 */, by }  // feature "takeover"; only while connected
{ t: "run.command.cancel", runId, commandId }                      // feature "takeover"
{ t: "home.clear", memberId }   // feature "homes"; only while connected: empty that AI member's home
                                 // folder (refused, and logged, while a run of it is on the machine)
```

GitHub (feature `github`), over HTTP with the machine token:

```ts
GET  /v1/machine/runs/:id/github        -> MachineGithubTokenResponse { token, login }
POST /v1/machine/runs/:id/github/wait   MachineGithubWaitRequest { reason: "not_connected" | "invalid" } -> { ok: true }
POST /v1/machine/runs/:id/github/rejected                                                               -> { ok: true }
```

- The token is given only for a run on the calling machine that has not ended, and only when the
  machine may use that person's GitHub; it is fetched for each git or `gh` use and never cached,
  written to a file, logged or put in the CLI's environment. Errors: `not_found` with reason
  `github.not_connected`, `conflict` with `github.invalid` (GitHub refused the token), `forbidden`
  with `github.not_allowed` (another machine, an ended run, or no right to it).
- `/wait` reports that the run needs a GitHub the person has not connected; the server asks the
  person once, and the run then ends waiting (its CLI stops and reports it). `/rejected` reports
  that git got an authentication failure, so the server checks the token once.
- The `chat-run` ops `git-credential` and `github.token` are answered by the daemon itself, like
  `secret`; they are never `output` run events.
- What the daemon does with `run.assign.github`: `GIT_COMMITTER_*` is the AI member and
  `GIT_AUTHOR_*` the person when `author` is given. With `source: "app"` it clears github.com's
  credential helpers and sets `!chat-run git-credential` instead (`GIT_CONFIG_COUNT/KEY/VALUE`),
  puts a `gh` proxy first on `PATH` (`<run folder>/bin/gh`; in a VM `/opt/runner/bin/gh` on the
  tools disk) that runs the real `gh` with `GH_TOKEN` for that one command, and sets
  `CHAT_GITHUB_AS` to the person's name (`gh auth login|logout|refresh|setup-git` are refused).
  Its own clone and fetch of the mirror use the token as an HTTP header of those git processes
  only. Every token it gets is masked in that run's frames, files, session upload and
  `stderr.log`. Before a VM run it removes an earlier GitHub sign-in from the member's kept home
  (`.config/gh/hosts.yml`, `.git-credentials`, github.com credential and token `url.*` sections
  of `.gitconfig`). Any other `source` gets only the committer.

Take-over (feature `takeover`):

- `run.pause` with `hold: "takeover"` stops the CLI as a pause does, or upgrades a pause, and the
  daemon records the work folder (HEAD and `git status --short`) for the hand-back. `run.command`
  is taken only while the run is held by a take-over of the same `by`; else `hand.result` with
  `ok: false` and `error: "not_held"` (`"busy"` while another command runs). A command runs in
  the work folder, one at a time, at most 10 minutes, with no `chat-run` and none of the run's
  secrets. `run.resume` ends a running command, sends `hand.summary` and gives the CLI a note on
  what the person did before `note`.
- `hand.*` events are not the AI member's progress: they reset no stall timer and count as no
  tool call. The server never queues `run.command` or `run.command.cancel` for a daemon that is
  away.

Other usable changes:

- `run.done.code` comes from the run's worktree: its branch, its head commit and whether every
  commit is on the remote. A daemon that cannot tell leaves it out.
- `chat-run artifact version --parent <n>` sends `parentVersion` in the body of `artifact.version`
  (`AddArtifactVersionRequest`).
- `member.handoff` and `member.assign` keep their bodies and results; their errors may carry the
  new reasons `delegation.cycle`, `delegation.stopped` and `delegation.not_for_requester`
  (protocol.md, "Participants, chains and relays"). An older `chat-run` prints them as they come.
- Machine ownership: a machine is the team's or one person's (protocol.md, "Roles: 所有者 and
  personal machines"). The daemon does not know which; the server only assigns a run to a
  personal machine when the run works for its owner.

### Failure kinds and the sign-in state

A failed run says why in `run.done.failure`, so the topic gets one plain sentence and a next
step while the CLI's own words stay in `run.done.error` for the process view. The daemon sorts
by what the CLI reports, its typed fields first and its words only when it has none:

| Kind | Claude Code (`stream-json`) | Codex (`app-server`) |
| --- | --- | --- |
| `signed_out` | assistant `error: "authentication_failed"` (or `oauth_org_not_allowed`), `api_error` `stored_login_refused` / `gateway_signin_required` / `gateway_session_expired`; words: "Not logged in", "Please run /login", "Invalid API key", "token has expired" | `codexErrorInfo: "unauthorized"`; words: "401 Unauthorized", "not logged in" |
| `limit` | assistant `error: "billing_error"`, or `error: "rate_limit"` with `api_error: "usage_limit_reached"` or a `rate_limit_event` with `status: "rejected"`; `resetsAt` from that event's `rate_limit_info.resetsAt` (seconds) or the old `…limit reached\|<epoch>` text; words: "You've hit your … limit", "usage limit reached", "Credit balance is too low" | `codexErrorInfo: "usageLimitExceeded"`; `resetsAt` from the latest `account/rateLimits/updated` window at 100 % |
| `model` | assistant `error: "model_not_found"`, `api_error` `model_requires_usage_credits` / `no_allowed_fallback`; words: "issue with the selected model", `not_found_error` with "model" | `badRequest` or words with `model_not_found`, "model … does not exist", "model is not supported" |
| `network` | `api_error` `connection_failed` / `stream_idle_timeout` / `no_response` / `network_edge_blocked` / `tls_untrusted_ca`, assistant `error` `overloaded` / `server_error`; words: "Connection error", "ECONNREFUSED", "ENOTFOUND", "fetch failed", "timed out" | `httpConnectionFailed`, `responseStreamConnectionFailed`, `responseStreamDisconnected`, `responseTooManyFailedAttempts`, `serverOverloaded`, `internalServerError` |
| `not_installed` | the program was not found when the daemon started it | same |
| `other` | anything else | same |

Claude Code's words in the stderr of a run that exited early say `signed_out` only when they are
the CLI's own ("Not logged in", "Please run /login", "Invalid API key"): that tail can hold an MCP
server's or a hook's "401 Unauthorized", which reads as `other`.

A run that failed with `limit` holds its runtime back on that machine (the sign-in, and so the
limit, is the machine's, shared by every AI member there): the server gives it no runs until
`resetsAt` (plus a minute); without one, until the end of the `metrics.usage` windows of that
runtime at 95 % or more; without those, for 15 minutes, doubling each time in a row up to 2
hours. A run of it that succeeds clears the hold. Nothing changes on the daemon: the work that
hit the limit comes back as a new `run.assign` with `resumeSessionId` once the hold is over
(protocol.md, "Run phase and take-over").

A run that failed with `signed_out` marks its runtime signed out at once: `metrics.logins` says
`logged_out` for it until the CLI's own status check says it is signed in again (a sign-in from
the app, or the check every 10 minutes) or a later run of it succeeds.

`CliLogin += { confirmed?: boolean }` (usable): `true` when a `logged_out` is certain: the CLI's
own status check said so, or a run just failed with `signed_out`. A `logged_out` from the
credential files alone (they do not know every way a CLI signs in, such as Claude Code's
`apiKeyHelper` or a token in its env) or from an older daemon has no `confirmed`; it is shown to
people but keeps no runs away.

The sign-in state in `metrics.logins` is the CLI's own status check when the daemon has one
(feature `accounts`, every 10 minutes and after each sign-in): `claude auth status --json`
(`loggedIn`) and `codex login status` ("Logged in" / "Not logged in"), run with the env a run
gets (the runtime's env and the daemon's `env_passthrough` variables), so it sees an
`apiKeyHelper` or a token passed that way. A check that cannot run or says nothing clear falls
back to the credential files. An API key the CLI gets (its runtime's env, the daemon's env or a
key saved from the app) counts as signed in. The server gives no run to a runtime whose login is
a confirmed `logged_out` (protocol.md, "Failure kinds and the model a run used").

The `session` event's `effort` is the 思考强度 the daemon passed to the CLI (`run.assign.effort`
after the check in "Models and effort"), or the one Codex's `thread/start` answered with.

## Raw session upload

After `run.done` the daemon uploads the CLI's own raw session file, so the process view and
later audits have the complete record:

- Source (`run.session.source`): `cli` is the CLI's file, copied into the run folder first:
  Codex `thread.path` from `thread/start` (under `$CODEX_HOME/sessions`, default
  `~/.codex/sessions`), Claude Code
  `$CLAUDE_CONFIG_DIR/projects/<cwd with every non-alphanumeric character as '->/<sessionId>.jsonl`
  (default `~/.claude/projects`). `stream` is the daemon's copy of the CLI's stdout
  (`stream.jsonl`, or `pty.log` for the PTY) when the CLI keeps no file (ACP, PTY).
- `PUT sessionUpload.url` with body = gzip of the file, `Content-Type: application/gzip`,
  `X-Session-Sha256` (hex SHA-256 of the uncompressed file), `X-Session-Bytes`, plus
  `sessionUpload.headers`. The daemon adds `Authorization: Bearer <machine token>` only when
  the URL has the same origin as the configured server URL; it never sends the token
  elsewhere. The server sends `{base}/v1/machine/runs/{runId}/session` (base = the origin of
  the daemon's WebSocket upgrade). It needs `Content-Length`, checks that the run was given to
  this machine and stores the body in R2 under `sessions/<runId>.jsonl.gz` (metadata: bot,
  machine, `X-Session-Sha256`, `X-Session-Bytes`). Owners download it with
  `GET /v1/runs/:id/session`.
- At most 100 MiB of gzip. 3 attempts (8 s, then 32 s between them) on network errors and 5xx; a 4xx
  is final. The result is the `run.session` frame (`ok: false` with `error` on failure).

## Run folders

- `<work_root>/runs/<runId>/` with `run.json`, `events.jsonl` (the sequenced frames),
  `stream.jsonl` (raw CLI stdout), `stderr.log`, and `session.jsonl` (the copied raw session).
  `runId` must match `[A-Za-z0-9_-]{1,64}`.
- Work lines: the runs of one member in one topic on one repository share a working folder
  and a branch, so a follow-up run (an answer, "再改一下") finds the files, commits and
  uncommitted changes of the run before it, and a resumed CLI session is in the folder it
  knows. The line is `<work_root>/lines/<key>/` with `work/` (the CLI's working directory)
  and `line.json`; `<key>` is the first 16 hex digits of SHA-256 of
  `botMemberId \n topicId \n url:<repo.url> | path:<repo.path> | ""`, its branch is
  `bot/<key>`. A line is used by one run at a time: a run of the same member in the topic that
  starts while another holds the line gets its own folder `<run folder>/work` and branch
  `bot/<runId>`, and its CLI is told so; a run waits up to 30 s for another run's hold first,
  since a restart comes right after the cancel of the run it continues. A read-only side chat run (`kind: "aside"`) always
  gets its own `<run folder>/work`. `run.json` names the line (`line`) when the run used one.
- `repo.path`: a local repository (inside `repo_roots` when configured). `repo.url`: a bare
  mirror under `<work_root>/repos/` is cloned once and fetched before each run with
  `+refs/heads/*:refs/remotes/origin/*`, so remote branches never touch the local `bot/*`
  branches of lines and runs. A line's `work/` that is there is used as it is (the base
  branch is not merged in). Otherwise the daemon runs `git worktree add work bot/<key>` when
  that branch is still there, else `git worktree add --no-track -b <branch> work <start>`,
  where `<start>` is `origin/bot/<key>` for a line whose branch was pushed under its own name,
  else `origin/<base>` when it exists, else `<base>`, else `HEAD`; `<base>` is `baseBranch`,
  or the branch HEAD names. Without `repo` the line (or the run) gets a scratch folder, kept
  like a worktree. When a resumed run (`resumeSessionId`) gets a line folder that was made
  again, its CLI is told that its folder changed and what is in it.
- Cleanup at start and every 30 minutes: finished run folders older than `keep_runs_hours`
  (default 72) are removed, and while free disk is below `min_free_gb` (default 10) the
  oldest finished ones go first; a run that used a line leaves the line alone. A line goes only
  when no run has used it for `keep_runs_hours` (also when disk is low) and no run holds it.
  Before a worktree (a line's or a run's own) is removed with `git worktree remove`, its
  uncommitted changes are committed to the branch it has checked out (`wip: uncommitted
  changes kept by cleanup`, hooks and signing off; a detached HEAD gets a branch
  `<branch>-kept-<ms>`). When that commit fails, also after a stale `index.lock` of the
  worktree is removed (a git killed with a cancelled run), the folder is moved with its files to
  `<work_root>/kept/<run-<runId> | line-<key>>-<ms>` instead, so it never blocks the cleanup;
  a line `work/` that git cannot use when a run starts is moved there too
  (`line-<key>-lost-<ms>`). Folders in `kept/` stay until free disk is below `min_free_gb`,
  then the oldest go first. A run that starts while the cleanup holds its line waits for it.
  The branch is then deleted
  only when all its commits are on a remote-tracking branch or another local branch; a branch
  with unpushed commits is kept, and the daemon log names it, so the line's next run checks it
  out again. Below half of `min_free_gb` new runs are rejected with `disk_low`.

## MicroVM runs (`[vm]`, Linux)

With `[vm]` in the config each run's CLI runs in its own Firecracker microVM instead of as a
process of the daemon's machine. Nothing in this protocol changes. The VM boots the read-only
base image (`scripts/runner/build-base-image.sh`: Ubuntu with git, Node, Claude Code, Codex,
Pi, bun, uv, gh) with a per-run sparse scratch disk as its writable overlay, and a tools disk
that holds the daemon binary as `vm-agent` (PID 1 after `vm-init`) and `chat-run`. The run
folder, the working folder (the line's `work/`, see Run folders), the run's git dir, the AI
member's own folder (below) and any `[vm].shares` (default none) are NFS shares of the host,
mounted at their host paths, so the worktree, session files, `stderr.log` and 产出 stay where
the daemon reads them, and the member's next run in the topic, in a new VM, finds its working
folder as it was left. Stdin and stdout go over vsock (port 1024) as
frames; `chat-run` in the VM uses the socket `/run/chat-run.sock`, which the agent relays to
the run's socket over vsock (port 1025), so Codex runs use the socket instead of the request
folder. With `[vm].homes` (default `/srv/runner/homes`) the folder `<homes>/<bot member id>` is
the member's own folder, mounted at its host path and as the VM's `$HOME`: what a bot clones,
installs and signs in to is there again in its next run, in every VM of that bot; the rest of
the disk still goes with the VM. With `[vm].home_gb` (default 20) that folder is a disk image
of that size (`<homes>/../home-images/<id>.ext4`, sparse, mounted with `discard`) that the root
helper makes on first use and mounts (`runner-vm home-up`), so one member cannot fill the host;
a larger `home_gb` grows existing images, a smaller one leaves them. A home folder from before
the images moves into its image the first time it is used (when it does not fit it stays a
plain folder without a limit until it is emptied). `home_gb = 0` keeps plain folders. Without
`homes` the member's own folder is `<[vm].dir>/cli/<id>` and only holds its CLIs' folders.
`home.clear` (feature `homes`) empties a member's home: the image goes, its next run starts with
a new one; the daemon refuses while a run of that member is on the machine and holds that
member's new runs (`busy`) while it empties. The CLI's system note says what is kept (the working
folder for the next run in the topic, the home folder for all runs), and that processes left
running when the turn ends are killed. 接管 pauses and resumes the whole VM. When the CLI exits
the agent syncs the shares before the daemon reads them; then the VM, its tap, its exports and
its scratch disk go.

**Sign-ins and session records.** The host's own CLI folders (`~/.claude`, `~/.codex`, `~/.pi`)
are never shared with a VM: the daemon skips any share that is the daemon user's home or in one
of its dot folders, and the root helper refuses to export them. A run's CLI uses its member's
own folders, so session records stay per member: `CLAUDE_CONFIG_DIR=<member>/.claude`,
`CODEX_HOME=<member>/.codex`, `PI_CODING_AGENT_DIR=<member>/.pi/agent` (host paths, mounted at
the same paths in the VM). The host's `settings.json` / `config.toml` are copied in for each
run. The sign-in reaches the run in a form it cannot use to change the host's:

- Claude Code: `CLAUDE_CODE_OAUTH_TOKEN`. A long-lived token (`claude setup-token` as the
  daemon user) given to the machine as an API key with that name (团队管理 › 订阅账号) is used as
  it is, and so is `ANTHROPIC_API_KEY` from the runtime's env; otherwise the host's current
  access token from `.credentials.json` (never its refresh token). When that token would run out
  before the run's limit plus 30 minutes, the host's CLI refreshes it first.
- Codex: `<member>/.codex/auth.json`, a copy of the host's with `refresh_token` set to `""`
  (Codex reads a file without the field as no sign-in). The access token lasts 10 days; when it
  would run out before the run's limit plus 30 minutes, the host's Codex refreshes it first
  (`codex app-server`, `account/read` with `refreshToken: true`), so a refresh never happens in
  a VM and never rotates the host's refresh token away.
- Pi: a copy of its `auth.json` with every `refresh` emptied. When an OAuth access token would
  run out before the run's limit plus 30 minutes, the host's Pi refreshes it first
  (`pi auth print-bearer-token --provider <p> --min-expiry <s>s`, output discarded); when every
  sign-in in it has run out and cannot be renewed, the run fails before it starts.

Refreshes on the host take turns (one at a time per daemon; each first rereads the host's file
and skips when another run just renewed it), so a refresh token is never used twice. Runs of one
member that start together take turns preparing its home (the daemon, and `flock` in the root
helper) and writing its sign-in copy.

A run that resumes a session from before per-member folders gets that one session file copied
from the host's folder into its member's.

**Capacity.** Besides `max_parallel_runs` and the free disk (of `work_root` and of the disk the
homes are on, whichever has less free; `health.diskFreeBytes` reports that one), a VM host
refuses a run (`run.reject`, `busy`) when `MemAvailable` does not cover `[vm].mem_mib` for the
new VM and for each accepted VM that has not booted yet, above a reserve of a fifth of the
memory (at least 2 GiB) for the running VMs to grow into.

The VMs reach the internet. A static nftables table (`scripts/runner/runner-vm.nft`) drops
their traffic to the host (except NFS on the host address of their own tap), to private,
link-local and metadata ranges and to each other; strict reverse-path filtering stops address
spoofing. Each VM's exports name only its own guest address. The daemon runs as an unprivileged
user; the tap device, the NFS exports and the home images go through the root helper
`runner-vm` with `sudo -n`.
`scripts/runner/host-setup.sh` installs all of it; `scripts/runner/net-check.py` boots a VM and
checks its network.

## Environment

The CLI process starts with an empty environment plus: `PATH HOME USER LOGNAME SHELL TMPDIR
LANG LC_ALL LC_CTYPE TERM SSH_AUTH_SOCK CODEX_HOME CLAUDE_CONFIG_DIR` from the daemon; each
name in `run.assign.envAllowlist` that the machine's config also lists in `env_passthrough`
(so the server can choose per bot among the variables the machine owner allowed, never more);
the runtime's fixed `env` from the config; and `CHAT_RUN_ID`, `CHAT_GROUP_ID`,
`CHAT_TOPIC_ID`, `CHAT_BOT_MEMBER_ID`, `CHAT_RUN_FOLDER` (the run folder: it marks the run's
processes for pause and cancel); 1.1 runs that are not side chats also get `CHAT_RUN_SOCKET`
(Codex runs: `CHAT_RUN_DIR`) and `<run folder>/bin` (with `chat-run`) first on `PATH`.

Names in `envAllowlist` that the config lists in `gated_env` are never in the environment. The
CLI asks for one with `chat-run secret <NAME>`: the first call sends `approval.request` (title
`使用 <NAME>`) and fails with `pending`; after `run.approval approve` the daemon emits
`approval.resolved` and the same command prints the value for 30 minutes; after `deny` it fails
with `denied` for the rest of the run. The daemon answers these approvals itself, the CLI never
sees them. Side chat runs get no gated variables.

Group members read run events, so the daemon redacts every frame before it sends or stores
it, and the session file before upload: the exact values of the `env_passthrough` and
`gated_env` variables and the machine token, known token formats (`tc_`, `whk_`, `tm_`, GitHub, OpenAI, Anthropic,
AWS, Slack, Google keys, bearer values, private key blocks, URL passwords, secret query
parameters, invite codes; not the one-time code of a device sign-in such as `gh auth login`,
which a person has to read) and secret-named `key=value` pairs become `<redacted>`.
`message.delta` text is released per line (or after 1 s, all but its last two words), so a
secret split over two deltas is redacted whole.

## Runtimes

| kind | Command | Protocol |
| --- | --- | --- |
| `codex` | `codex app-server` | JSON-RPC lines on stdio (v2): `initialize`, `initialized`, `thread/start {cwd, approvalPolicy, sandbox, model?}`, `turn/start`, `turn/steer`, `turn/interrupt`; notifications `turn/*`, `item/*`, `item/agentMessage/delta`, `thread/tokenUsage/updated`, `error`; server requests `item/commandExecution/requestApproval`, `item/fileChange/requestApproval` |
| `claude` | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages --session-id <uuid> --permission-mode <mode>` | one JSON line per user message in (a line the CLI reads while a turn runs joins that turn; the CLI echoes each line it takes up, and the run ends when a `result` arrives with no line unread); `system/init`, `stream_event` (text deltas), `assistant`, `user` (tool results), `result` out |
| `acp` | configured, e.g. `gemini --acp`, `opencode acp` | Agent Client Protocol v1 (JSON-RPC 2.0): `initialize`, `session/new` or `session/load` (resume, when the agent has `loadSession`), `session/prompt`, `session/update`, `session/request_permission`, `session/cancel`. No effort setting and no read-only mode: the daemon reports no `efforts` and `aside: false` |
| `pty` | configured; `{prompt}` in args is replaced | a pseudo terminal; output without ANSI codes streams as message item `out` |

## Daemon config and commands

`~/.config/<slug>/daemon.toml` (or `--config`, or `CHAT_DAEMON_CONFIG`); `daemon
example-config` prints a commented example. Keys: `server_url`, `token_file`, `machine_id`,
`name`, `labels`, `work_root`, `max_parallel_runs`, `keep_runs_hours`, `min_free_gb`,
`stall_minutes`, `repo_roots`, `env_passthrough`, `gated_env`, `takeover_url` (1.1), `self_update`, `[runtimes.<name>]` with `kind`,
`command`, `args`, `env`, `model`, `asks_permission` (ACP: the agent asks before its tool
calls, see "Gated actions"), and `[vm]` with `kernel`, `rootfs`, `dir`, `vcpus`, `mem_mib`
(a cap: the host gives only the pages the VM touches), `scratch_gb`, `firecracker`, `helper`,
`shares` (default none; never the daemon user's home or its dot folders), `homes`, `home_gb`. `approval_policy`, `sandbox`, `permission_mode` and
`allowed_tools` still parse (1.1 configs) but are ignored since 1.2.1. Without `[runtimes.*]`, `codex` and `claude` are added when
found on PATH.

- `daemon run`: the service. SIGTERM cancels all runs and exits.
- `daemon doctor`: config problems, CLIs and versions, git, disk, load. Never prints secrets.
  Exit code 1 when something is wrong.
- `daemon install-service` writes `~/Library/LaunchAgents/<bundleId>.daemon.plist` (RunAtLoad,
  KeepAlive, the current PATH, logs in `~/Library/Logs/<slug>-daemon/`) and loads it with
  `launchctl bootstrap gui/<uid>`; `--no-load` only writes the file.
  `daemon uninstall-service` unloads and removes it.
- Windows: `daemon install-service` registers a scheduled task of the current user (named by
  security id) called `<bundleId>.daemon` (`schtasks /Create /XML`; the definition is kept in
  `%LOCALAPPDATA%\<slug>-daemon\task.xml`) and starts it. It refuses a daemon whose file, folder
  or a folder above it can be changed by anyone but this user and administrators. The task starts at this user's logon and
  runs only in this user's logon session, so the CLIs see the user's logins; one instance only; a
  second trigger asks for a start every minute, which starts the daemon again within a minute
  after it ended for any reason; no time limit; not stopped on battery. Its program is
  `conhost.exe --headless <daemon> run --config <file> --log-file <file>`: the daemon gets a
  console without a window, and writes its own log to `%LOCALAPPDATA%\<slug>-daemon\daemon.log`
  (moved to `daemon.log.1` above 10 MB at a start). `--no-load` only writes the definition file.
  `daemon uninstall-service` ends and deletes the task and stops its daemon;
  `daemon restart-service` (Windows only) stops the daemon and starts the task again. Ending the
  task ends only the console host, so the task's daemon writes its pid and its process
  creation time to `%LOCALAPPDATA%\<slug>-daemon\daemon.pid` and waits on the event
  `Local\<slug>-daemon-stop-<pid>`: setting it shuts the daemon down as Ctrl+C does (runs are
  stopped and reported as `daemon restarted`). From another logon session of the same user (SSH, WinRM), where the
  event is not visible, or after 20 s, the process (matched by pid and creation time) is
  terminated. Both commands fail when the daemon is still running afterwards.
- Windows, replacing the program file (every updater follows this; the daemon is never stopped
  for an update): write the new program completely as `<name>.exe.new` in the same folder and
  check its SHA-256; rename the running `<name>.exe` to `<name>.exe.old-<anything unique>`;
  rename `<name>.exe.new` to `<name>.exe` (if that fails, rename the old file back). The daemon
  sees the new file, exits once no run is active, and the task starts the new one within a
  minute; a starting daemon deletes the `<name>.exe.old-*` files next to it. An update
  interrupted between the two renames (power loss) leaves no `<name>.exe`: the task's first
  action (`repair.exe repair-program <daemon>`, a copy of the daemon that `install-service`
  keeps next to the log and updates never touch) renames the newest `<name>.exe.old-*` back
  before the daemon is started.
- Updating itself (a daemon installed on its own, today the Linux x86_64 daemon of a host; the
  app updates the daemon it ships on macOS and Windows): when systemd started it (`daemon
  install-service`) and `self_update` is not `false`, the daemon asks
  `GET /v1/machine/updates/latest?platform=linux-x86_64` with its machine token 10 minutes after
  it starts and then every hour (docs/protocol.md, "Updates and downloads"). A release newer than
  its own version (never an older one) is downloaded from the answer's `downloadUrl`, which must
  be on the configured server, to `<program>.new` next to the program, and checked against the
  answer's size and SHA-256; a file that does not match is deleted. Once no run is active the
  daemon renames it over its program (on Windows it would follow the replace steps above) and
  exits; systemd's `Restart=always` starts the new version. If somebody else replaced the
  program file meanwhile, theirs wins and the download is dropped. Every step is logged; a
  failed check is tried again an hour later. A download that waits for the daemon to be idle is
  put in place even if the release was withdrawn on the server meanwhile: withdraw a bad release
  before hosts download it, or replace the program by hand (which wins over the download).
  `scripts/runner/install-daemon.sh` (run by
  `scripts/runner/host-setup.sh`) installs the released daemon of a host the first time.

## Tests

- `cargo test -p daemon`: runtimes against fake CLIs, the golden frames, and the daemon against
  a mock server.
- `bun run test` in `server/`: MachineLink and BotDesk against a fake daemon (dispatch, labels,
  health gating, leases, follow-ups, no bot loops, stall detection).
- `bun apps/daemon/e2e/local-e2e.ts [--port 8793]`: `wrangler dev` with fresh local state plus
  the real daemon and the fake Claude Code CLI. A member mentions the bot; the run must show
  the acknowledgement reaction, no progress message, the done card with `resultMessageId`, the
  topic status `done`, the run log and the uploaded session. It writes `report.json` in a temp folder and stops both processes by
  the PIDs it started. With `--vm` on a runner host every run's CLI runs in a microVM.
