// Pure rules of the bots feature: machine choice, health gating, approval answers, prompt and
// progress text. No I/O here, so the rules are easy to test.
import type { BotRunner, CliInfo, Health, Machine, MachineInfo } from "@chat/protocol";
import { REFS_RULE } from "./refs";

/** Lease the server gives each run; heartbeats and run frames renew it. */
export const LEASE_MS = 10 * 60_000;
/** No run event for this long: the topic is set to blocked with one note. */
export const STALL_MS = 10 * 60_000;
/** A machine whose last heartbeat is older than this is not healthy (3 heartbeats). */
export const HEARTBEAT_STALE_MS = 90_000;
/** The link closes when nothing arrives for this long. */
export const LINK_IDLE_MS = 90_000;
/** Resend run.assign when neither accept nor reject came. */
export const ASSIGN_RETRY_MS = 30_000;
/** Below this much free disk a machine takes no new runs. */
export const MIN_FREE_DISK_BYTES = 5 * 1024 ** 3;
/** The daemon cancels a run after this; status "timeout". */
export const MAX_RUN_MS = 2 * 60 * 60_000;
/** Longest progress message body we stream (the message body limit is 32,768). */
export const MAX_PROGRESS_CHARS = 30_000;
/** Longest result message body. */
export const MAX_RESULT_CHARS = 30_000;
/** Messages of the topic given to the CLI as context (side chats and the `messages` list). */
export const CONTEXT_MESSAGES = 30;
/** Characters of topic lines in a run's prompt (docs/protocol.md, "Run context"). */
export const CONTEXT_CHARS = 24_000;

/** How a run with the `context` feature reads what the prompt left out. */
export const CONTEXT_TOOLS_RULE =
  "The topic lines above are only part of the topic. Read older messages with `chat-run topic history --before <seq>`, a whole message with `chat-run topic message --id <m id>`, search with `chat-run search --query <text>` (add `--group` for every topic of the group) and get a file with `chat-run file get --id <f id>`. Look things up instead of guessing.";

/** Topic texts the bot posts. Product words only (PRODUCT.md): no developer words. */
export const TEXT = {
  noMachine: "现在没有空闲的电脑能接手，请稍后再 @我。",
  lost: "卡住了：处理中断了（电脑离线太久），这次没有完成。需要的话请再 @我。",
  stalled: "卡住了：10 分钟没有进展。",
  /** After the desk's own tries (盯活): told once. */
  gaveUp: "卡住了：我自己试了几次（提醒、换电脑重来）还是没有进展，需要你看一下。",
  progress: "进行中…",
  done: "完成了。",
  failed: (why: string) => (why ? `没有完成：${why}` : "没有完成。"),
  cancelled: "已停止，没有完成。",
  followUpLimit: "后面的消息没有接着处理，需要的话请再 @我。",
  approval: (title: string, detail?: string) =>
    `需要你确认：${title}${detail ? `\n\n${detail}` : ""}\n\n回复「同意」或「拒绝」。`,
  approve: "同意",
  deny: "拒绝",
  /** A gated command (feature "gates"): the kind, the command, and the third answer. */
  approvalGate: (kind: string, command?: string) =>
    `需要你确认：${kind}${command ? `\n\n${command}` : ""}\n\n回复「同意」「拒绝」或「这件事里都同意」。`,
  approveRun: "这件事里都同意",
  approveRunConfirm: "这件事里之后同类的操作都不再问你。",
  /** The approval card after the answer (DESIGN.md §7.10 answered): title and one record line. */
  approvalClosed: {
    approve: { title: (what: string) => `已同意：${what}`, line: (who: string, at: string) => `${who} 同意了 · ${at}` },
    approve_run: { title: (what: string) => `已同意：${what}`, line: (who: string, at: string) => `${who} 同意了，这件事里同类的操作不再问 · ${at}` },
    deny: { title: (what: string) => `已拒绝：${what}`, line: (who: string, at: string) => `${who} 拒绝了 · ${at}` },
    expired: { title: (what: string) => `没用上：${what}`, line: (_who: string, at: string) => `这件事结束了，没等到回答 · ${at}` },
  },
  /** 设置 › 默认规则 names of the gated kinds. */
  gateKinds: {
    mergeMain: "合并到 main",
    deployProd: "发布到生产",
    prodConfig: "改生产环境配置",
    deleteData: "删除数据或文件",
    externalPublish: "对外发布",
  } as Record<string, string>,
  noAsideMachine: "现在没有能单独回答的电脑，请稍后再问。",
} as const;

/** The machine's daemon reports this 1.1 feature (docs/machine-protocol.md, "1.1 features"). */
export function hasFeature(m: Pick<MachineView, "info">, feature: string): boolean {
  return (m.info?.features ?? []).includes(feature);
}

