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
/**
 * The default time limit of one run (盯 1.6): work time, without waits. The team can change it
 * (`/v1/settings/runs`); at the limit the run saves its state and goes on in a new turn.
 */
export const MAX_RUN_MS = 2 * 60 * 60_000;

/** "45 分钟", "2 小时", "2.5 小时": how long a run worked, for topic notes. */
export function workTime(minutes: number): string {
  if (minutes < 120) return `${Math.max(1, Math.round(minutes))} 分钟`;
  return `${(Math.round((minutes / 60) * 10) / 10).toString()} 小时`;
}
/** Longest progress message body we stream (the message body limit is 32,768). */
export const MAX_PROGRESS_CHARS = 30_000;
/** Longest result message body. */
export const MAX_RESULT_CHARS = 30_000;
/** Messages of the topic given to the CLI as context (side chats and the `messages` list). */
export const CONTEXT_MESSAGES = 30;
/** Characters of topic lines in a run's prompt (docs/protocol.md, "Run context"). */
export const CONTEXT_CHARS = 24_000;
/** 模 3.4: a run that waits for a subscription limit tries a little past the reset, so it does not race the provider's clock. */
export const LIMIT_SLACK_MS = 60_000;

/** How a run with the `context` feature reads what the prompt left out. */
export const CONTEXT_TOOLS_RULE =
  "The topic lines above are only part of the topic. Read older messages with `chat-run topic history --before <seq>`, a whole message with `chat-run topic message --id <m id>` (a quote `in t <t id>` is in another topic: add `--topic <t id>`), search with `chat-run search --query <text>` (add `--group` for every topic of the group) and get a file with `chat-run file get --id <f id>`. Look things up instead of guessing.";

