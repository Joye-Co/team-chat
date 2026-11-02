// GitHub webhook payloads shaped after GitHub's documented webhook schemas
// (docs.github.com, "Webhook events and payloads"). They keep the fields GitHub sends that are
// near the ones the adapter reads, so a schema mistake shows up here and not in production.

export const REPO = "joye-co/team-chat";

const repository = {
  id: 812345678,
  node_id: "R_kgDOMG5x7g",
  name: "team-chat",
  full_name: REPO,
  private: true,
  owner: { login: "joye-co", id: 170000001, type: "Organization", site_admin: false },
  html_url: `https://github.com/${REPO}`,
  description: null,
  fork: false,
  url: `https://api.github.com/repos/${REPO}`,
  default_branch: "main",
};

const user = (login: string, id: number) => ({
  login,
  id,
  node_id: `U_${id}`,
  avatar_url: `https://avatars.githubusercontent.com/u/${id}?v=4`,
  html_url: `https://github.com/${login}`,
  type: "User",
  site_admin: false,
});

const sender = user("alice-gh", 1001);

export interface PrOpts {
  number?: number;
  title?: string;
  body?: string | null;
  author?: string;
  draft?: boolean;
  state?: "open" | "closed";
  merged?: boolean;
  sha?: string;
  ref?: string;
  reviewers?: string[];
}

export function pullRequest(o: PrOpts = {}) {
  const n = o.number ?? 42;
  return {
    url: `https://api.github.com/repos/${REPO}/pulls/${n}`,
    id: 2000000000 + n,
    node_id: `PR_kwDOMG5x7s5${n}`,
    html_url: `https://github.com/${REPO}/pull/${n}`,
    diff_url: `https://github.com/${REPO}/pull/${n}.diff`,
    number: n,
    state: o.state ?? "open",
    locked: false,
    title: o.title ?? "Add webhook adapter",
    user: user(o.author ?? "alice-gh", 1001),
    body: o.body === undefined ? "Adds the adapter." : o.body,
    created_at: "2026-11-01T08:00:00Z",
    updated_at: "2026-11-01T09:00:00Z",
    closed_at: o.state === "closed" ? "2026-11-02T10:00:00Z" : null,
    merged_at: o.merged ? "2026-11-02T10:00:00Z" : null,
    merge_commit_sha: o.merged ? "9f1c0de5a1e8b0c4d2f3a4b5c6d7e8f9a0b1c2d3" : null,
    assignee: null,
    assignees: [],
    requested_reviewers: (o.reviewers ?? []).map((l, i) => user(l, 2000 + i)),
    requested_teams: [],
    labels: [],
    milestone: null,
    draft: o.draft ?? false,
    head: {
      label: `joye-co:${o.ref ?? "feat/adapter"}`,
      ref: o.ref ?? "feat/adapter",
      sha: o.sha ?? "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
      user: user("joye-co", 170000001),
      repo: repository,
    },
    base: {
      label: "joye-co:main",
      ref: "main",
      sha: "0000000000000000000000000000000000000001",
      user: user("joye-co", 170000001),
      repo: repository,
    },
    author_association: "MEMBER",
    auto_merge: null,
    merged: o.merged ?? false,
    mergeable: null,
    comments: 0,
    review_comments: 0,
    commits: 3,
    additions: 120,
    deletions: 4,
    changed_files: 5,
  };
}

export function pullRequestEvent(action: string, o: PrOpts = {}, extra: Record<string, unknown> = {}) {
  return { action, number: o.number ?? 42, pull_request: pullRequest(o), repository, sender, ...extra };
}

export function reviewEvent(state: "approved" | "changes_requested" | "commented", reviewer: string, o: PrOpts = {}) {
  return {
    action: "submitted",
    review: {
      id: 3000000001,
      node_id: "PRR_kwDOMG5x7s6",
      user: user(reviewer, 2000),
      body: null,
      commit_id: o.sha ?? "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
      submitted_at: "2026-11-01T10:00:00Z",
      state,
      html_url: `https://github.com/${REPO}/pull/${o.number ?? 42}#pullrequestreview-3000000001`,
      author_association: "MEMBER",
    },
    pull_request: pullRequest(o),
    repository,
    sender: user(reviewer, 2000),
  };
}

