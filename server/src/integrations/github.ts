// GitHub adapter, pure part: payload parsing, the per-topic state reducer, the status card and
// the topic status. The gateway part (deliver.ts) stores the state and talks to the room.
import { Card, MAX_TITLE_CHARS, type TopicStatus } from "@chat/protocol";
import { z } from "zod";

// ---------------------------------------------------------------------------
// Payloads (only the fields we use; GitHub sends many more)
// ---------------------------------------------------------------------------

const User = z.object({ login: z.string() });
const Repo = z.object({ full_name: z.string().min(1), html_url: z.string().optional() });
const PrRef = z.object({ number: z.int().positive() });

const PullRequest = z.object({
  number: z.int().positive(),
  title: z.string(),
  html_url: z.string(),
  user: User.nullable().optional(),
  draft: z.boolean().optional(),
  merged: z.boolean().nullable().optional(),
  state: z.enum(["open", "closed"]),
  body: z.string().nullable().optional(),
  head: z.object({ sha: z.string(), ref: z.string() }),
  base: z.object({ ref: z.string() }),
  requested_reviewers: z.array(z.object({ login: z.string().optional() })).optional(),
});

const PullRequestEvent = z.object({ action: z.string(), pull_request: PullRequest, repository: Repo });
const ReviewEvent = z.object({
  action: z.string(),
  review: z.object({ user: User.nullable().optional(), state: z.string() }),
  pull_request: PullRequest,
  repository: Repo,
});
const CheckSuiteEvent = z.object({
  action: z.string(),
  check_suite: z.object({
    head_sha: z.string(),
    head_branch: z.string().nullable().optional(),
    conclusion: z.string().nullable().optional(),
    app: z.object({ name: z.string() }).nullable().optional(),
    pull_requests: z.array(PrRef).optional(),
  }),
  repository: Repo,
});
const WorkflowRunEvent = z.object({
  action: z.string(),
  workflow_run: z.object({
    name: z.string().nullable().optional(),
    head_sha: z.string(),
    head_branch: z.string().nullable().optional(),
    conclusion: z.string().nullable().optional(),
    html_url: z.string().optional(),
    pull_requests: z.array(PrRef).optional(),
  }),
  repository: Repo,
});
const IssuesEvent = z.object({
  action: z.string(),
  issue: z.object({
    number: z.int().positive(),
    title: z.string(),
    html_url: z.string(),
    user: User.nullable().optional(),
    state: z.enum(["open", "closed"]),
    body: z.string().nullable().optional(),
  }),
  repository: Repo,
});

export interface PrInfo {
  title: string;
  url: string;
  author: string | null;
  draft: boolean;
  state: "open" | "closed" | "merged";
  body: string;
  headSha: string;
  headRef: string;
  baseRef: string;
  requested: string[];
}

/** A GitHub delivery reduced to what the adapter acts on. */
export type GhEvent =
  | { type: "pr"; repo: string; number: number; action: string; pr: PrInfo; review?: { login: string; state: string } }
  | {
      type: "issue";
      repo: string;
      number: number;
      action: string;
      issue: { title: string; url: string; author: string | null; state: "open" | "closed"; body: string };
    }
  | {
      type: "ci";
      repo: string;
      repoUrl: string;
      name: string;
      sha: string;
      branch: string | null;
      conclusion: string;
      url: string | null;
      prNumbers: number[];
    };

const PR_ACTIONS = new Set([
  "opened",
  "reopened",
  "edited",
  "ready_for_review",
  "converted_to_draft",
  "review_requested",
  "review_request_removed",
  "synchronize",
  "closed",
]);

function prInfo(p: z.infer<typeof PullRequest>): PrInfo {
  return {
    title: p.title,
    url: p.html_url,
    author: p.user?.login ?? null,
    draft: p.draft === true,
    state: p.state === "closed" ? (p.merged ? "merged" : "closed") : "open",
    body: p.body ?? "",
    headSha: p.head.sha,
    headRef: p.head.ref,
    baseRef: p.base.ref,
    requested: (p.requested_reviewers ?? []).map((r) => r.login).filter((l): l is string => !!l),
  };
}

export class PayloadError extends Error {}

function parse<S extends z.ZodType>(schema: S, payload: unknown): z.infer<S> {
  const r = schema.safeParse(payload);
  if (!r.success) {
    const first = r.error.issues[0];
    throw new PayloadError(`unexpected payload: ${first?.path.join(".") || "(root)"} ${first?.message ?? ""}`.trim());
  }
  return r.data;
}