/** Topic texts the bot posts. Product words only (PRODUCT.md): no developer words. */
export const TEXT = {
  /** No computer is set up for this member at all (机 2.10: each reason has its own text). */
  noMachine: "现在没有能接手的电脑，给我配好电脑后再 @我。",
  noRuntime: "没有电脑装了我用的运行方式，这次没有开始。装好后再 @我。",
  noDisk: "电脑的磁盘空间不够，这次没有开始。清出空间后再 @我。",
  /** The computers that could take it are busy (a computer turned the run down). */
  allBusy: "能接手的电脑都在忙，这次没有开始。请稍后再 @我。",
  /** The member's computers are offline: the run waits (排队) and starts when one comes online. */
  waitOffline: (names: string[]) => `${names.join("、")} 不在线，等它上线后自动开始；不需要了可以停止。`,
  /**
   * 模 3.4, 盯 1.5: the computer's subscription is at its limit: the run waits for the reset (the
   * team's clock) and starts, or goes on (`goesOn`), by itself. Without a reset time it tries
   * again after `minutes`.
   */
  waitLimit: (cli: string, when: string, goesOn: boolean) => `${cli} 的订阅额度用完了，${when} 恢复后${goesOn ? "接着做" : "开始"}；不需要了可以停止。`,
  waitLimitRetry: (cli: string, minutes: number) => `${cli} 的订阅额度用完了，不知道几点恢复，${minutes} 分钟后再试；不需要了可以停止。`,
  /** The wait for the limit is over: the run starts on the computer again. */
  limitBack: (goesOn: boolean) => `额度恢复了，${goesOn ? "接着做" : "开始做"}。`,
  /** A queued run that waited too long, by the reason it waited. */
  queueExpired: (hours: number, offline?: string[]) =>
    offline && offline.length > 0
      ? `等了 ${hours} 小时，${offline.join("、")} 还是没上线，这次没有开始。需要的话请再 @我。`
      : `排了 ${hours} 小时还没轮到空闲的电脑，这次没有开始。需要的话请再 @我。`,
  /** The run's computer went away: the run waits for it (盯 1.1). */
  machineAway: (name: string) => `${name} 不在线了，等它回来接着做；不需要了可以停止。`,
  machineBack: (name: string) => `${name} 回来了，接着做。`,
  machineMoved: (name: string, minutes: number) => `${name} ${minutes} 分钟没回来，换一台电脑接着做。`,
  /** 盯 1.7: the computer's program restarted and the run could not be continued. */
  interrupted: "电脑上的程序重启了，这次没能接着做。需要的话请再 @我。",
  /** The computer is back but no longer has the run, or it was removed. */
  lost: "卡住了：电脑上的这次处理中断了，没有完成。需要的话请再 @我。",
  stalled: (minutes: number) => `卡住了：${minutes} 分钟没有进展。`,
  /** After the desk's own tries (盯活), named as they happened: told once. */
  gaveUp: (minutes: number, tries: string[]) =>
    tries.length > 0 ? `卡住了：${minutes} 分钟没有进展，我自己试过（${tries.join("、")}）还是不行，需要你看一下。` : `卡住了：${minutes} 分钟没有进展，需要你看一下。`,
  /** 盯 1.6: turns at the time limit made no progress; the work stops and people are told once. */
  limitStopped: (worked: string, turns: number) => `已经做了 ${worked}，最近 ${turns} 轮都没有新进展，我先停下了，需要你看一下。`,
  /** 盯 1.6: the time limit came and the work could not go on (no computer to continue on). */
  timeUp: (worked: string) => `已经做了 ${worked}，到了单次时限，这次先停下了。需要的话请再 @我。`,
  progress: "进行中…",
  done: "完成了。",
  failed: (why: string) => (why ? `没有完成：${why}` : "没有完成。"),
  cancelled: "已停止，没有完成。",
  /** design-relay R2: the summary of a run that handed its work over (转交). */
  handedOver: (name: string) => `已转交给 ${name}`,
  followUpLimit: "后面的消息没有接着处理，需要的话请再 @我。",
  approval: (title: string, detail?: string) =>
    `需要你确认：${title}${detail ? `\n\n${detail}` : ""}\n\n回复「同意」或「拒绝」。`,
  approve: "同意",
  deny: "拒绝",
  /** A gated command (feature "gates"): the kind, the command, and the third answer. */
  approvalGate: (kind: string, command?: string) =>
    `需要你确认：${kind}${command ? `\n\n${command}` : ""}\n\n回复「同意」「拒绝」或「这件事里都同意」。`,
  approveRun: "这件事里都同意",
  /** The kind's approver is the owners, and the group has none to ask: the requester is asked. */
  approvalNoOwner: "这个群里没有管理员，先请你确认。",
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
  /**
   * Why a run failed (模 2.2, 接 5.3, 盯 1.13): one sentence per kind (`run.done.failure`) and one
   * next step. The CLI's own words stay in the run log for the work record.
   */
  failure: {
    signedOut: (machine: string, cli: string) => `${machine} 上的 ${cli} 没有登录，或者登录已经过期`,
    limit: (cli: string) => `${cli} 的订阅额度用完了`,
    model: (cli: string) => `${cli} 用不了现在选的模型`,
    network: (machine: string) => `${machine} 连不上模型服务（网络不通，或者服务暂时不可用）`,
    notInstalled: (machine: string, cli: string) => `${machine} 上没有装 ${cli}`,
    other: "运行时出错了，原因记在工作记录里",
  },
  /** The next step after the sentence. */
  next: {
    /**
     * The requester manages the computers and may open 团队管理 (a 负责人, 模 3.5): its sign-in
     * is in 账号与密钥 › 订阅账号, with the computer chosen there.
     */
    signInSelf: (machines: string[]) =>
      machines.length === 1
        ? `请到 团队管理 › 账号与密钥 › 订阅账号，选 ${machines[0]}，点「重新登录」，然后再 @我。`
        : `请到 团队管理 › 账号与密钥 › 订阅账号，给 ${machines.join("、")} 重新登录，然后再 @我。`,
    /** The requester owns the personal computers but cannot open 团队管理: sign in there. */
    signInOnMachine: (machines: string[], cli: string) =>
      `请在 ${machines.join("、")} 上重新登录 ${cli}，或者请负责人在 团队管理 › 账号与密钥 › 订阅账号 里帮忙，然后再 @我。`,
    /** Someone else does: they are mentioned. */
    signInAsk: (names: string[]) =>
      `请 ${names.join("、")} 重新登录（团队管理 › 账号与密钥 › 订阅账号，或者在那台电脑上），然后再 @我。`,
    signInAdmin: "请负责人到 团队管理 › 账号与密钥 › 订阅账号 重新登录，然后再 @我。",
    limitAt: (when: string) => `额度在 ${when} 恢复，到时再 @我。`,
    limitLater: "额度恢复后再 @我。",
    /** The AI member's own model (运行设置) is the one that failed. */
    modelOwn: "请到 设置 › 成员 里我的 运行设置 › 模型 换一个模型，然后再 @我。",
    /** The shared model (设置 › 通用 › 模型, only 负责人 change it). */
    model: "请负责人到 设置 › 通用 › 模型 换一个模型，然后再 @我。",
    network: "请稍后再 @我；一直这样的话，看看那台电脑的网络。",
    notInstalled: "装好后再 @我。",
    other: "需要的话再 @我 试一次。",
  },
  /** A run that failed: the sentence and the next step. */
  failedWith: (sentence: string, next: string) => `没有完成：${sentence}。${next}`,
  /** No run started because the computers are not signed in. */
  notStartedWith: (sentence: string, next: string) => `这次没有开始：${sentence}。${next}`,
} as const;

