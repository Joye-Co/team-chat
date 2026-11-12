// The machine side of a run's GitHub (docs/machine-protocol.md, "GitHub (feature `github`)";
// design-github-identity 3.4, unit G3), over HTTP with the machine token:
// - GET  /v1/machine/runs/:id/github           the person's token, for one git or gh use
// - POST /v1/machine/runs/:id/github/wait      the run needs a GitHub the person has not connected
// - POST /v1/machine/runs/:id/github/rejected  git got an authentication failure: check the token once
// Every call checks with the run's desk that the run is still alive on the calling machine. The
// token goes only into the 200 response: never into a log line, an error, an audit detail or a
// run event.
import { GITHUB_WAIT_REASONS, type GithubWait, MachineGithubWaitRequest } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { logger } from "../core/logger";
import type { RouteDef } from "../core/registry";
import { deskStub, linkStub } from "../bots/env";
import { authenticateMachine } from "../bots/store";
import { fetchUser } from "./api";
import { machineKind, mayUseToken } from "./assign";
import { loadAccount, loadToken, markInvalid, unavailableReason } from "./store";

type RunGithubRow = { id: string; bot_id: string; group_id: string; topic_id: string; machine_id: string | null; github_member_id: string | null };

const notAllowed = () => new ApiError("forbidden", "this machine may not use GitHub for this run", "github.not_allowed");

/**
 * The run, when the calling machine has it, it has an identity person and its desk says it has
 * not ended and is on this machine; `forbidden` (`github.not_allowed`) otherwise, also for an
 * unknown run, so a machine learns nothing about other machines' runs.
 */
async function liveRun(request: Request, env: Env, runId: string): Promise<{ run: RunGithubRow & { github_member_id: string }; machineId: string }> {
  const machine = await authenticateMachine(request, env.DB);
  const run = await env.DB.prepare("SELECT id, bot_id, group_id, topic_id, machine_id, github_member_id FROM bot_runs WHERE id = ?")
    .bind(runId)
    .first<RunGithubRow>();
  if (!run || run.machine_id !== machine.id || !run.github_member_id) throw notAllowed();
  // The desk is the one truth of whether a run ended (done, cancelled, timed out, lease lost,
  // moved to another machine all end there or change its machine).
  const live = await deskStub(env, run.bot_id).getRun(run.bot_id, run.id);
  if (!live || live.endedAt !== undefined || live.machineId !== machine.id) throw notAllowed();
  return { run: run as RunGithubRow & { github_member_id: string }, machineId: machine.id };
}

/** Whether the machine may get this person's token: not another person's personal machine on its host. */
async function mayUse(env: Env, machineId: string, memberId: string): Promise<boolean> {
  const row = await env.DB.prepare("SELECT owner_id FROM machines WHERE id = ?").bind(machineId).first<{ owner_id: string | null }>();
  const kind = machineKind(row?.owner_id, memberId);
  if (kind !== "other") return true;
  const runsIn = (await linkStub(env, machineId).status()).info?.runsIn;
  return mayUseToken(kind, runsIn);
}

function waitReason(reason: string): "not_connected" | "invalid" {
  return reason === GITHUB_WAIT_REASONS[1] ? "invalid" : "not_connected";
}