/**
 * The machine stops this runtime's gated commands itself: its daemon has feature "gates" and the
 * CLI reports `gates` (machine-protocol.md, "Gated actions").
 */
export function gatesRuntime(m: Pick<MachineView, "info" | "clis">, runtime: string): boolean {
  return hasFeature(m, "gates") && (m.clis ?? []).some((c) => c.runtime === runtime && c.gates === true);
}

export interface MachineView {
  id: string;
  name: string;
  /** Owner labels and daemon labels. */
  labels: string[];
  revoked: boolean;
  connected: boolean;
  lastHeartbeatAt?: number;
  info?: MachineInfo;
  health?: Health;
  clis?: CliInfo[];
  runningRuns: string[];
}

export function mergeLabels(owner: string[], daemon: string[] | undefined): string[] {
  return [...new Set([...owner, ...(daemon ?? [])])].sort();
}

/** Connected, a recent heartbeat, enough free disk. */
export function isHealthy(m: MachineView, now: number): boolean {
  if (m.revoked || !m.connected || !m.health || m.lastHeartbeatAt === undefined) return false;
  if (now - m.lastHeartbeatAt > HEARTBEAT_STALE_MS) return false;
  return m.health.diskFreeBytes >= MIN_FREE_DISK_BYTES;
}

/** The machine has every label the bot asks for. */
export function labelsMatch(required: string[], have: string[]): boolean {
  const set = new Set(have);
  return required.every((l) => set.has(l));
}

export function hasRuntime(m: MachineView, runtime: string): boolean {
  return (m.clis ?? []).some((c) => c.runtime === runtime && c.available);
}

function activeRuns(m: MachineView): number {
  return Math.max(m.health?.activeRuns ?? 0, m.runningRuns.length);
}

/** Machines that can take a new run of this bot, best first. */
export function candidates(
  machines: MachineView[],
  runner: Pick<BotRunner, "labels" | "runtime" | "machineId">,
  now: number,
  exclude: string[] = [],
): MachineView[] {
  return machines
    .filter(
      (m) =>
        !exclude.includes(m.id) &&
        isHealthy(m, now) &&
        // A runner pinned to one machine (1.2.2) runs only there; its labels do not count.
        (runner.machineId ? m.id === runner.machineId : labelsMatch(runner.labels, m.labels)) &&
        hasRuntime(m, runner.runtime) &&
        activeRuns(m) < Math.max(1, m.info?.maxParallelRuns ?? 1),
    )
    .sort((a, b) => activeRuns(a) - activeRuns(b) || (a.health?.load1 ?? 0) - (b.health?.load1 ?? 0) || a.id.localeCompare(b.id));
}

export function pickMachine(
  machines: MachineView[],
  runner: Pick<BotRunner, "labels" | "runtime" | "machineId">,
  now: number,
  exclude: string[] = [],
): MachineView | null {
  return candidates(machines, runner, now, exclude)[0] ?? null;
}

export function toMachine(m: MachineView, createdBy: string, createdAt: number, now: number): Machine {
  const out: Machine = {
    id: m.id,
    name: m.name,
    labels: m.labels,
    createdBy,
    createdAt,
    connected: m.connected,
    healthy: isHealthy(m, now),
    runningRuns: m.runningRuns,
  };
  if (m.revoked) out.revoked = true;
  if (m.lastHeartbeatAt !== undefined) out.lastHeartbeatAt = m.lastHeartbeatAt;
  if (m.info) {
    out.daemonVersion = m.info.daemonVersion;
    out.os = m.info.os;
    out.arch = m.info.arch;
    out.maxParallelRuns = m.info.maxParallelRuns;
    if (m.info.features) out.features = m.info.features;
    if (m.info.takeoverUrl) out.takeoverUrl = m.info.takeoverUrl;
  }
  if (m.health) out.health = m.health;
  if (m.clis) out.clis = m.clis;
  return out;
}

/**
 * A CLI error as people read it: the message inside a JSON error body (the raw body stays in the
 * run log), and a plain Chinese line for errors people can fix themselves.
 */
export function readableError(raw: string): string {
  const t = raw.trim();
  if (!t.startsWith("{")) return t;
  try {
    const j = JSON.parse(t) as { message?: unknown; error?: unknown };
    const e = (typeof j.error === "object" && j.error !== null ? j.error : j) as { message?: unknown; code?: unknown };
    const msg = typeof e.message === "string" ? e.message : typeof j.error === "string" ? j.error : "";
    if (e.code === "model_not_found") {
      const model = /'([^']+)'/.exec(msg)?.[1];
      return `这台电脑上的账号用不了模型 ${model ?? ""}，换个模型再试`.replace("模型 ，", "这个模型，");
    }
    return msg || t;
  } catch {
    return t;
  }
}