/** `RunFailure.kind` values the server words (machine-protocol.md, "Failure kinds"). */
export type FailureKind = "signed_out" | "limit" | "model" | "network" | "not_installed" | "other";

/**
 * The kind of a failed run: the daemon's (`run.done.failure.kind`; an unknown one is `other`),
 * else, for a daemon before failure kinds, `model` for a JSON `model_not_found` body (#103).
 */
export function failureKind(failure: { kind?: unknown } | undefined, raw: string): FailureKind {
  const k = failure?.kind;
  if (typeof k === "string") {
    return (["signed_out", "limit", "model", "network", "not_installed"] as const).find((x) => x === k) ?? "other";
  }
  return /"code"\s*:\s*"model_not_found"/.test(raw) ? "model" : "other";
}

/** How people know a runtime: its CLI's name for the kinds the product knows, else the runtime. */
export function cliName(kind: string | undefined, runtime: string): string {
  if (kind === "claude") return "Claude Code";
  if (kind === "codex") return "Codex";
  return runtime;
}

/**
 * A runtime entry as the computer's 订阅账号 page shows it (verify 2, problem 8): the CLI's name,
 * and the entry's own name when it is not the kind's (`Codex（codexout）`), so two entries of
 * one CLI on a computer are told apart.
 */
export function cliEntryName(kind: string | undefined, runtime: string): string {
  const cli = cliName(kind, runtime);
  return (kind === "claude" || kind === "codex") && runtime !== kind ? `${cli}（${runtime}）` : cli;
}