export function checkSuiteEvent(conclusion: string, o: { sha?: string; branch?: string; prs?: number[] } = {}) {
  const sha = o.sha ?? "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
  return {
    action: "completed",
    check_suite: {
      id: 4000000001,
      node_id: "CS_kwDOMG5x7s8",
      head_branch: o.branch ?? "feat/adapter",
      head_sha: sha,
      status: "completed",
      conclusion,
      url: `https://api.github.com/repos/${REPO}/check-suites/4000000001`,
      before: "0000000000000000000000000000000000000001",
      after: sha,
      pull_requests: (o.prs ?? [42]).map((n) => ({
        url: `https://api.github.com/repos/${REPO}/pulls/${n}`,
        id: 2000000000 + n,
        number: n,
        head: { ref: o.branch ?? "feat/adapter", sha, repo: { id: 812345678, url: repository.url, name: "team-chat" } },
        base: { ref: "main", sha: "0000000000000000000000000000000000000001", repo: { id: 812345678, url: repository.url, name: "team-chat" } },
      })),
      app: { id: 15368, slug: "github-actions", name: "GitHub Actions", owner: user("github", 9919) },
      created_at: "2026-11-01T09:01:00Z",
      updated_at: "2026-11-01T09:05:00Z",
      latest_check_runs_count: 3,
      check_runs_url: `https://api.github.com/repos/${REPO}/check-suites/4000000001/check-runs`,
      head_commit: { id: sha, tree_id: "1111111111111111111111111111111111111111", message: "feat: adapter", timestamp: "2026-11-01T09:00:00Z" },
    },
    repository,
    sender,
  };
}

export function workflowRunEvent(
  conclusion: string,
  o: { name?: string; sha?: string; branch?: string; prs?: number[] } = {},
) {
  const sha = o.sha ?? "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
  return {
    action: "completed",
    workflow_run: {
      id: 5000000001,
      name: o.name ?? "server",
      node_id: "WFR_kwLOMG5x7s8",
      head_branch: o.branch ?? "feat/adapter",
      head_sha: sha,
      path: ".github/workflows/server.yml",
      run_number: 17,
      event: "pull_request",
      status: "completed",
      conclusion,
      workflow_id: 6000001,
      html_url: `https://github.com/${REPO}/actions/runs/5000000001`,
      pull_requests: (o.prs ?? []).map((n) => ({ url: `https://api.github.com/repos/${REPO}/pulls/${n}`, id: 2000000000 + n, number: n })),
      created_at: "2026-11-01T09:01:00Z",
      updated_at: "2026-11-01T09:06:00Z",
      run_attempt: 1,
      head_commit: { id: sha, message: "feat: adapter" },
      repository,
      head_repository: repository,
    },
    workflow: { id: 6000001, name: o.name ?? "server", path: ".github/workflows/server.yml", state: "active" },
    repository,
    sender,
  };
}

export function issuesEvent(action: "opened" | "closed" | "reopened", o: { number?: number; title?: string; body?: string } = {}) {
  const n = o.number ?? 7;
  return {
    action,
    issue: {
      url: `https://api.github.com/repos/${REPO}/issues/${n}`,
      id: 7000000000 + n,
      node_id: `I_kwDOMG5x7s6${n}`,
      number: n,
      title: o.title ?? "Login fails on slow networks",
      user: user("bob-gh", 1002),
      labels: [],
      state: action === "closed" ? "closed" : "open",
      state_reason: action === "closed" ? "completed" : null,
      locked: false,
      assignee: null,
      assignees: [],
      comments: 0,
      created_at: "2026-11-01T08:00:00Z",
      updated_at: "2026-11-01T08:00:00Z",
      closed_at: action === "closed" ? "2026-11-02T08:00:00Z" : null,
      author_association: "MEMBER",
      body: o.body ?? "Steps to reproduce: ...",
      html_url: `https://github.com/${REPO}/issues/${n}`,
    },
    repository,
    sender: user("bob-gh", 1002),
  };
}

export const pingEvent = (hookId: number) => ({
  zen: "Keep it logically awesome.",
  hook_id: hookId,
  hook: { type: "Repository", id: hookId, name: "web", active: true, events: ["pull_request"], config: { content_type: "json" } },
  repository,
  sender,
});