/** "同意" / "拒绝" (and a few common forms) answer a pending approval; anything else is input. */
export function approvalDecision(body: string): "approve" | "deny" | null {
  const t = body
    .replace(/@\S+/g, "")
    .trim()
    .toLowerCase()
    .replace(/[。.!！,，\s]+$/u, "");
  if (["同意", "可以", "批准", "好", "好的", "行", "approve", "yes", "y", "ok"].includes(t)) return "approve";
  if (["拒绝", "不同意", "不行", "不可以", "不要", "deny", "no", "n"].includes(t)) return "deny";
  return null;
}

/** "这件事里都同意" (and "都同意") answers a gated approval for the whole piece of work. */
export function approvesWholeWork(body: string): boolean {
  const t = body
    .replace(/@\S+/g, "")
    .trim()
    .replace(/[。.!！,，\s]+$/u, "");
  return t === TEXT.approveRun || t === "都同意";
}

/** Cuts text to at most `max` code points. */
export function cut(text: string, max: number): string {
  const cps = [...text];
  return cps.length <= max ? text : `${cps.slice(0, Math.max(0, max - 1)).join("")}…`;
}

export function firstLine(text: string, max = 280): string {
  const line = text.split(/\r?\n/).find((l) => l.trim() !== "") ?? "";
  return cut(line.trim(), max);
}

export interface ContextMessage {
  id?: string;
  authorId: string;
  body: string;
}

/**
 * How a CLI reports its 验收标准 (machine-protocol.md, "Acceptance criteria"): the daemon reads
 * the last fenced `acceptance` block of an assistant message. Limits as in the protocol.
 */
export const ACCEPTANCE_RULE = [
  "When the request is a piece of work with outcomes someone can check (code, a deploy, a file, a 产出), report its acceptance criteria and their state as a fenced block with the info string `acceptance` at the end of your message. Skip the block for questions, chat and short answers. The block is taken out of your posted answer and shown as a checklist under it, so never refer to it in your text. Example:",
  "```acceptance",
  '[{ "text": "Tests pass", "state": "pass", "evidence": { "text": "cargo test: 42 passed" } }, { "text": "Deployed to staging", "state": "pending" }]',
  "```",
  "Each item has `text` (1-500 characters), `state` (`pending`, `pass` or `fail`) and optional `evidence` (`text` up to 1000 characters, `url`, `fileId`, or `artifact` { artifactId, version }). At most 20 items. Send the whole list again when a state changes: the last block counts.",
].join("\n");

/**
 * The text without its fenced `acceptance` blocks (the daemon already sent them as the run's
 * acceptance criteria, which the done card shows). An unclosed fence stays as it is.
 */
export function stripAcceptance(text: string): string {
  const lines = text.split("\n");
  const out: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const m = /^\s*(`{3,})\s*acceptance\s*$/.exec(lines[i]);
    if (m) {
      const fence = m[1];
      const end = lines.findIndex((l, j) => j > i && l.trim() === fence);
      if (end !== -1) {
        i = end;
        continue;
      }
    }
    out.push(lines[i]);
  }
  return out.join("\n").replace(/\n{3,}/g, "\n\n").trim();
}

/** The full prompt for the CLI: who it is, the topic's recent messages, and the request. */
export function buildPrompt(opts: {
  botName: string;
  title: string;
  messages: ContextMessage[];
  names: Map<string, string>;
  request: ContextMessage;
  followUps?: ContextMessage[];
  /**
   * 1.1 (引用提问): the request's references section (`refsText`). The request body is then the
   * question without the server's quote lines.
   */
  refs?: string;
  /** Settings: the team's approval rules for this bot (settings/rules.ts `policyPrompt`). */
  rules?: string;
  /** The daemon reads acceptance blocks (feature "acceptance"): the prompt says how to write them. */
  acceptance?: boolean;
  /** Topic lines (context/render.ts); replaces `messages`. */
  context?: string;
  /** The machine answers chat-run context ops (feature "context"). */
  contextTools?: boolean;
  /** The front desk's work order (agent/frontdesk.ts `briefText`). */
  brief?: string;
  /** 沉淀: the memory index (memory/inject.ts), reference material. */
  memory?: string;
}): string {
  const name = (id: string) => opts.names.get(id) ?? id;
  const lines = [
    `You are ${opts.botName}, a member of a team chat. You were asked to work in the topic "${opts.title}".`,
    "Your final answer is posted into the topic as is, so keep it short and in the language of the request.",
    "People also watch what you say while you work: write those lines in the language of the request too.",
    ...(opts.acceptance ? [ACCEPTANCE_RULE] : []),
    ...(opts.refs ? [REFS_RULE] : []),
    ...(opts.rules ? [opts.rules] : []),
    ...(opts.memory ? ["", opts.memory] : []),
    "",
    ...(opts.context !== undefined
      ? [
          "The topic, oldest first. Each message is `[#seq m id · author · time]`; files, 产出, quotes and cards follow it on indented lines:",
          opts.context,
          ...(opts.contextTools ? ["", CONTEXT_TOOLS_RULE] : []),
        ]
      : ["Recent messages of the topic, oldest first:", ...opts.messages.map((m) => `${name(m.authorId)}: ${m.body}`)]),
    "",
    `Request from ${name(opts.request.authorId)}:`,
    opts.request.body,
  ];
  if (opts.brief) lines.push("", opts.brief);
  if (opts.refs) lines.push("", opts.refs);
  for (const f of opts.followUps ?? []) lines.push("", `Follow-up from ${name(f.authorId)}:`, f.body);
  return lines.join("\n");
}

