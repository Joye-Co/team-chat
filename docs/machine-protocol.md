# Machine protocol (version 1)

The contract between the machine daemon (`apps/daemon`) and the server's **MachineLink**
Durable Object (one per machine). The daemon runs on a team machine (the Mac mini, a member's
laptop, later a container), connects out to the server (no inbound ports), receives runs for
bot members, runs a coding CLI for each run in its own folder, and streams normalized progress
back. **BotDesk** (one per bot member) decides what to run and turns run events into topic
messages; it talks to MachineLink, never to the daemon.

The shared types are `packages/protocol/src/machine.ts` (Zod, used by the server) and
`crates/chat-protocol/src/machine.rs` (serde, used by the daemon through
`apps/daemon/src/proto.rs`). `apps/daemon/tests/fixtures/machine/{up,down}.jsonl` hold one
golden frame per line; the daemon test parses each one and serializes it back to an equal
value, and `bun run gen:fixtures` copies them into `protocol/fixtures/machine/`, which both
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
  sessionUpload?: { url: string, headers?: Record<string, string> },
  kind?: "aside" }                 // 1.1, feature "aside": a read-only side chat run
{ t: "run.input",    runId, inputId, text, authorId?,            // a follow-up from the topic
  answer?: RunAnswer, artifactEvent?: RunArtifactEvent }         // 1.1, feature "answer"
{ t: "run.approval", runId, approvalId, decision: "approve" | "deny" }
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
  | { type: "approval.request", approvalId, itemId?, title, detail? }   // answer with run.approval
  | { type: "approval.resolved", approvalId, decision: "approve" | "deny" }
  | { type: "input.accepted",   inputId }                               // the CLI took this run.input
  | { type: "plan",             items: RunPlanItem[] }                  // replaces the run plan (protocol.md)
  | { type: "status",           state: "preparing" | "running" | "retrying" | "stalled" | "error", message? }
  | { type: "acceptance",       items: AcceptanceItem[] }               // 1.1: replaces BotRun.acceptance
  | { type: "paused",           paused: boolean, by? }                  // 1.1: after run.pause / run.resume
```

- `status: "preparing"` comes first (worktree setup), then `running`.
- `status: "stalled"` is sent once after `stall_minutes` (default 5) without any event; the
  next event clears it. BotDesk uses it for the topic's stuck state.
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
  (`failed`, error `daemon restarted`) and a failed `run.session`. Their frames, and those of
  runs that finished in the last hour, are offered in `hello` and replayed after `welcome` as
  above. On a graceful stop the daemon cancels its runs and waits up to 10 s for the server to
  ack their last frames.
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

## Leases

- `run.assign.leaseMs` is the time the server waits without hearing about a run before it
  declares the run lost. Every `heartbeat` lists the daemon's runs and renews each listed
  run's lease to `now + leaseMs`; any `run.*` frame for a run also renews it.
- When a lease expires, the server marks the run lost and leaves it out of later
  `welcome.runs`, so the daemon stops it on reconnect. BotDesk sets the run and the topic to
  `blocked` with one short note. A lost run is never assigned again: a new run needs a new
  mention. Late frames of a lost or closed run are acked and dropped.
- The server's lease is 10 minutes (`leaseMs = 600000`) and `maxDurationMs` is 2 hours.
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
| `aside` | `run.assign.kind = "aside"`: the run is read-only (Codex sandbox `read-only` with no MCP servers or web tools, Claude Code permission mode `plan` with only the read tools, no repo write) whatever `daemon.toml` or the CLI's own settings say. |
| `acceptance` | The daemon forwards the CLI's acceptance criteria as the `acceptance` run event. |

How the 1.1 daemon does each one:

- **pause**: the daemon stops the CLI's process group (`SIGSTOP`, the group from `run.json`) for
  every runtime kind, also Codex: one mechanism, no CLI-specific interrupt. CLIs start tool
  commands in process groups or sessions of their own (Claude Code's Bash tool, Codex exec), so
  the daemon then finds the CLI's descendants (libproc on macOS, `/proc` on Linux) and stops
  each of their groups, scanning again until a scan finds nothing new. It never signals its own
  group, pid 1, or a process outside the run's tree. Resume and cancel send `SIGCONT` to the
  same groups. The daemon keeps each target's start time (libproc, or the `starttime` field of
  `/proc/<pid>/stat`) and checks it before each later signal, so a pid that now belongs to
  another process gets no signal. A pause that comes
  while the run is still `preparing` stops the CLI as soon as it starts. While paused, no
  `stalled` status is sent and the `maxDurationMs` clock stands still. `run.resume` sends
  `SIGCONT`, moves the deadline by the paused time, emits `paused { paused: false }`, and sends
  `note` (cut to 2000 characters) as the next input with `inputId` `resume-<n>`. A
  `run.cancel` while paused sends `SIGCONT` first (a stopped process cannot handle
  `SIGTERM`), emits `paused { paused: false }`, then cancels as usual. A repeated pause, or a
  resume of a run that is not paused, does nothing.
- **answer**: the CLI gets `text` (when not empty) followed by a fenced `json` block for
  `answer` and one for `artifactEvent`, each after a line that labels it as untrusted data from a
  member of the group. The fence is longer than any backtick run in the data, so the data cannot
  close it. It is the CLI's next turn in the same session (Claude Code: one more stream-json user
  line; Codex: `turn/steer`, or the next `turn/start`). When the CLI has already ended, the input
  is not taken (see "Follow-up input"), and BotDesk starts a new run.
- **aside**: `run.assign.kind = "aside"` is accepted only for runtimes of kind `claude` and
  `codex`; others are rejected with `runtime_unavailable` (ACP agents and the PTY have no
  read-only mode the daemon can force). A `kind` this daemon does not know is rejected with
  `invalid`. Claude Code gets `--permission-mode plan --tools Read,Grep,Glob --disallowedTools
  "Bash Edit Write MultiEdit NotebookEdit WebFetch WebSearch mcp__*" --strict-mcp-config
  --mcp-config '{"mcpServers":{}}' --setting-sources user`: no MCP servers, and only the
  user's own settings file (its env, `apiKeyHelper` and model keep the run's auth and model);
  the project and local settings come from the repo and stay out. The tool set keeps a user
  allow rule for a command or edit tool from widening the run. Codex gets
  `approvalPolicy: "never"` and `sandbox: "read-only"` (no writes, no network for commands),
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
  itself is sent unchanged.

`takeoverUrl` comes from `daemon.toml` (`takeover_url`, for example `vnc://mac-mini.tailnet`).
It must be `scheme://host...`, at most 2048 bytes, with no spaces and no password (`daemon
doctor` reports a bad one, and the daemon does not send it). A value that redaction would
change is not sent either.
The owner turns on macOS Screen Sharing by hand; the daemon never changes system settings, and
no credentials pass through chat. The server shows the address to owners and run requesters.

