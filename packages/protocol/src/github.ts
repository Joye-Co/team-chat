// Usable: each person's GitHub sign-in. See docs/protocol.md, "GitHub sign-in", and
// docs/machine-protocol.md, "GitHub (feature `github`)"; design
// docs/plans/usable/design-github-identity.zh.md. Readers take states and reasons as open strings.
import { z } from "zod";
import { Id, Millis } from "./common.ts";

/** Known `GithubAccount.status` values; an unknown value reads as "invalid". */
export const GITHUB_ACCOUNT_STATUSES = ["ok", "invalid"] as const;
/** Known `GithubStatusResponse.unavailableReason` values. */
export const GITHUB_UNAVAILABLE_REASONS = ["no_client_id", "no_secret_key"] as const;
/** Known `GithubWait.reason` and `MachineGithubWaitRequest.reason` values. */
export const GITHUB_WAIT_REASONS = ["not_connected", "invalid"] as const;
/** Known `GithubPollResponse.state` values. */
export const GITHUB_POLL_STATES = ["pending", "slow_down", "done", "expired", "denied", "taken"] as const;
/** `hello.features` entry that turns on the `github.status` transient for a connection. */
export const FEATURE_GITHUB = "github/1";

/** A person's connected GitHub account. Never the token. */
export const GithubAccount = z.object({
  login: z.string().min(1).max(100),
  /** The GitHub profile name; absent = use `login`. */
  name: z.string().min(1).max(255).optional(),
  /** "ok" | "invalid" (GitHub refused the token; connect again). */
  status: z.string().min(1).max(32),
  connectedAt: Millis,
});
export type GithubAccount = z.infer<typeof GithubAccount>;

/** A run that ended waiting for this person to connect GitHub. */
export const GithubWait = z.object({
  runId: Id,
  groupId: Id,
  topicId: Id,
  botId: Id,
  /** "not_connected" | "invalid" */
  reason: z.string().min(1).max(32),
});
export type GithubWait = z.infer<typeof GithubWait>;

/** `GET /v1/me/github` */
export const GithubStatusResponse = z.object({
  /** The server can connect GitHub (a client id and its secret key are set). */
  available: z.boolean(),
  /** Why not, when `available` is false: "no_client_id" | "no_secret_key". */
  unavailableReason: z.string().min(1).max(64).optional(),
  /** Absent = not connected. */
  account: GithubAccount.optional(),
  waits: z.array(GithubWait).max(100),
});
export type GithubStatusResponse = z.infer<typeof GithubStatusResponse>;

/** `POST /v1/me/github/device`: a GitHub device code sign-in the server started. */
export const GithubDeviceResponse = z.object({
  deviceId: Id,
  /** What the person types on GitHub's page. */
  userCode: z.string().min(1).max(32),
  verificationUri: z.string().min(1).max(2048),
  /** Seconds between polls. */
  interval: z.int().positive(),
  expiresAt: Millis,
});
export type GithubDeviceResponse = z.infer<typeof GithubDeviceResponse>;

/** `POST /v1/me/github/device/:id/poll` */
export const GithubPollResponse = z.object({
  /** "pending" | "slow_down" | "done" | "expired" | "denied" | "taken" (`GITHUB_POLL_STATES`). */
  state: z.string().min(1).max(32),
  /** With "done". */
  account: GithubAccount.optional(),
});
export type GithubPollResponse = z.infer<typeof GithubPollResponse>;

/**
 * Transient, only to the person's own connections whose hello has feature `github/1`: the
 * connection changed. Absent `account` = disconnected.
 */
export const GithubStatusTransient = z.object({
  type: z.literal("github.status"),
  account: GithubAccount.optional(),
});
export type GithubStatusTransient = z.infer<typeof GithubStatusTransient>;

// ---------------------------------------------------------------------------
// Machine side (docs/machine-protocol.md, feature `github`)
// ---------------------------------------------------------------------------

/** Known `hello.machine.runsIn` values. */
export const MACHINE_RUNS_IN = ["host", "vm"] as const;
/** Known `RunGithub.source` values. */
export const RUN_GITHUB_SOURCES = ["app", "machine"] as const;
/** Known states of the `github` run event. */
export const RUN_GITHUB_STATES = ["used", "missing", "invalid"] as const;

/** A git author or committer. */
export const GitIdentity = z.object({
  name: z.string().min(1).max(200),
  email: z.string().min(3).max(320),
});
export type GitIdentity = z.infer<typeof GitIdentity>;

/** `run.assign.github` (feature `github`): whose GitHub the run uses and how. */
export const RunGithub = z.object({
  /** "app": git and gh ask the daemon for the person's token; "machine": the machine's own sign-in. */
  source: z.string().min(1).max(16),
  /** The person whose GitHub the run acts as. */
  memberId: Id,
  /** Their GitHub login, when connected. */
  login: z.string().min(1).max(100).optional(),
  /** Commit author; absent = the machine's own git config. */
  author: GitIdentity.optional(),
  /** Commit committer: the AI member. */
  committer: GitIdentity,
  /** Rewrite `git@github.com:` and `ssh://git@github.com/` to HTTPS and drop the SSH agent. */
  rewriteSsh: z.boolean().optional(),
});
export type RunGithub = z.infer<typeof RunGithub>;

/** `GET /v1/machine/runs/:id/github` (machine token). Never logged. */
export const MachineGithubTokenResponse = z.object({
  token: z.string().min(1).max(1024),
  login: z.string().min(1).max(100),
});
export type MachineGithubTokenResponse = z.infer<typeof MachineGithubTokenResponse>;

/** `POST /v1/machine/runs/:id/github/wait` (machine token). Response `{ ok: true }`. */
export const MachineGithubWaitRequest = z.object({
  /** "not_connected" | "invalid" */
  reason: z.string().min(1).max(32),
});
export type MachineGithubWaitRequest = z.infer<typeof MachineGithubWaitRequest>;
