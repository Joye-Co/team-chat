// HTTP routes of the bots feature (docs/protocol.md, "Bot runs").
import {
  AddArtifactVersionRequest,
  CreateFileRequest,
  CreateMachineRequest,
  EndRunRequest,
  MAX_FILE_BYTES,
  MachineCreateArtifactRequest,
  RUN_KIND_ASIDE,
  ResumeRunRequest,
  SetBotRunnerRequest,
  SetRunAcceptanceRequest,
  SetRunPlanRequest,
  StartRunRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { sha256Hex } from "../core/crypto";
import { getMember, isGroupMember } from "../core/db";
import { ApiError, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import { registerRoute } from "../core/registry";
import { membersChanged } from "../core/routes";
import { type WriterContext, addArtifactVersionAs, artifactGroupOf, completeFile, createArtifactAs, reserveFile } from "../files/routes";
import { toMachine } from "./dispatch";
import { deskStub, linkStub } from "./env";
import type { TakeoverActor } from "./desk/takeover";
import { MACHINE_HEADER } from "./link";
import { authenticateMachine, loadMachineViews, loadRunner, newMachineToken, runIndex } from "./store";

/** Raw sessions are gzip of the CLI's own file, at most 100 MiB. */
const MAX_SESSION_BYTES = 100 * 1024 * 1024;

export function sessionKey(runId: string): string {
  return `sessions/${runId}.jsonl.gz`;
}

async function requireBot(db: D1Database, id: string) {
  const m = await getMember(db, id);
  if (!m || m.kind !== "bot") throw new ApiError("not_found", "bot not found");
  return m;
}

/** The run, when the caller is a member of its group. */
async function visibleRun(db: D1Database, auth: AuthContext, runId: string) {
  const row = await runIndex(db, runId);
  if (!row || !(await isGroupMember(db, row.group_id, auth.memberId))) throw new ApiError("not_found", "run not found");
  return row;
}

/**
 * A machine route of a run (`chat-run` files and 产出): the run must be on the calling machine,
 * active, not a side chat, and its runner member still in the group. The member writes, with
 * `machine:{id}` as the audit credential.
 */
async function machineRun(request: Request, env: Parameters<typeof deskStub>[0], runId: string) {
  const machine = await authenticateMachine(request, env.DB);
  const run = await runIndex(env.DB, runId);
  if (!run || run.machine_id !== machine.id) throw new ApiError("not_found", "run not found");
  const info = await deskStub(env, run.bot_id).getRun(run.bot_id, run.id);
  if (!info || info.endedAt !== undefined) throw new ApiError("conflict", "the run ended", "run.ended");
  if (info.kind === RUN_KIND_ASIDE) throw new ApiError("forbidden", "a side chat run is read only", "output.read_only");
  const bot = await getMember(env.DB, run.bot_id);
  if (!bot || bot.deactivated || !(await isGroupMember(env.DB, run.group_id, run.bot_id))) {
    throw new ApiError("forbidden", "the run's member is not in the group");
  }
  const writer: WriterContext = { memberId: run.bot_id, tokenId: `machine:${machine.id}`, member: { level: bot.level, kind: bot.kind } };
  return { run, writer };
}

export function registerBotRoutes(): void {
  // -------------------------------------------------------------------------
  // Runner-backed bots
  // -------------------------------------------------------------------------

  registerRoute({
    method: "PUT",
    path: "/v1/bots/:id/runner",
    auth: "owner",
    async handler({ env, params, auth, body }) {
      const req = await body(SetBotRunnerRequest);
      await requireBot(env.DB, params.id);
      const now = Date.now();
      const a = auth as AuthContext;
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO bot_runners (member_id, runtime, labels, repo_url, repo_path, base_branch, model, env_allowlist, enabled, updated_by, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT (member_id) DO UPDATE SET runtime = excluded.runtime, labels = excluded.labels,
             repo_url = excluded.repo_url, repo_path = excluded.repo_path, base_branch = excluded.base_branch,
             model = excluded.model, env_allowlist = excluded.env_allowlist, enabled = excluded.enabled,
             updated_by = excluded.updated_by, updated_at = excluded.updated_at`,
        ).bind(
          params.id,
          req.runtime,
          JSON.stringify([...new Set(req.labels ?? [])]),
          req.repo?.url ?? null,
          req.repo?.path ?? null,
          req.baseBranch ?? null,
          req.model ?? null,
          JSON.stringify([...new Set(req.envAllowlist ?? [])]),
          req.enabled === false ? 0 : 1,
          a.memberId,
          now,
        ),
        auditStatement(env.DB, a, { action: "bot.runner.set", target: params.id, details: { ...req } }, now),
      ]);
      await membersChanged(env, [params.id]);
      return loadRunner(env.DB, params.id);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/bots/:id/runner",
    auth: "owner",
    async handler({ env, params }) {
      const r = await loadRunner(env.DB, params.id);
      if (!r) throw new ApiError("not_found", "bot is not runner-backed");
      return r;
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/bots/:id/runner",
    auth: "owner",
    async handler({ env, params, auth }) {
      const res = await env.DB.batch([
        env.DB.prepare("DELETE FROM bot_runners WHERE member_id = ?").bind(params.id),
        auditStatement(env.DB, auth as AuthContext, { action: "bot.runner.delete", target: params.id }),
      ]);
      if (!res[0].meta.changes) throw new ApiError("not_found", "bot is not runner-backed");
      await membersChanged(env, [params.id]);
      return { ok: true };
    },
  });

  // -------------------------------------------------------------------------
  // Machines
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/machines",
    auth: "owner",
    async handler({ env, auth, body }) {
      const req = await body(CreateMachineRequest);
      const a = auth as AuthContext;
      const now = Date.now();
      const id = ulid(now);
      const token = newMachineToken();
      const labels = [...new Set(req.labels ?? [])].sort();
      await env.DB.batch([
        env.DB.prepare(
          "INSERT INTO machines (id, name, labels, token_hash, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        ).bind(id, req.name, JSON.stringify(labels), await sha256Hex(token), a.memberId, now),
        auditStatement(env.DB, a, { action: "machine.create", target: id, details: { name: req.name, labels } }, now),
      ]);
      const machine = toMachine(
        { id, name: req.name, labels, revoked: false, connected: false, runningRuns: [] },
        a.memberId,
        now,
        now,
      );
      return { machine, token };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machines",
    auth: "member",
    async handler({ env, auth }) {
      const now = Date.now();
      const views = await loadMachineViews(env, { includeRevoked: true });
      const all = views.map(({ row, view }) => toMachine(view, row.created_by, row.created_at, now));
      const a = auth as AuthContext;
      if (a.member.level === "owner") return all;
      // Members see which machine ran a run, nothing about the machine itself; the requester of
      // a run that runs on a machine also gets its screen sharing address (接管).
      const mine = new Set<string>();
      for (const m of all) {
        if (!m.takeoverUrl) continue;
        for (const runId of m.runningRuns) {
          const row = await runIndex(env.DB, runId);
          const run = row ? await deskStub(env, row.bot_id).getRun(row.bot_id, row.id) : null;
          if (run?.requestedBy === a.memberId) mine.add(m.id);
        }
      }
      return all.map((m) => ({
        ...(mine.has(m.id) ? { takeoverUrl: m.takeoverUrl } : {}),
        id: m.id,
        name: m.name,
        labels: [],
        createdBy: m.createdBy,
        createdAt: m.createdAt,
        ...(m.revoked ? { revoked: true } : {}),
        connected: m.connected,
        healthy: m.healthy,
        runningRuns: [],
      }));
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/machines/:id",
    auth: "owner",
    async handler({ env, params, auth }) {
      const now = Date.now();
      const res = await env.DB.batch([
        env.DB.prepare("UPDATE machines SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL").bind(now, params.id),
        auditStatement(env.DB, auth as AuthContext, { action: "machine.revoke", target: params.id }, now),
      ]);
      if (!res[0].meta.changes) throw new ApiError("not_found", "machine not found");
      await linkStub(env, params.id).revoke(params.id);
      return { ok: true };
    },
  });

  // -------------------------------------------------------------------------
  // The daemon: WebSocket and raw session upload (machine token)
  // -------------------------------------------------------------------------

  registerRoute({
    method: "GET",
    path: "/v1/machine/ws",
    auth: "none",
    async handler({ request, env }) {
      if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
        throw new ApiError("invalid", "expected a websocket upgrade");
      }
      // 401 before the upgrade.
      const machine = await authenticateMachine(request, env.DB);
      const headers = new Headers(request.headers);
      headers.delete("authorization");
      headers.set(MACHINE_HEADER, machine.id);
      const ns = linkStub(env, machine.id) as unknown as { fetch(r: Request): Promise<Response> };
      return ns.fetch(new Request(request.url, { method: "GET", headers }));
    },
  });

  registerRoute({
    method: "PUT",
    path: "/v1/machine/runs/:id/session",
    auth: "none",
    async handler({ request, env, params }) {
      const machine = await authenticateMachine(request, env.DB);
      const run = await runIndex(env.DB, params.id);
      if (!run || run.machine_id !== machine.id) throw new ApiError("not_found", "run not found");
      const declared = request.headers.get("content-length");
      if (declared === null || !/^\d+$/.test(declared)) throw new ApiError("invalid", "Content-Length is required");
      // A run uploads with one PUT; parts are for member clients.
      if (Number(declared) > MAX_FILE_BYTES) throw new ApiError("too_large", "file too large");
      const size = Number(declared);
      if (size > MAX_SESSION_BYTES) throw new ApiError("too_large", "session is larger than 100 MiB");
      if (!request.body || size === 0) throw new ApiError("invalid", "empty session");
      const sha = request.headers.get("x-session-sha256") ?? "";
      const bytes = request.headers.get("x-session-bytes") ?? "";
      const fixed = new FixedLengthStream(size);
      const piping = request.body.pipeTo(fixed.writable).catch(() => {});
      try {
        await env.FILES.put(sessionKey(run.id), fixed.readable, {
          httpMetadata: { contentType: "application/gzip" },
          customMetadata: {
            botId: run.bot_id,
            machineId: machine.id,
            ...(/^[0-9a-f]{64}$/.test(sha) ? { sha256: sha } : {}),
            ...(/^\d{1,15}$/.test(bytes) ? { bytes } : {}),
          },
        });
      } catch {
        throw new ApiError("invalid", "session body does not match Content-Length");
      } finally {
        await piping;
      }
      return { ok: true };
    },
  });

  // -------------------------------------------------------------------------
  // chat-run: files and 产出 of a run (docs/machine-protocol.md, "Files and 产出")
  // -------------------------------------------------------------------------

  registerRoute({
    method: "PUT",
    path: "/v1/machine/runs/:id/files",
    auth: "none",
    async handler({ request, env, params }) {
      const { run, writer } = await machineRun(request, env, params.id);
      const declared = request.headers.get("content-length");
      if (declared === null || !/^\d+$/.test(declared)) throw new ApiError("invalid", "Content-Length is required");
      let name: string;
      try {
        name = decodeURIComponent(request.headers.get("x-file-name") ?? "");
      } catch {
        throw new ApiError("invalid", "X-File-Name is not URI-encoded UTF-8");
      }
      const req = CreateFileRequest.safeParse({
        groupId: run.group_id,
        topicId: run.topic_id,
        name,
        size: Number(declared),
        mime: (request.headers.get("content-type") ?? "application/octet-stream").split(";")[0].trim().toLowerCase(),
        sha256: (request.headers.get("x-file-sha256") ?? "").toLowerCase(),
      });
      if (!req.success) {
        const issue = req.error.issues[0];
        throw new ApiError("invalid", `invalid file: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim());
      }
      const row = await reserveFile(env, writer, req.data);
      return completeFile(env, writer, row, request);
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/machine/runs/:id/artifacts",
    auth: "none",
    // The first data snapshot may be 256 KiB on its own.
    bodyLimit: 640 * 1024,
    async handler({ request, env, params, body }) {
      const { run, writer } = await machineRun(request, env, params.id);
      const req = await body(MachineCreateArtifactRequest);
      return createArtifactAs(env, writer, { ...req, groupId: run.group_id, topicId: run.topic_id, runId: run.id });
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/machine/runs/:id/artifacts/:artifactId/versions",
    auth: "none",
    bodyLimit: 640 * 1024,
    async handler({ request, env, params, body }) {
      const { run, writer } = await machineRun(request, env, params.id);
      const req = await body(AddArtifactVersionRequest);
      // A run adds versions only to 产出 of its own group.
      const groupId = await artifactGroupOf(env, params.artifactId);
      if (groupId !== run.group_id) throw new ApiError("not_found", "产出 not found");
      return addArtifactVersionAs(env, writer, groupId, params.artifactId, { ...req, runId: run.id });
    },
  });

  // -------------------------------------------------------------------------
  // Runs for clients
  // -------------------------------------------------------------------------

  registerRoute({
    method: "GET",
    path: "/v1/runs/:id",
    auth: "member",
    async handler({ env, params, auth }) {
      const row = await visibleRun(env.DB, auth as AuthContext, params.id);
      const run = await deskStub(env, row.bot_id).getRun(row.bot_id, row.id);
      if (!run) throw new ApiError("not_found", "run not found");
      return run;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/runs/:id/events",
    auth: "member",
    async handler({ env, params, auth, url }) {
      const row = await visibleRun(env.DB, auth as AuthContext, params.id);
      const after = url.searchParams.get("afterSeq");
      const limit = url.searchParams.get("limit");
      const afterSeq = after === null ? 0 : Number(after);
      const n = limit === null ? 200 : Number(limit);
      if (!Number.isInteger(afterSeq) || afterSeq < 0) throw new ApiError("invalid", "afterSeq must be a non-negative integer");
      if (!Number.isInteger(n) || n < 1 || n > 500) throw new ApiError("invalid", "limit must be 1..500");
      const page = await deskStub(env, row.bot_id).events(row.bot_id, row.id, afterSeq, n);
      if (!page) throw new ApiError("not_found", "run not found");
      return page;
    },
  });

  // -------------------------------------------------------------------------
  // Runs of external bots and run plans (docs/protocol.md, "Run plans")
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/runs",
    auth: "member",
    async handler({ env, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(StartRunRequest);
      if (a.member.kind !== "bot") throw new ApiError("forbidden", "only bots start runs");
      if (!(await isGroupMember(env.DB, req.groupId, a.memberId))) throw new ApiError("not_found", "group not found");
      if (await loadRunner(env.DB, a.memberId)) throw new ApiError("conflict", "a runner-backed bot gets runs from mentions");
      const run = unwrap(await deskStub(env, a.memberId).startExternal(a.memberId, req.groupId, req.topicId, req.plan));
      await auditStatement(env.DB, a, { action: "run.start", target: run.id, details: { topicId: req.topicId, planItems: req.plan?.length ?? 0 } }).run();
      return run;
    },
  });

  registerRoute({
    method: "PUT",
    path: "/v1/runs/:id/plan",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(SetRunPlanRequest);
      const row = await visibleRun(env.DB, a, params.id);
      if (row.bot_id !== a.memberId) throw new ApiError("forbidden", "only the run's bot sets its plan");
      const run = unwrap(await deskStub(env, row.bot_id).setPlan(row.bot_id, row.id, req.plan));
      await auditStatement(env.DB, a, { action: "run.plan", target: row.id, details: { planItems: req.plan.length } }).run();
      return run;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/end",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(EndRunRequest);
      const row = await visibleRun(env.DB, a, params.id);
      if (row.bot_id !== a.memberId) throw new ApiError("forbidden", "only the run's bot ends it");
      const run = unwrap(await deskStub(env, row.bot_id).endExternal(row.bot_id, row.id, req.status, req.summary));
      await auditStatement(env.DB, a, { action: "run.end", target: row.id, details: { status: req.status } }).run();
      return run;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/cancel",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      // People in the run's group stop runs; a bot ends its own runs with /end.
      if (a.member.kind !== "human") throw new ApiError("forbidden", "only people stop runs");
      const row = await visibleRun(env.DB, a, params.id);
      const run = unwrap(await deskStub(env, row.bot_id).cancel(row.bot_id, row.id, a.memberId));
      await auditStatement(env.DB, a, { action: "run.cancel", target: row.id, details: { botId: row.bot_id } }).run();
      return run;
    },
  });

  // -------------------------------------------------------------------------
  // 1.1: take-over (pause / resume) and acceptance criteria
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/pause",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const run = unwrap(await deskStub(env, row.bot_id).pause(row.bot_id, row.id, who));
      await auditStatement(env.DB, a, { action: "run.pause", target: row.id, details: { botId: row.bot_id } }).run();
      return run;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/resume",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(ResumeRunRequest);
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const run = unwrap(await deskStub(env, row.bot_id).resume(row.bot_id, row.id, who, req.note));
      // The note itself goes to the CLI, not into the audit log.
      await auditStatement(env.DB, a, { action: "run.resume", target: row.id, details: { botId: row.bot_id, note: Boolean(req.note) } }).run();
      return run;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/retry",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const run = unwrap(await deskStub(env, row.bot_id).retry(row.bot_id, row.id, who));
      await auditStatement(env.DB, a, { action: "run.retry", target: row.id, details: { botId: row.bot_id, runId: run.id } }).run();
      return run;
    },
  });

  registerRoute({
    method: "PUT",
    path: "/v1/runs/:id/acceptance",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(SetRunAcceptanceRequest);
      const row = await visibleRun(env.DB, a, params.id);
      if (row.bot_id !== a.memberId) throw new ApiError("forbidden", "only the run's bot sets its acceptance criteria");
      const run = unwrap(await deskStub(env, row.bot_id).setAcceptance(row.bot_id, row.id, req.items));
      await auditStatement(env.DB, a, { action: "run.acceptance", target: row.id, details: { items: req.items.length } }).run();
      return run;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/runs/:id/session",
    auth: "owner",
    async handler({ env, params }) {
      const row = await runIndex(env.DB, params.id);
      if (!row) throw new ApiError("not_found", "run not found");
      const obj = await env.FILES.get(sessionKey(row.id));
      if (!obj) throw new ApiError("not_found", "no session uploaded for this run");
      return new Response(obj.body, {
        headers: {
          "content-type": "application/gzip",
          "content-length": String(obj.size),
          "content-disposition": `attachment; filename="${row.id}.jsonl.gz"`,
          "x-content-type-options": "nosniff",
        },
      });
    },
  });
}