### Interaction loop (features `interact` and `output`)

```ts
// server -> daemon (feature "interact"; an old daemon gets run.input with the text envelope instead)
{ t: "run.answer", runId, inputId, source: { kind: "card", messageId, askId, answerId, rev },
  actionId?, blockId?, event, value?, inputs?, editedValues?, actor: { id, name }, eventId?, block? /* the block at rev */ }
{ t: "run.interaction", runId, inputId,
  source: { kind: "card", messageId, blockId?, actionId?, rev } | { kind: "artifact", artifactId, version, eventId, ask },
  event, value?, actor: { id, name }, eventId?, block? /* card events: the block or action at rev */ }
// daemon -> server (feature "output", `chat-run`): a normal run.event with seq and ack
RunEvent += { type: "output", outputId, op: "card.post" | "card.patch" | "interaction.resolve" | "artifact.data" | "artifact.reply", body: JSON }
// server -> daemon
{ t: "run.output.result", runId, outputId, ok, result?, error? { code, reason?, message } }
run.input.answer += { answerId?, askId?, eventId?, event?, blockId?, value? }
```

The daemon gives `run.answer` and `run.interaction` to the CLI as one user turn: a
`<teamchat-event v="1" id="{inputId}" kind="card.answer|card.event|artifact.emit|artifact.ask">`
envelope with the JSON, followed by the line that it is data, not an instruction. It answers
`input.accepted {inputId}` as for `run.input`. BotDesk performs each `output` op as the bot member
with the rules of the matching WebSocket frame. The `output` body is that frame without `t`, `rid` and
`groupId` (BotDesk takes the group and topic from the run):

| `op` | `body` | `chat-run` command |
| --- | --- | --- |
| `card.post` | `{ cardV2, body?, causeEventId? }` (a `send`) | `chat-run card post --file card.json [--cause ev]` |
| `card.patch` | `{ messageId, baseRev, ops, cause? }` | `chat-run card patch --message M --base-rev N --ops ops.json [--cause ev]` |
| `interaction.resolve` | `{ eventId, result: "ignored" \| "failed", note? }` | `chat-run resolve --event ev --result ignored\|failed [--note …]` |
| `artifact.data` | `{ artifactId, version, data, baseDataRev?, cause? }` | `chat-run artifact data --artifact A --version V --file data.json [--base-data-rev N] [--cause ev]` |
| `artifact.reply` | `{ eventId, ok: true, payload? }` or `{ eventId, ok: false, error: { code: "declined", message } }` | `chat-run artifact reply --event ev --file reply.json` or `--decline "…"` |

