// 盯 1.2, 1.7; 接 3.5; 机 2.6, 5.3: when the desk starts a piece of work again (a stall, a daemon
// restart, a computer that went away), the new turn continues it instead of starting over. The
// recovery note is written by code from what the server holds: the plan, the last steps and
// every outward action the earlier tries made. Pure, so it is easy to test.
import type { RunPlan } from "@chat/protocol";
import { cut } from "../bots/dispatch";

/** `run.done.error` of a run the daemon found unfinished when it started (machine-protocol.md, "Daemon restart"). */
export const DAEMON_RESTARTED = "daemon restarted";
/** A run is resumed after a daemon restart at most this often in a row; then it fails with the reason. */
export const MAX_DAEMON_RESUMES = 3;
/** Steps (tool calls) the note lists, newest last. */
export const NOTE_STEPS = 20;
/** Outward actions the note lists at most. */
export const NOTE_ACTIONS = 30;

/** One stored run frame, as BotDesk keeps it in `run_log` (only the fields read here). */
export interface LoggedFrame {
  t: string;
  event?: { type?: unknown; title?: unknown; tool?: unknown; input?: unknown; op?: unknown; body?: unknown };
}

/** Shell commands that reach outside the machine. Matched against a tool call's command. */
const OUTWARD: [RegExp, string][] = [
  // Options before the subcommand, with the value of `-C <dir>` and `-c <key=value>`.
  [/\bgit(?:\s+(?:-[cC]\s+\S+|-\S+))*\s+push\b/i, "pushed with git"],
  [/\bgh\s+pr\s+(?:create|merge|close|reopen|comment|edit|review|ready)\b/i, "changed a pull request with gh"],
  [/\bgh\s+(?:issue|release)\s+(?:create|comment|close|edit|delete|upload)\b/i, "changed GitHub with gh"],
  [/\bgh\s+api\b[^\n]*(?:-X|--method)\s*(?:POST|PUT|PATCH|DELETE)\b/i, "called the GitHub API with gh"],
  [/\b(?:npm|pnpm|yarn|bun|cargo)\s+publish\b/i, "published a package"],
  [/\bwrangler\s+(?:deploy|publish)\b/i, "deployed with wrangler"],
];

/** The command text of a tool call: its input's `command` when there is one, else its title. */
function commandOf(ev: NonNullable<LoggedFrame["event"]>): string {
  const input = ev.input as { command?: unknown; cmd?: unknown } | undefined;
  const raw = input && typeof input === "object" ? (input.command ?? input.cmd) : undefined;
  const cmd = Array.isArray(raw) ? raw.filter((x) => typeof x === "string").join(" ") : typeof raw === "string" ? raw : "";
  return cmd || (typeof ev.title === "string" ? ev.title : "");
}

/** What a `chat-run` output did in the chat, for the note (null for ops that change nothing outside). */
function outputAction(op: string, body: unknown): string | null {
  const b = body && typeof body === "object" ? (body as Record<string, unknown>) : {};
  const card = (b.cardV2 ?? b.card) as { title?: unknown } | undefined;
  const title = card && typeof card.title === "string" ? `: ${cut(card.title, 80)}` : "";
  const artifact = typeof b.artifactId === "string" ? ` ${b.artifactId}` : "";
  switch (op) {
    case "card.post":
      return `posted a card in the topic${title}`;
    case "card.patch":
      return "changed a card in the topic";
    case "interaction.resolve":
      return "answered an interaction in the topic";
    case "artifact.data":
      return `changed the data of 产出${artifact}`;
    case "artifact.reply":
      return `answered an event of 产出${artifact}`;
    case "member.handoff":
      return "handed this work to another member";
    case "member.assign":
      return "gave part of this work to another member";
    case "desk.note":
      return "changed the group's desk note";
    default:
      return null;
  }
}

/** The outward action a stored frame records, or null. */
export function outwardAction(f: LoggedFrame): string | null {
  if (f.t !== "run.event" || !f.event) return null;
  const ev = f.event;
  if (ev.type === "output") return outputAction(String(ev.op ?? ""), ev.body);
  if (ev.type !== "tool.call") return null;
  const cmd = commandOf(ev);
  for (const [re, what] of OUTWARD) if (re.test(cmd)) return `${what}: ${cut(cmd.replace(/\s+/g, " ").trim(), 160)}`;
  return null;
}

/** The title of a step (tool call), or null for any other frame. */
export function stepOf(f: LoggedFrame): string | null {
  if (f.t !== "run.event" || f.event?.type !== "tool.call") return null;
  const t = String(f.event.title || f.event.tool || "").replace(/\s+/g, " ").trim();
  return t ? cut(t, 160) : null;
}

/** What the earlier tries of a piece of work did, oldest first. */
export interface RecoveryFacts {
  /** Why the work starts again (developer words: the CLI reads them). */
  why: string;
  /** How many tries came before this one. */
  tries: number;
  plan: RunPlan | null;
  /** Step titles, oldest first. */
  steps: string[];
  /** Outward actions from the run events, oldest first. */
  actions: string[];
  /** Pull requests linked to the topic. */
  prs: string[];
  /** 产出 versions the bot made in the topic since the work began. */
  artifacts: string[];
}

/** True when the earlier tries reached outside: pushes, pull requests, cards, 产出 (机 2.6). */
export function actedOutward(f: Pick<RecoveryFacts, "actions" | "artifacts">): boolean {
  return f.actions.length > 0 || f.artifacts.length > 0;
}

function dedupe(xs: string[]): string[] {
  return [...new Set(xs)];
}

/**
 * The note the new turn starts with (developer words are fine: the CLI reads it). It says the
 * work is being continued, where the plan stood, the last steps, and every outward action the
 * earlier tries made, with the instruction to check them before repeating anything.
 */
export function recoveryNote(f: RecoveryFacts): string {
  const lines = [
    `[team-chat] Recovery note, written automatically (not a message from a person): this request was already being worked on, and that work stopped (${cut(f.why, 80)}). This is try ${f.tries + 1}. Continue the work from where it stopped; do not start it over.`,
  ];
  if (f.plan && f.plan.length > 0) {
    lines.push("", "Where the plan stood:");
    for (const p of f.plan) lines.push(`- [${p.state}] ${cut(p.text, 160)}`);
  }
  const steps = f.steps.slice(-NOTE_STEPS);
  if (steps.length > 0) {
    lines.push("", `The last ${steps.length} steps, oldest first:`);
    for (const s of steps) lines.push(`- ${s}`);
  }
  const actions = dedupe(f.actions).slice(-NOTE_ACTIONS);
  if (actions.length > 0 || f.artifacts.length > 0) {
    lines.push("", "Actions the earlier tries already took outside this machine:");
    for (const a of actions) lines.push(`- ${a}`);
    for (const a of dedupe(f.artifacts)) lines.push(`- made 产出 ${a}`);
  }
  if (f.prs.length > 0) {
    lines.push("", "Pull requests linked to this topic:");
    for (const p of dedupe(f.prs)) lines.push(`- ${p}`);
  }
  lines.push(
    "",
    actedOutward(f) || f.prs.length > 0
      ? "Before you do anything else, check the state of each of these (git status, git log, the remote branch, gh pr list/view, the topic and its cards and 产出). Do not push, open or change a pull request, post a card or make 产出 again when it is already there: verify it and go on with what is still missing."
      : "Check the working folder (git status, git log) for what the earlier tries left, then go on with what is still missing.",
  );
  return lines.join("\n");
}
