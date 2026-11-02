# Machine protocol (version 1)

The contract between the machine daemon (`apps/daemon`) and the server's **MachineLink**
Durable Object (one per machine). The daemon runs on a team machine (the Mac mini, a member's
laptop, later a container), connects out to the server (no inbound ports), receives runs for
bot members, runs a coding CLI for each run in its own folder, and streams normalized progress
back. **BotDesk** (one per bot member) decides what to run and turns run events into topic
messages; it talks to MachineLink, never to the daemon.

`apps/daemon/src/proto.rs` (serde) is the executable source of truth on the daemon side.
`apps/daemon/tests/fixtures/machine/{up,down}.jsonl` hold one golden frame per line; the
daemon test parses each one and serializes it back to an equal value. A server
implementation must accept the same files. Conventions are those of [protocol.md](protocol.md):
JSON, UTF-8, camelCase, unknown fields ignored, times in ms since the epoch, IDs are ULIDs
made by the server (except the ids below that the daemon or the CLI makes), one frame at most
1 MiB.

## Machine token

- The owner adds a machine in settings (机器). The server makes a machine member row in D1
  (`machines`: id, name, owner, labels, token hash, created, last seen) and a token
  `tm_` + 43 characters of base64url (32 random bytes). The server stores only the SHA-256 hex
  digest, like member tokens. The token can be revoked; a revoked token closes the link with
  `error.code = "unauthorized"`.
- The daemon reads the token from `token_file` (mode 600; the daemon refuses group- or
  world-readable files) and never logs or prints it.

## Connection

`GET {base}/v1/machine/ws` with header `Authorization: Bearer <machine token>`. The Worker
rejects a missing or bad token with HTTP 401 before the upgrade and routes the upgrade to the
MachineLink of that machine. One machine has at most one live link: a new link replaces the
old one (MachineLink closes the old socket).

Every frame has a `t` field. The first daemon frame is `hello`; the server answers `welcome`.
Before `welcome` the daemon sends nothing else, and the server sends nothing but `welcome` or
`error`.

## Daemon to server

```ts
{ t: "hello", protocol: 1,
  machine: { id?: string, name: string, labels: string[], daemonVersion: string,
             os: string, arch: string, maxParallelRuns: number },
  clis: CliInfo[], health: Health, runs: RunCursor[] }
  // machine.id is the configured id (optional); the token is authoritative.
  // runs = every run the daemon still holds (running, or finished with frames not yet acked)

{ t: "heartbeat", at, health: Health, runs: RunCursor[] }      // every 30 s; renews leases
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
RunCursor = { runId, lastSeq: number /* highest seq produced */, done: boolean }
Usage     = { inputTokens: number, outputTokens: number, costUsd?: number }
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
  repo?: { url?: string, path?: string, baseBranch?: string },   // url xor path; none = scratch folder
  envAllowlist: string[],          // names of daemon env vars the CLI may see (see Environment)
  leaseMs: number,                 // e.g. 600000
  maxDurationMs?: number,          // the daemon cancels the run after this; status "timeout"
  sessionUpload?: { url: string, headers?: Record<string, string> } }
{ t: "run.input",    runId, inputId, text, authorId? }           // a follow-up from the topic
{ t: "run.approval", runId, approvalId, decision: "approve" | "deny" }
{ t: "run.cancel",   runId, reason? }
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
  | { type: "approval.request", approvalId, itemId?, title, detail? }   // answer with run.approval
  | { type: "approval.resolved", approvalId, decision: "approve" | "deny" }
  | { type: "input.accepted",   inputId }                               // the CLI took this run.input
  | { type: "status",           state: "preparing" | "running" | "retrying" | "stalled" | "error", message? }
```

- `status: "preparing"` comes first (worktree setup), then `running`.
- `status: "stalled"` is sent once after `stall_minutes` (default 5) without any event; the
  next event clears it. BotDesk uses it for the topic's stuck state.
- `tool.result.itemId` equals the `tool.call.itemId` it answers.

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
- `run.assign` is idempotent: when the daemon already holds the run it answers `run.accept`
  again. When the server gets neither `run.accept` nor `run.reject` within 30 s, or the link
  drops first, it sends the same `run.assign` again on the next link.

## Leases

- `run.assign.leaseMs` is the time the server waits without hearing about a run before it
  declares the run lost. Every `heartbeat` lists the daemon's runs and renews each listed
  run's lease to `now + leaseMs`; any `run.*` frame for a run also renews it.
- When a lease expires, the server marks the run lost (BotDesk tells the topic) and leaves it
  out of later `welcome.runs`, so the daemon stops it on reconnect.
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
- `approval.request` comes from CLIs that ask (Codex with `approval_policy` other than
  `never`, ACP `session/request_permission`). The server answers `run.approval`; the daemon
  answers the CLI and emits `approval.resolved`. Claude Code runs with a fixed
  `permission_mode` and does not ask.