`chat-run` is the daemon binary started by the name `chat-run` (argv0). The daemon links it as
`<run folder>/bin/chat-run`, puts that folder first on the CLI's `PATH`, and sets
`CHAT_RUN_SOCKET` to a unix socket of the run (`<run folder>/chat-run.sock`, or a short path in
`$TMPDIR` when that one is too long for a socket address), mode 0600, accepting only the
daemon's own user. The socket is removed when the run ends. Side chat runs (`aside`) get
neither. One request is one JSON line `{ op, body }`; the daemon sends the `output` run event
(redacted like every frame) and answers one JSON line `{ ok, result? , error? }` when
`run.output.result` comes, or after 15 s `{ ok: false, error: { code: "timeout", reason:
"server.too_old" } }`. `chat-run` prints the result JSON (exit 0) or the error JSON with its
`reason` (exit 1). The CLI learns about `chat-run` from a short note: Claude Code
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
  payload? | error?}` or `{eventId, decline: "…"}`. Without `clientPatchId` / `clientPushId` in
  the body, the key is `o:{runId}:{outputId}`, so a later run's `o1` is never a repeat. An unknown op gets `invalid` with reason
  `output.unknown_op`; an aside run gets `forbidden` (`output.read_only`). The server measures an
  `output`'s age on its own clock: the frame's `at` moved by the clock offset of the last
  `heartbeat` (or the arrival time before the first heartbeat), never later than the arrival. An
  `output` older than 12 s when it is applied is not performed; it gets `ok: false` with code
  `conflict` and reason `output.expired`, so an output that `chat-run` reported as timed out (15 s)
  is never done later, and a machine clock behind the server drops nothing.

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

- `<work_root>/runs/<runId>/` with `work/` (the CLI's working directory), `run.json`,
  `events.jsonl` (the sequenced frames), `stream.jsonl` (raw CLI stdout), `stderr.log`, and
  `session.jsonl` (the copied raw session). `runId` must match `[A-Za-z0-9_-]{1,64}`.
- `repo.path`: a local repository (inside `repo_roots` when configured). `repo.url`: a bare
  mirror under `<work_root>/repos/` is cloned once and fetched before each run with
  `+refs/heads/*:refs/remotes/origin/*`, so remote branches never touch the local
  `bot/<runId>` branches of other runs. Either way the daemon runs
  `git worktree add --no-track -b bot/<runId> work <start>`, where `<start>` is
  `origin/<branch>` when it exists, else `<branch>`, else `HEAD`; `<branch>` is `baseBranch`,
  or the branch HEAD names. Without `repo` the run gets an empty scratch folder.
- Cleanup at start and every 30 minutes: finished runs older than `keep_runs_hours`
  (default 72) are removed with `git worktree remove`; while free disk is below `min_free_gb`
  (default 10) the oldest finished runs go first. `bot/<runId>` is deleted only when all its
  commits are on a remote-tracking branch or another local branch; a branch with unpushed
  commits is kept, and the daemon log names it. Below half of `min_free_gb` new runs are
  rejected with `disk_low`.

## Environment

The CLI process starts with an empty environment plus: `PATH HOME USER LOGNAME SHELL TMPDIR
LANG LC_ALL LC_CTYPE TERM SSH_AUTH_SOCK CODEX_HOME CLAUDE_CONFIG_DIR` from the daemon; each
name in `run.assign.envAllowlist` that the machine's config also lists in `env_passthrough`
(so the server can choose per bot among the variables the machine owner allowed, never more);
the runtime's fixed `env` from the config; and `CHAT_RUN_ID`, `CHAT_GROUP_ID`,
`CHAT_TOPIC_ID`, `CHAT_BOT_MEMBER_ID`; 1.1 runs that are not side chats also get `CHAT_RUN_SOCKET`
and `<run folder>/bin` (with `chat-run`) first on `PATH`.

Group members read run events, so the daemon redacts every frame before it sends or stores
it, and the session file before upload: the exact values of the `env_passthrough` variables
and the machine token, known token formats (`tc_`, `whk_`, `tm_`, GitHub, OpenAI, Anthropic,
AWS, Slack, Google keys, bearer values, private key blocks, URL passwords, secret query
parameters, invite codes) and secret-named `key=value` pairs become `<redacted>`.
`message.delta` text is released per line (or after 1 s, all but its last two words), so a
secret split over two deltas is redacted whole.

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
`stall_minutes`, `repo_roots`, `env_passthrough`, `takeover_url` (1.1), and `[runtimes.<name>]` with `kind`,
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

## Tests

- `cargo test -p daemon`: runtimes against fake CLIs, the golden frames, and the daemon against
  a mock server.
- `bun run test` in `server/`: MachineLink and BotDesk against a fake daemon (dispatch, labels,
  health gating, leases, follow-ups, no bot loops, stall detection).
- `bun apps/daemon/e2e/local-e2e.ts [--port 8793]`: `wrangler dev` with fresh local state plus
  the real daemon and the fake Claude Code CLI. A member mentions the bot; the run must show
  the acknowledgement reaction, no progress message, the done card with `resultMessageId`, the
  topic status `done`, the run log and the uploaded session. It writes `report.json` in a temp folder and stops both processes by
  the PIDs it started.