/** Returns null for events and actions the adapter ignores (including `ping`). */
export function normalize(eventName: string, payload: unknown): GhEvent | null {
  const action = (payload as { action?: unknown } | null)?.action;
  switch (eventName) {
    case "pull_request": {
      if (typeof action !== "string" || !PR_ACTIONS.has(action)) return null;
      const e = parse(PullRequestEvent, payload);
      return { type: "pr", repo: e.repository.full_name, number: e.pull_request.number, action, pr: prInfo(e.pull_request) };
    }
    case "pull_request_review": {
      if (action !== "submitted" && action !== "dismissed") return null;
      const e = parse(ReviewEvent, payload);
      const login = e.review.user?.login;
      const state = action === "dismissed" ? "dismissed" : e.review.state.toLowerCase();
      if (!login || (state !== "approved" && state !== "changes_requested" && state !== "dismissed")) return null;
      return {
        type: "pr",
        repo: e.repository.full_name,
        number: e.pull_request.number,
        action: `review_${state}`,
        pr: prInfo(e.pull_request),
        review: { login, state },
      };
    }
    case "check_suite": {
      if (action !== "completed") return null;
      const e = parse(CheckSuiteEvent, payload);
      const s = e.check_suite;
      const repoUrl = e.repository.html_url ?? `https://github.com/${e.repository.full_name}`;
      return {
        type: "ci",
        repo: e.repository.full_name,
        repoUrl,
        name: s.app?.name ?? "checks",
        sha: s.head_sha,
        branch: s.head_branch ?? null,
        conclusion: s.conclusion ?? "neutral",
        url: `${repoUrl}/commit/${s.head_sha}/checks`,
        prNumbers: (s.pull_requests ?? []).map((p) => p.number),
      };
    }
    case "workflow_run": {
      if (action !== "completed") return null;
      const e = parse(WorkflowRunEvent, payload);
      const w = e.workflow_run;
      return {
        type: "ci",
        repo: e.repository.full_name,
        repoUrl: e.repository.html_url ?? `https://github.com/${e.repository.full_name}`,
        name: w.name ?? "workflow",
        sha: w.head_sha,
        branch: w.head_branch ?? null,
        conclusion: w.conclusion ?? "neutral",
        url: w.html_url ?? null,
        prNumbers: (w.pull_requests ?? []).map((p) => p.number),
      };
    }
    case "issues": {
      if (action !== "opened" && action !== "closed" && action !== "reopened" && action !== "edited") return null;
      const e = parse(IssuesEvent, payload);
      const i = e.issue;
      return {
        type: "issue",
        repo: e.repository.full_name,
        number: i.number,
        action,
        issue: { title: i.title, url: i.html_url, author: i.user?.login ?? null, state: i.state, body: i.body ?? "" },
      };
    }
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// State per topic key
// ---------------------------------------------------------------------------

export interface CheckResult {
  conclusion: string;
  url: string | null;
}

export interface PrState {
  kind: "pr";
  repo: string;
  number: number;
  /** False while only CI results are known (the PR was opened before the hook existed). */
  known: boolean;
  title: string;
  url: string;
  author: string | null;
  draft: boolean;
  state: "open" | "closed" | "merged";
  headSha: string;
  headRef: string;
  baseRef: string;
  requested: string[];
  /** Latest review per reviewer login: approved or changes_requested. */
  reviews: Record<string, "approved" | "changes_requested">;
  /** CI results of the current head commit, by check name. */
  checks: Record<string, CheckResult>;
}

export interface IssueState {
  kind: "issue";
  repo: string;
  number: number;
  title: string;
  url: string;
  author: string | null;
  state: "open" | "closed";
}

export interface BranchState {
  kind: "branch";
  repo: string;
  repoUrl: string;
  branch: string;
  sha: string;
  checks: Record<string, CheckResult>;
}

export type GhState = PrState | IssueState | BranchState;

export const prKey = (repo: string, n: number) => `github:${repo}#${n}`;
export const branchKey = (repo: string, branch: string) => `github:${repo}@${branch}`;

/** `Topic: <topicId>` in a PR or issue body links it to an existing topic. */
export function linkedTopicId(body: string): string | null {
  const m = /(?:^|\s)Topic:\s*([0-9A-HJKMNP-TV-Z]{26})\b/i.exec(body);
  return m ? m[1].toUpperCase() : null;
}

function fromPr(prev: PrState | null, repo: string, number: number, pr: PrInfo): PrState {
  const sameHead = prev?.headSha === pr.headSha;
  return {
    kind: "pr",
    repo,
    number,
    known: true,
    title: pr.title,
    url: pr.url,
    author: pr.author,
    draft: pr.draft,
    state: pr.state,
    headSha: pr.headSha,
    headRef: pr.headRef,
    baseRef: pr.baseRef,
    requested: pr.requested,
    reviews: { ...(prev?.reviews ?? {}) },
    checks: sameHead ? { ...(prev?.checks ?? {}) } : {},
  };
}

/** Applies a PR or issue event; CI events use `applyCi`. */
export function applyEvent(prev: GhState | null, e: Exclude<GhEvent, { type: "ci" }>): GhState {
  if (e.type === "issue") {
    return { kind: "issue", repo: e.repo, number: e.number, ...e.issue };
  }
  const s = fromPr(prev?.kind === "pr" ? prev : null, e.repo, e.number, e.pr);
  if (e.review) {
    const login = e.review.login.toLowerCase();
    if (e.review.state === "dismissed") delete s.reviews[login];
    else s.reviews[login] = e.review.state as "approved" | "changes_requested";
  }
  return s;
}

/** Applies a CI result to a PR or branch state. Results for an older head commit are ignored. */
export function applyCi(prev: GhState | null, e: Extract<GhEvent, { type: "ci" }>, key: string): GhState | null {
  const result: CheckResult = { conclusion: e.conclusion, url: e.url };
  if (prev?.kind === "issue") return null;
  if (prev?.kind === "pr") {
    if (prev.headSha && prev.headSha !== e.sha) return null;
    return { ...prev, headSha: e.sha, checks: { ...prev.checks, [e.name]: result } };
  }
  if (prev?.kind === "branch") {
    const checks = prev.sha === e.sha ? { ...prev.checks } : {};
    return { ...prev, sha: e.sha, checks: { ...checks, [e.name]: result } };
  }
  if (key.includes("#")) {
    const number = e.prNumbers[0] as number;
    return {
      kind: "pr",
      repo: e.repo,
      number,
      known: false,
      title: `#${number}`,
      url: `${e.repoUrl}/pull/${number}`,
      author: null,
      draft: false,
      state: "open",
      headSha: e.sha,
      headRef: e.branch ?? "",
      baseRef: "",
      requested: [],
      reviews: {},
      checks: { [e.name]: result },
    };
  }
  return { kind: "branch", repo: e.repo, repoUrl: e.repoUrl, branch: e.branch ?? "", sha: e.sha, checks: { [e.name]: result } };
}

// ---------------------------------------------------------------------------
// Topic status and card
// ---------------------------------------------------------------------------

/** Status and the GitHub logins the topic waits on; null = the adapter leaves the topic alone. */
export function desiredTopic(s: GhState): { status: TopicStatus; waitingOn: string[] } | null {
  if (s.kind === "branch") return null;
  if (s.kind === "issue") return { status: s.state === "closed" ? "done" : "open", waitingOn: [] };
  if (!s.known) return null;
  if (s.state !== "open") return { status: "done", waitingOn: [] };
  if (s.draft) return { status: "in_progress", waitingOn: [] };
  if (s.requested.length > 0) return { status: "waiting_review", waitingOn: s.requested };
  const author = s.author ? [s.author] : [];
  const reviews = Object.values(s.reviews);
  if (reviews.includes("changes_requested")) return { status: "in_progress", waitingOn: author };
  if (reviews.includes("approved")) return { status: "open", waitingOn: author };
  return { status: "open", waitingOn: [] };
}

function cut(s: string, max: number): string {
  const cps = [...s];
  return cps.length <= max ? s : `${cps.slice(0, max - 1).join("")}…`;
}

const WEB_URL = /^https?:\/\/[^\s]+$/;

export function topicTitle(s: GhState): string {
  if (s.kind === "branch") return cut(`${s.repo} ${s.branch} CI`, MAX_TITLE_CHARS);
  return cut(`${s.repo}#${s.number} ${s.title}`, MAX_TITLE_CHARS);
}

const CONCLUSION: Record<string, { label: string; status: "done" | "failed" | "skipped" | "in_progress" }> = {
  success: { label: "通过", status: "done" },
  failure: { label: "失败", status: "failed" },
  timed_out: { label: "超时", status: "failed" },
  startup_failure: { label: "启动失败", status: "failed" },
  action_required: { label: "需要处理", status: "failed" },
  cancelled: { label: "已取消", status: "skipped" },
  skipped: { label: "已跳过", status: "skipped" },
  neutral: { label: "中性", status: "done" },
  stale: { label: "过期", status: "skipped" },
};

function checksBlock(checks: Record<string, CheckResult>) {
  const names = Object.keys(checks).sort();
  if (names.length === 0) return [];
  return [
    {
      type: "list" as const,
      items: names.slice(0, 50).map((n) => {
        const c = CONCLUSION[checks[n].conclusion] ?? { label: checks[n].conclusion, status: "in_progress" as const };
        return { text: cut(`${n}: ${c.label}`, 1000), status: c.status };
      }),
    },
  ];
}

function failed(checks: Record<string, CheckResult>): boolean {
  return Object.values(checks).some((c) => (CONCLUSION[c.conclusion]?.status ?? "") === "failed");
}

function link(label: string, url: string | null) {
  return url && WEB_URL.test(url) && url.length <= 2048
    ? [{ type: "link" as const, label, target: { kind: "url" as const, url } }]
    : [];
}

/** The one status card of a topic key. Always valid against the protocol Card schema. */
export function buildCard(s: GhState): Card {
  let card: Card;
  if (s.kind === "branch") {
    card = {
      v: 1,
      title: cut(`${s.repo} ${s.branch} CI`, 200),
      tone: failed(s.checks) ? "danger" : "success",
      status: { label: failed(s.checks) ? "失败" : "通过" },
      blocks: [
        { type: "fields", fields: [{ label: "提交", value: s.sha.slice(0, 12) }] },
        ...checksBlock(s.checks),
        ...link("在 GitHub 打开", `${s.repoUrl}/commit/${s.sha}`),
      ],
    };
  } else if (s.kind === "issue") {
    card = {
      v: 1,
      title: cut(`#${s.number} ${s.title}`, 200),
      tone: s.state === "closed" ? "neutral" : "info",
      status: { label: s.state === "closed" ? "已关闭" : "打开" },
      blocks: [
        {
          type: "fields",
          fields: [
            { label: "仓库", value: cut(s.repo, 1000) },
            { label: "作者", value: s.author ? `@${s.author}` : "-" },
          ],
        },
        ...link("在 GitHub 打开", s.url),
      ],
    };
  } else {
    const reviews = Object.entries(s.reviews);
    const changes = reviews.some(([, v]) => v === "changes_requested");
    const approved = reviews.some(([, v]) => v === "approved");
    let label = "打开";
    let tone: Card["tone"] = "info";
    if (s.state === "merged") [label, tone] = ["已合并", "success"];
    else if (s.state === "closed") [label, tone] = ["已关闭", "neutral"];
    else if (s.draft) [label, tone] = ["草稿", "neutral"];
    else if (changes) [label, tone] = ["需要修改", "warning"];
    else if (s.requested.length > 0) [label, tone] = ["等待评审", "info"];
    else if (approved) [label, tone] = ["已批准", "success"];
    if (s.state === "open" && failed(s.checks)) tone = "danger";

    const fields = [{ label: "仓库", value: cut(s.repo, 1000) }];
    if (s.author) fields.push({ label: "作者", value: `@${s.author}` });
    if (s.headRef) fields.push({ label: "分支", value: cut(s.baseRef ? `${s.headRef} → ${s.baseRef}` : s.headRef, 1000) });
    const reviewLines = [
      ...reviews.map(([login, v]) => `@${login} ${v === "approved" ? "已批准" : "需要修改"}`),
      ...s.requested.filter((l) => !(l.toLowerCase() in s.reviews)).map((l) => `@${l} 待评审`),
    ];
    if (reviewLines.length > 0) fields.push({ label: "评审", value: cut(reviewLines.join("\n"), 1000) });
    card = {
      v: 1,
      title: cut(`#${s.number} ${s.title}`, 200),
      tone,
      status: { label },
      blocks: [{ type: "fields", fields }, ...checksBlock(s.checks), ...link("在 GitHub 打开", s.url)],
    };
  }
  return Card.parse(card);
}