/**
 * The prompt of a run that continues the CLI session of the topic's last run: the session has
 * the rest, so only what is new since then, and the request.
 */
export function buildResumePrompt(opts: {
  names: Map<string, string>;
  /** New topic lines since the last run (context/render.ts), or "" when there are none. */
  context: string;
  request: ContextMessage;
  followUps?: ContextMessage[];
  refs?: string;
  contextTools?: boolean;
  brief?: string;
}): string {
  const name = (id: string) => opts.names.get(id) ?? id;
  const lines: string[] = [];
  if (opts.context !== "") lines.push("New in the topic since your last turn, oldest first:", opts.context, "");
  lines.push(`Request from ${name(opts.request.authorId)}:`, opts.request.body);
  if (opts.brief) lines.push("", opts.brief);
  if (opts.refs) lines.push("", REFS_RULE, "", opts.refs);
  for (const f of opts.followUps ?? []) lines.push("", `Follow-up from ${name(f.authorId)}:`, f.body);
  if (opts.contextTools) lines.push("", CONTEXT_TOOLS_RULE);
  return lines.join("\n");
}

/**
 * The prompt of a side chat run (单独问): read-only, private to the asker, about a topic of
 * another group read up to the asker's point. The asker's DM messages of the side chat come
 * last, then the question.
 */
export function buildAsidePrompt(opts: {
  botName: string;
  source: { title: string; messages: ContextMessage[] };
  chat: ContextMessage[];
  names: Map<string, string>;
  request: ContextMessage;
  followUps?: ContextMessage[];
}): string {
  const name = (id: string) => opts.names.get(id) ?? id;
  const lines = [
    `You are ${opts.botName}, a member of a team chat. ${name(opts.request.authorId)} asks you privately about the topic "${opts.source.title}".`,
    "This is a side conversation: only the asker reads your answer. Nothing you write is posted to the topic.",
    "Read only: do not change files, run commands that change anything, or contact anyone.",
    "Answer short and in the language of the question. When you suggest a message for the topic, write it as a quote the asker can send.",
    "",
    "Messages of the topic up to the asker's point, oldest first:",
    ...opts.source.messages.map((m) => `${name(m.authorId)}: ${m.body}`),
  ];
  if (opts.chat.length > 0) {
    lines.push("", "This side conversation so far, oldest first:", ...opts.chat.map((m) => `${name(m.authorId)}: ${m.body}`));
  }
  lines.push("", `Question from ${name(opts.request.authorId)}:`, opts.request.body);
  for (const f of opts.followUps ?? []) lines.push("", `Follow-up from ${name(f.authorId)}:`, f.body);
  return lines.join("\n");
}

/**
 * Progress text as one append-only stream. Deltas append; the final text of an item replaces
 * its deltas, so only the part the deltas missed is appended (gaps happen when the daemon drops
 * deltas offline). Tool calls show as one short line.
 */
export class Progress {
  constructor(
    public text: string,
    public items: Record<string, string>,
    public full: boolean,
  ) {}
  private pending = "";

  private push(s: string): void {
    if (this.full || s === "") return;
    if ([...this.text].length + [...s].length > MAX_PROGRESS_CHARS) {
      this.full = true;
      s = "\n…";
    }
    this.text += s;
    this.pending += s;
  }

  delta(itemId: string, text: string): void {
    this.items[itemId] = (this.items[itemId] ?? "") + text;
    this.push(text);
  }

  message(itemId: string, text: string): void {
    const had = this.items[itemId] ?? "";
    this.items[itemId] = text;
    if (text.startsWith(had)) this.push(text.slice(had.length));
    else this.push(`\n${text}`);
    this.push("\n\n");
    // The item is complete; its text is no longer needed.
    delete this.items[itemId];
  }

  tool(title: string): void {
    const sep = this.text === "" || this.text.endsWith("\n") ? "" : "\n";
    this.push(`${sep}· ${cut(title.replace(/\s+/g, " ").trim(), 200)}\n`);
  }

  /** Text appended since the last call. */
  take(): string {
    const p = this.pending;
    this.pending = "";
    return p;
  }
}
