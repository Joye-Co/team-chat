// 盯活 (docs/design/agent-harness.zh.md 3.4): the rules the desk applies by code while a run
// works. Pure, so they are easy to test: what counts as a nudge, a loop, a promise without work,
// a progress question, and the progress answer.
import { cut } from "../bots/dispatch";

/** A run silent this long gets one nudge (run.input). */
export const NUDGE_MS = 5 * 60_000;
/** The same tool call this many times in a row is a loop: one reminder. */
export const LOOP_LIMIT = 4;
/** The desk fixes a run by itself at most this often (nudge, loop reminder, restart, continue); then it tells people once. */
export const MAX_INTERVENTIONS = 3;
/** A run whose machine has sent no heartbeat for this long is restarted elsewhere. */
export const MACHINE_GONE_MS = 2 * 60_000;
/** Runs in a row that the desk continued because the CLI only promised to work. */
export const MAX_AUTO_CONTINUE = 3;

/** What the CLI reads when the desk nudges it (developer words are fine: the CLI reads them). */
export const NUDGE_TEXT =
  "[team-chat] No progress for 5 minutes. Say in one line what you are doing or where you are stuck, then go on. If the current approach does not work, try another one.";
export const LOOP_TEXT = (title: string) =>
  `[team-chat] You ran the same step ${LOOP_LIMIT} times in a row (${cut(title, 120)}). It is not getting anywhere: stop repeating it, find out why, and try a different approach.`;
export const CONTINUE_TEXT =
  "[team-chat] You said you would do it but ended without doing it. Do the work now with your tools, then report the result.";

/** A tool call's identity for loop detection: its title without volatile numbers and spacing. */
export function loopKey(title: string): string {
  return title.replace(/\s+/g, " ").trim().slice(0, 300);
}

const PROMISE = /^(好的?|收到|没问题|ok(ay)?)?[，,。.!！\s]*(我(来|去|先|会|马上|这就)|让我|稍等|马上|这就|i'?ll|i will|let me|on it|working on it|looking into)/i;

/** A result that only promises work (and the run called no tool): the desk continues it. */
export function isPromiseOnly(result: string, toolCalls: number): boolean {
  const t = result.trim();
  return toolCalls === 0 && t.length > 0 && [...t].length <= 200 && PROMISE.test(t);
}

const PROGRESS = /(进展|进度(如何|怎样|怎么样|呢|[?？]|$)|怎么样了|到哪(一步|了)|好了(没|吗)|完成了(吗|没)|做完了(吗|没)|还要多久|status\b|progress|eta\b|how('s| is) it going)/i;

/** A short question about the run's progress: the desk answers from the run, not the CLI. */
export function isProgressQuestion(body: string): boolean {
  const t = body.replace(/@\S+/g, "").trim();
  return [...t].length > 0 && [...t].length <= 30 && PROGRESS.test(t);
}

export interface ProgressState {
  startedAt: number | null;
  now: number;
  plan: { text: string; state: string }[];
  lastTool?: string;
  status: string;
}

/** The desk's answer to a progress question, from the run's own state. */
export function progressAnswer(s: ProgressState): string {
  const mins = s.startedAt !== null ? Math.max(1, Math.round((s.now - s.startedAt) / 60_000)) : 0;
  const lines = [s.status === "blocked" ? `卡住了，已经做了 ${mins} 分钟，我在处理。` : mins > 0 ? `还在做，已经 ${mins} 分钟。` : "刚开始做。"];
  if (s.plan.length > 0) {
    const done = s.plan.filter((p) => p.state === "done").length;
    const current = s.plan.find((p) => p.state === "current");
    lines.push(`计划 ${done}/${s.plan.length} 完成${current ? `，正在：${cut(current.text, 80)}` : ""}。`);
  }
  if (s.lastTool) lines.push(`最近一步：${cut(s.lastTool, 80)}`);
  return lines.join("\n");
}