export const githubMachineRoutes: RouteDef[] = [
  {
    method: "GET",
    path: "/v1/machine/runs/:id/github",
    auth: "none",
    async handler({ request, env, params }) {
      const { run, machineId } = await liveRun(request, env, params.id);
      const memberId = run.github_member_id;
      if (unavailableReason(env) !== null || !(await mayUse(env, machineId, memberId))) throw notAllowed();
      const account = await loadAccount(env.DB, memberId);
      if (!account) throw new ApiError("not_found", "the person has not connected GitHub", "github.not_connected");
      if (account.status !== "ok") throw new ApiError("conflict", "GitHub refused the person's sign-in", "github.invalid");
      const got = await loadToken(env, memberId);
      if (!got) throw new ApiError("conflict", "GitHub refused the person's sign-in", "github.invalid");
      await auditStatement(env.DB, { memberId: run.bot_id, tokenId: `machine:${machineId}` }, {
        action: "github.token.issue",
        target: memberId,
        details: { runId: run.id, machineId, memberId },
      }).run();
      return { token: got.token, login: got.login };
    },
  },
  {
    method: "POST",
    path: "/v1/machine/runs/:id/github/wait",
    auth: "none",
    async handler({ request, env, params, body }) {
      const { run } = await liveRun(request, env, params.id);
      const reason = waitReason((await body(MachineGithubWaitRequest)).reason);
      const memberId = run.github_member_id;
      // One wait per run: only the call that stored the row goes on. The row is stored before the
      // account is read, so a connection that lands in between finds it (resumeWaits).
      const stored = await env.DB.prepare(
        `INSERT INTO github_waits (run_id, member_id, group_id, topic_id, bot_id, reason, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (run_id) DO NOTHING`,
      )
        .bind(run.id, memberId, run.group_id, run.topic_id, run.bot_id, reason, Date.now())
        .run();
      if (!stored.meta.changes) return { ok: true };
      // Connected in the meantime: nothing to ask; the run goes on by itself once it stops. Who
      // takes the row back (this call or a connection's resumeWaits) resumes it, once.
      const account = await loadAccount(env.DB, memberId);
      if (reason === "not_connected" && account?.status === "ok") {
        const taken = await env.DB.prepare("DELETE FROM github_waits WHERE run_id = ? AND member_id = ?").bind(run.id, memberId).run();
        if (taken.meta.changes) await deskStub(env, run.bot_id).githubResume(run.bot_id, run.id, memberId);
        return { ok: true };
      }
      await deskStub(env, run.bot_id).githubWait(run.bot_id, run.id, memberId, reason);
      return { ok: true };
    },
  },
  {
    method: "POST",
    path: "/v1/machine/runs/:id/github/rejected",
    auth: "none",
    async handler({ request, env, params }) {
      const { run } = await liveRun(request, env, params.id);
      if (unavailableReason(env) !== null) return { ok: true };
      const got = await loadToken(env, run.github_member_id);
      if (!got) return { ok: true };
      try {
        // Only here is a token checked (OAuth App tokens do not expire).
        if ((await fetchUser(env, got.token)) === null) await markInvalid(env, run.github_member_id);
      } catch (err) {
        logger.warn("github token check failed", { runId: run.id, err: String((err as Error)?.message ?? err).slice(0, 200) });
      }
      return { ok: true };
    },
  },
];

/** `GET /v1/me/github` `waits`: the runs that wait for this person, newest first. */
export async function waitsOf(env: Env, memberId: string): Promise<GithubWait[]> {
  const rows = await env.DB.prepare(
    "SELECT run_id, group_id, topic_id, bot_id, reason FROM github_waits WHERE member_id = ? ORDER BY created_at DESC, run_id DESC LIMIT 100",
  )
    .bind(memberId)
    .all<{ run_id: string; group_id: string; topic_id: string; bot_id: string; reason: string }>();
  return rows.results.map((r) => ({ runId: r.run_id, groupId: r.group_id, topicId: r.topic_id, botId: r.bot_id, reason: r.reason }));
}

/**
 * The person connected GitHub: every run that waited for them goes on (one follow-up each). The
 * row is taken first, so two connections at once resume a run only once.
 */
export async function resumeWaits(env: Env, memberId: string): Promise<void> {
  const rows = await env.DB.prepare("SELECT run_id, bot_id FROM github_waits WHERE member_id = ? ORDER BY created_at, run_id")
    .bind(memberId)
    .all<{ run_id: string; bot_id: string }>();
  for (const w of rows.results) {
    const taken = await env.DB.prepare("DELETE FROM github_waits WHERE run_id = ? AND member_id = ?").bind(w.run_id, memberId).run();
    if (!taken.meta.changes) continue;
    try {
      await deskStub(env, w.bot_id).githubResume(w.bot_id, w.run_id, memberId);
    } catch (err) {
      logger.error("github resume failed", { runId: w.run_id, err });
    }
  }
}

/** A new run of this AI member in a topic: earlier waits there are answered by it (谁说话谁负责). */
export async function supersedeWaits(env: Env, botId: string, topicId: string): Promise<void> {
  await env.DB.prepare("DELETE FROM github_waits WHERE bot_id = ? AND topic_id = ?").bind(botId, topicId).run();
}

/** A deactivated person's waits go away with their connection. */
export async function forgetWaits(env: Env, memberId: string): Promise<void> {
  await env.DB.prepare("DELETE FROM github_waits WHERE member_id = ?").bind(memberId).run();
}