/** `11 月 13 日 15:00` (`今天 15:00` on the same day) on the team's clock `zone`. */
export function clockText(at: number, now: number, zone: string): string {
  const parts = (t: number) => {
    try {
      return Object.fromEntries(
        new Intl.DateTimeFormat("en-CA", { timeZone: zone, year: "numeric", month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" })
          .formatToParts(new Date(t))
          .map((x) => [x.type, x.value]),
      ) as Record<string, string>;
    } catch {
      return null;
    }
  };
  const p = parts(at);
  const n = parts(now);
  if (!p) return new Date(at).toISOString();
  const time = `${p.hour}:${p.minute}`;
  if (n && n.year === p.year && n.month === p.month && n.day === p.day) return `今天 ${time}`;
  return `${p.month} 月 ${p.day} 日 ${time}`;
}

/**
 * Who may sign a computer's CLI in again (模 3.5): the requester from 团队管理 (`admin`), the
 * requester on their own personal computers (`machine`: they cannot open 团队管理), or others.
 */
export type SignInBy = { self: "admin" | "machine" } | { names: string[] };

/** The sentence and next step of a failure kind. */
export function failureWords(
  kind: FailureKind,
  ctx: { machines: string[]; cli: string; resetsAt?: number; now: number; zone: string; signIn: SignInBy; ownModel?: boolean },
): { sentence: string; next: string } {
  const f = TEXT.failure;
  const n = TEXT.next;
  const machine = ctx.machines.length > 0 ? ctx.machines.join("、") : "电脑";
  switch (kind) {
    case "signed_out":
      return {
        sentence: f.signedOut(machine, ctx.cli),
        next:
          "self" in ctx.signIn
            ? ctx.signIn.self === "admin"
              ? n.signInSelf(ctx.machines)
              : n.signInOnMachine(ctx.machines, ctx.cli)
            : ctx.signIn.names.length > 0
              ? n.signInAsk(ctx.signIn.names)
              : n.signInAdmin,
      };
    case "limit":
      return {
        sentence: f.limit(ctx.cli),
        next: ctx.resetsAt && ctx.resetsAt > ctx.now ? n.limitAt(clockText(ctx.resetsAt, ctx.now, ctx.zone)) : n.limitLater,
      };
    case "model":
      return { sentence: f.model(ctx.cli), next: ctx.ownModel ? n.modelOwn : n.model };
    case "network":
      return { sentence: f.network(machine), next: n.network };
    case "not_installed":
      return { sentence: f.notInstalled(machine, ctx.cli), next: n.notInstalled };
    default:
      return { sentence: f.other, next: n.other };
  }
}

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

/** Kinds that continue a session and have a read-only mode when a daemon does not say (`CliInfo.resume` / `aside` absent). */
const SESSION_KINDS: readonly string[] = ["claude", "codex"];

function cliOf(m: Pick<MachineView, "clis">, runtime: string): CliInfo | undefined {
  return (m.clis ?? []).find((c) => c.runtime === runtime);
}

/**
 * Runs of this runtime on the machine continue `resumeSessionId` (接 3.6): feature "resume" and
 * the CLI's `resume`; a daemon that does not report it continues only Claude Code and Codex.
 */
export function resumesRuntime(m: Pick<MachineView, "info" | "clis">, runtime: string): boolean {
  const cli = cliOf(m, runtime);
  return hasFeature(m, "resume") && cli !== undefined && (cli.resume ?? SESSION_KINDS.includes(cli.kind));
}

/** The machine runs a side chat (单独问) of this runtime read-only: feature "aside" and the CLI's `aside` (absent: Claude Code and Codex). */
export function asideRuntime(m: Pick<MachineView, "info" | "clis">, runtime: string): boolean {
  const cli = cliOf(m, runtime);
  return hasFeature(m, "aside") && cli !== undefined && (cli.aside ?? SESSION_KINDS.includes(cli.kind));
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
  /** Usable: the person whose own machine this is; absent = a team machine. */
  ownerId?: string;
  /** Usable: the owner's checked AI members on a personal machine (bot id -> repo path). */
  personalBots?: Map<string, string | null>;
  /** 模 3.1: runtimes whose CLI is not signed in (`MachineLink.status`); they take no runs. */
  signedOut?: string[];
  /** 模 3.4: runtimes at their subscription limit (`MachineLink.status`); they take no runs until then. */
  limited?: { runtime: string; until: number; known: boolean }[];
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

/** 模 3.1, 机 2.5: the machine's CLI for this runtime is signed in, or nothing says it is not. */
export function signedIn(m: Pick<MachineView, "signedOut">, runtime: string): boolean {
  return !(m.signedOut ?? []).includes(runtime);
}

/** 模 3.4: the machine's sign-in for this runtime is at its limit now: when it resets, else null. */
export function limitOf(m: Pick<MachineView, "limited">, runtime: string, now: number): { until: number; known: boolean } | null {
  const l = (m.limited ?? []).find((x) => x.runtime === runtime && x.until > now);
  return l ? { until: l.until, known: l.known } : null;
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
        signedIn(m, runner.runtime) &&
        limitOf(m, runner.runtime, now) === null &&
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

/** Connected with a recent heartbeat (free disk aside). */
export function isOnline(m: MachineView, now: number): boolean {
  return !m.revoked && m.connected && m.lastHeartbeatAt !== undefined && now - m.lastHeartbeatAt <= HEARTBEAT_STALE_MS;
}

/** Why no machine takes a run right now (机 2.10). `busy` and `offline` wait; the rest fail. */
export type NoMachine =
  | { why: "none" }
  | { why: "runtime" }
  | { why: "disk" }
  | { why: "busy" }
  | { why: "offline"; names: string[] }
  /** 模 3.1: every computer with the runtime has its CLI signed out (fails, the topic says which). */
  | { why: "signed_out"; machines: MachineView[] }
  /**
   * 模 3.4: every signed-in computer with the runtime is at its subscription limit: the run
   * waits (排队) until the first one resets (`until`; `known`: a real reset time, else a retry).
   */
  | { why: "limit"; machines: MachineView[]; until: number; known: boolean };

export function whyNoMachine(machines: MachineView[], runner: Pick<BotRunner, "labels" | "runtime" | "machineId">, now: number): NoMachine {
  const mine = machines.filter((m) => !m.revoked && (runner.machineId ? m.id === runner.machineId : labelsMatch(runner.labels, m.labels)));
  // A machine that never said hello is not set up yet.
  const known = mine.filter((m) => m.clis !== undefined);
  if (known.length === 0) return { why: "none" };
  const installed = known.filter((m) => hasRuntime(m, runner.runtime));
  if (installed.length === 0) return { why: "runtime" };
  const able = installed.filter((m) => signedIn(m, runner.runtime));
  if (able.length === 0) return { why: "signed_out", machines: installed };
  // A computer at its limit waits for the reset; one free of it that is only busy comes first.
  const free = able.filter((m) => limitOf(m, runner.runtime, now) === null);
  if (free.some((m) => isHealthy(m, now))) return { why: "busy" };
  const limited = able.filter((m) => limitOf(m, runner.runtime, now) !== null);
  if (limited.length > 0) {
    const first = limited.map((m) => limitOf(m, runner.runtime, now) as { until: number; known: boolean }).sort((a, b) => a.until - b.until)[0];
    return { why: "limit", machines: limited, until: first.until, known: first.known };
  }
  const away = free.filter((m) => !isOnline(m, now));
  if (away.length > 0) return { why: "offline", names: away.map((m) => m.name) };
  return { why: "disk" };
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
  if (m.ownerId) out.ownerId = m.ownerId;
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

/**
 * "同意" / "拒绝" answer a pending approval; anything else is input. A plain "好" or "ok" said to
 * someone else must not approve a merge, so the common short forms ("好", "可以", "行", "ok", …)
 * count only in a reply that quotes the approval card itself (`toCard`).
 */
export function approvalDecision(body: string, toCard = false): "approve" | "deny" | null {
  const t = body
    .replace(/@\S+/g, "")
    .trim()
    .toLowerCase()
    .replace(/[。.!！,，\s]+$/u, "");
  if (t === "同意") return "approve";
  if (t === "拒绝" || t === "不同意") return "deny";
  if (!toCard) return null;
  if (["可以", "批准", "好", "好的", "行", "approve", "yes", "y", "ok"].includes(t)) return "approve";
  if (["不行", "不可以", "不要", "deny", "no", "n"].includes(t)) return "deny";
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
  /** The group's members with 岗位 (context/read.ts `membersText`). */
  members?: string;
}): string {
  const name = (id: string) => opts.names.get(id) ?? id;
  const lines = [
    `You are ${opts.botName}, a member of a team chat. You were asked to work in the topic "${opts.title}".`,
    "Your final answer is posted into the topic as is, so keep it short and in the language of the request.",
    "People also watch what you say while you work: write those lines in the language of the request too.",
    ...(opts.acceptance ? [ACCEPTANCE_RULE] : []),
    ...(opts.refs ? [REFS_RULE] : []),
    ...(opts.rules ? [opts.rules] : []),
    ...(opts.members ? ["", opts.members] : []),
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