- `run.cancel` interrupts the CLI (Codex `turn/interrupt`, ACP `session/cancel`), then sends
  SIGTERM to the CLI's whole process group, SIGKILL after 3 s, and ends with
  `run.done status = "cancelled"`. `maxDurationMs` does the same with `status = "timeout"`.

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
  elsewhere. Suggested URL: `{base}/v1/machine/runs/{runId}/session` (the server checks that
  the run belongs to this machine and stores the body in R2 under
  `sessions/{botMemberId}/{runId}.jsonl.gz`).
- At most 100 MiB of gzip. 3 attempts (8 s, then 32 s between them) on network errors and 5xx; a 4xx
  is final. The result is the `run.session` frame (`ok: false` with `error` on failure).

## Run folders

- `<work_root>/runs/<runId>/` with `work/` (the CLI's working directory), `run.json`,
  `events.jsonl` (the sequenced frames), `stream.jsonl` (raw CLI stdout), `stderr.log`, and
  `session.jsonl` (the copied raw session). `runId` must match `[A-Za-z0-9_-]{1,64}`.
- `repo.path`: a local repository (inside `repo_roots` when configured). `repo.url`: a bare
  mirror under `<work_root>/repos/` is cloned once and fetched before each run. Either way
  the daemon runs `git worktree add -b bot/<runId> work <start>`, where `<start>` is
  `origin/<baseBranch>` when it exists, else `<baseBranch>`, else `HEAD`. Without `repo` the
  run gets an empty scratch folder.
- Cleanup at start and every 30 minutes: finished runs older than `keep_runs_hours`
  (default 72) are removed with `git worktree remove` and `git branch -D bot/<runId>`; while
  free disk is below `min_free_gb` (default 10) the oldest finished runs go first. Below half
  of `min_free_gb` new runs are rejected with `disk_low`.

## Environment

The CLI process starts with an empty environment plus: `PATH HOME USER LOGNAME SHELL TMPDIR
LANG LC_ALL LC_CTYPE TERM SSH_AUTH_SOCK CODEX_HOME CLAUDE_CONFIG_DIR` from the daemon; each
name in `run.assign.envAllowlist` that the machine's config also lists in `env_passthrough`
(so the server can choose per bot among the variables the machine owner allowed, never more);
the runtime's fixed `env` from the config; and `CHAT_RUN_ID`, `CHAT_GROUP_ID`,
`CHAT_TOPIC_ID`, `CHAT_BOT_MEMBER_ID`.

## Runtimes

| kind | Command | Protocol |
| --- | --- | --- |
| `codex` | `codex app-server` | JSON-RPC lines on stdio (v2): `initialize`, `initialized`, `thread/start {cwd, approvalPolicy, sandbox, model?}`, `turn/start`, `turn/steer`, `turn/interrupt`; notifications `turn/*`, `item/*`, `item/agentMessage/delta`, `thread/tokenUsage/updated`, `error`; server requests `item/commandExecution/requestApproval`, `item/fileChange/requestApproval` |
| `claude` | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --session-id <uuid> --permission-mode <mode>` | one JSON line per user message in; `system/init`, `stream_event` (text deltas), `assistant`, `user` (tool results), `result` out |
| `acp` | configured, e.g. `gemini --acp`, `opencode acp` | Agent Client Protocol v1 (JSON-RPC 2.0): `initialize`, `session/new`, `session/prompt`, `session/update`, `session/request_permission`, `session/cancel` |
| `pty` | configured; `{prompt}` in args is replaced | a pseudo terminal; output without ANSI codes streams as message item `out` |

## Daemon config and commands

`~/.config/<slug>/daemon.toml` (or `--config`, or `CHAT_DAEMON_CONFIG`); `daemon
example-config` prints a commented example. Keys: `server_url`, `token_file`, `machine_id`,
`name`, `labels`, `work_root`, `max_parallel_runs`, `keep_runs_hours`, `min_free_gb`,
`stall_minutes`, `repo_roots`, `env_passthrough`, and `[runtimes.<name>]` with `kind`,
`command`, `args`, `env`, `model`, `approval_policy`, `sandbox` (Codex), `permission_mode`,
`allowed_tools` (Claude Code). Without `[runtimes.*]`, `codex` and `claude` are added when
found on PATH.

- `daemon run`: the service. SIGTERM cancels all runs and exits.
- `daemon doctor`: config problems, CLIs and versions, git, disk, load. Never prints secrets.
  Exit code 1 when something is wrong.
- `daemon install-service` writes `~/Library/LaunchAgents/<bundleId>.daemon.plist` (RunAtLoad,
  KeepAlive, the current PATH, logs in `~/Library/Logs/<slug>-daemon/`) and loads it with
  `launchctl bootstrap gui/<uid>`; `--no-load` only writes the file.
  `daemon uninstall-service` unloads and removes it.
