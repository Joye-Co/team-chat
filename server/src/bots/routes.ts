// HTTP routes of the bots feature (docs/protocol.md, "Bot runs").
import {
  AddArtifactVersionRequest,
  ChainStopResponse,
  CreateFileRequest,
  CreateMachineRequest,
  EndRunRequest,
  MAX_FILE_BYTES,
  MAX_PERSONAL_BOTS,
  type Machine,
  MachineCreateArtifactRequest,
  RUN_KIND_ASIDE,
  ResumeRunRequest,
  RunCommandRequest,
  SetBotRunnerRequest,
  SetMachineBotRequest,
  SetRunAcceptanceRequest,
  SetRunPlanRequest,
  StartRunRequest,
  UpdateMachineRequest,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import { auditValue } from "../core/auditvalue";
import { roomStub } from "../core/env";
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
import type { Env } from "../core/env";
import {
  type ManagedMachine,
  authenticateMachine,
  loadMachineViews,
  loadRunner,
  newMachineToken,
  requireMachineManager,
  revokeMachines,
  runIndex,
} from "./store";

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
 * `GET /v1/machines` for the caller: team machines in full for admins and cut down for members
 * (as before); my own personal machines in full with my checked AI members; other people's
 * personal machines cut down, with `ownerId`. A cut-down machine keeps its screen sharing
 * address (接管) for the person a run on it works for.
 */
async function visibleMachines(env: Env, a: AuthContext, only?: string): Promise<Machine[]> {
  const now = Date.now();
  const admin = a.member.level === "owner";
  const out: Machine[] = [];
  for (const { row, view } of await loadMachineViews(env, { includeRevoked: true })) {
    if (only !== undefined && row.id !== only) continue;
    const m = toMachine(view, row.created_by, row.created_at, now);
    if (view.ownerId !== undefined && view.ownerId === a.memberId) {
      const personalBots = [...(view.personalBots ?? new Map<string, string | null>())].map(([botId, repoPath]) =>
        repoPath ? { botId, repoPath } : { botId },
      );
      out.push({ ...m, personalBots });
      continue;
    }
    if (view.ownerId === undefined && admin) {
      out.push(m);
      continue;
    }
    let mine = false;
    if (m.takeoverUrl) {
      for (const runId of m.runningRuns) {
        const ix = await runIndex(env.DB, runId);
        const run = ix ? await deskStub(env, ix.bot_id).getRun(ix.bot_id, ix.id) : null;
        if (run && (run.requestedFor ?? run.requestedBy) === a.memberId) mine = true;
      }
    }
    out.push({
      ...(mine ? { takeoverUrl: m.takeoverUrl } : {}),
      id: m.id,
      name: m.name,
      labels: [],
      createdBy: m.createdBy,
      createdAt: m.createdAt,
      ...(m.ownerId ? { ownerId: m.ownerId } : {}),
      ...(m.revoked ? { revoked: true } : {}),
      connected: m.connected,
      healthy: m.healthy,
      runningRuns: [],
    });
  }
  return out;
}

/** One machine as `visibleMachines` shows it to the caller. */
async function oneMachine(env: Env, a: AuthContext, id: string): Promise<Machine> {
  const [m] = await visibleMachines(env, a, id);
  if (!m) throw new ApiError("not_found", "machine not found");
  return m;
}

/** A personal machine of the caller (checked AI members live only there). */
async function requirePersonalOwner(db: D1Database, a: AuthContext, machineId: string): Promise<ManagedMachine> {
  const m = await requireMachineManager(db, a, machineId);
  if (m.owner_id !== a.memberId) {
    throw new ApiError("forbidden", "AI members are checked only on your own personal machine", "machine.not_yours");
  }
  return m;
}

/**
 * A machine route of a run (`chat-run` files and 产出): the run must be on the calling machine,
 * active, not a side chat, and its runner member still in the group. The member writes, with
 * `machine:{id}` as the audit credential.
 */
/** `read`: the route only reads, so a read-only side chat run (单独问) may call it too. */
export async function machineRun(request: Request, env: Parameters<typeof deskStub>[0], runId: string, opts: { read?: boolean } = {}) {
  const machine = await authenticateMachine(request, env.DB);
  const run = await runIndex(env.DB, runId);
  if (!run || run.machine_id !== machine.id) throw new ApiError("not_found", "run not found");
  const info = await deskStub(env, run.bot_id).getRun(run.bot_id, run.id);
  if (!info || info.endedAt !== undefined) throw new ApiError("conflict", "the run ended", "run.ended");
  if (info.kind === RUN_KIND_ASIDE && !opts.read) throw new ApiError("forbidden", "a side chat run is read only", "output.read_only");
  const bot = await getMember(env.DB, run.bot_id);
  if (!bot || bot.deactivated || !(await isGroupMember(env.DB, run.group_id, run.bot_id))) {
    throw new ApiError("forbidden", "the run's member is not in the group");
  }
  const writer: WriterContext = { memberId: run.bot_id, tokenId: `machine:${machine.id}`, member: { level: bot.level, kind: bot.kind } };
  return { run, writer, info };
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
      const a = auth as AuthContext;
      if (req.machineId) {
        const m = await env.DB.prepare("SELECT id, owner_id FROM machines WHERE id = ? AND revoked_at IS NULL")
          .bind(req.machineId)
          .first<{ id: string; owner_id: string | null }>();
        if (!m) throw new ApiError("not_found", "machine not found");
        // Placing an AI member on a personal machine is its owner's call; sending the stored
        // machine back (an edit of the model, runtime or labels) is always allowed.
        if (m.owner_id && m.owner_id !== a.memberId) {
          const stored = await env.DB.prepare("SELECT machine_id FROM bot_runners WHERE member_id = ?")
            .bind(params.id)
            .first<{ machine_id: string | null }>();
          if (stored?.machine_id !== req.machineId) {
            throw new ApiError("forbidden", "only its owner places an AI member on a personal machine", "machine.not_yours");
          }
        }
      }
      const now = Date.now();
      // machineId: absent keeps the stored machine (clients before 1.2.2 send none), null clears.
      const keepMachine = req.machineId === undefined ? 1 : 0;
      // effort: the same rule (absent keeps, null clears).
      const keepEffort = req.effort === undefined ? 1 : 0;
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO bot_runners (member_id, runtime, labels, repo_url, repo_path, base_branch, model, env_allowlist, enabled, updated_by, updated_at, machine_id, effort)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT (member_id) DO UPDATE SET runtime = excluded.runtime, labels = excluded.labels,
             repo_url = excluded.repo_url, repo_path = excluded.repo_path, base_branch = excluded.base_branch,
             model = excluded.model, env_allowlist = excluded.env_allowlist, enabled = excluded.enabled,
             updated_by = excluded.updated_by, updated_at = excluded.updated_at,
             machine_id = CASE WHEN ? = 1 THEN bot_runners.machine_id ELSE excluded.machine_id END,
             effort = CASE WHEN ? = 1 THEN bot_runners.effort ELSE excluded.effort END`,
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
          req.machineId ?? null,
          req.effort ?? null,
          keepMachine,
          keepEffort,
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
    auth: "member",
    async handler({ env, auth, body }) {
      const req = await body(CreateMachineRequest);
      const a = auth as AuthContext;
      // An admin without `personal` makes a team machine (as before); a member always makes his own.
      const admin = a.member.level === "owner";
      const personal = req.personal ?? !admin;
      if (!personal && !admin) throw new ApiError("forbidden", "only an admin adds a team machine", "machine.team_admin_only");
      if (personal && a.member.kind !== "human") throw new ApiError("forbidden", "only people have personal machines");
      const ownerId = personal ? a.memberId : null;
      const now = Date.now();
      const id = ulid(now);
      const token = newMachineToken();
      const labels = [...new Set(req.labels ?? [])].sort();
      await env.DB.batch([
        env.DB.prepare(
          "INSERT INTO machines (id, name, labels, token_hash, created_by, created_at, owner_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
        ).bind(id, req.name, JSON.stringify(labels), await sha256Hex(token), a.memberId, now, ownerId),
        auditStatement(
          env.DB,
          a,
          { action: "machine.create", target: id, details: { name: req.name, labels, ...(personal ? { personal: true } : {}) } },
          now,
        ),
      ]);
      const machine = toMachine(
        {
          id,
          name: req.name,
          labels,
          revoked: false,
          connected: false,
          runningRuns: [],
          ...(ownerId ? { ownerId, personalBots: new Map() } : {}),
        },
        a.memberId,
        now,
        now,
      );
      return { machine: ownerId ? { ...machine, personalBots: [] } : machine, token };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machines",
    auth: "member",
    async handler({ env, auth }) {
      return visibleMachines(env, auth as AuthContext);
    },
  });

  registerRoute({
    method: "PATCH",
    path: "/v1/machines/:id",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const req = await body(UpdateMachineRequest);
      const a = auth as AuthContext;
      const m = await requireMachineManager(env.DB, a, params.id);
      const now = Date.now();
      const stmts: D1PreparedStatement[] = [];
      const details: Record<string, unknown> = {};
      // An ownership change re-checks the owner it was decided on in every statement, with the
      // owner UPDATE last: of two concurrent changes only the first wins, the other changes nothing.
      let guard = "";
      let guardArgs: unknown[] = [];
      if (req.owner === null && m.owner_id) {
        // 交给团队: only an owner who is an admin; the checked AI members were his own choice.
        if (a.member.level !== "owner") throw new ApiError("forbidden", "only an admin gives a machine to the team", "machine.team_admin_only");
        guard = " AND EXISTS (SELECT 1 FROM machines WHERE id = ? AND owner_id = ?)";
        guardArgs = [m.id, a.memberId];
      } else if (req.owner === "me" && !m.owner_id) {
        // 改成我的电脑 (requireMachineManager: an admin, and a person: only people have personal machines).
        if (a.member.kind !== "human") throw new ApiError("forbidden", "only people have personal machines");
        guard = " AND EXISTS (SELECT 1 FROM machines WHERE id = ? AND owner_id IS NULL)";
        guardArgs = [m.id];
      }
      if (req.name !== undefined && req.name !== m.name) {
        stmts.push(env.DB.prepare(`UPDATE machines SET name = ? WHERE id = ?${guard}`).bind(req.name, m.id, ...guardArgs));
        details.name = req.name;
      }
      if (req.owner === null && m.owner_id) {
        stmts.push(
          env.DB.prepare(`DELETE FROM machine_bots WHERE machine_id = ?${guard}`).bind(m.id, ...guardArgs),
          env.DB.prepare("UPDATE machines SET owner_id = NULL WHERE id = ? AND owner_id = ?").bind(m.id, a.memberId),
        );
        details.owner = null;
      } else if (req.owner === "me" && !m.owner_id) {
        // AI members someone else placed here are unpinned; my work for them keeps running here
        // through a checked row.
        stmts.push(
          env.DB.prepare(
            `INSERT OR IGNORE INTO machine_bots (machine_id, bot_id, repo_path, created_at)
             SELECT ?, r.member_id, NULL, ? FROM bot_runners r JOIN members b ON b.id = r.member_id
             WHERE r.machine_id = ? AND r.updated_by IS NOT ? AND b.deactivated = 0${guard}`,
          ).bind(m.id, now, m.id, a.memberId, ...guardArgs),
          env.DB.prepare(`UPDATE bot_runners SET machine_id = NULL WHERE machine_id = ? AND updated_by IS NOT ?${guard}`).bind(
            m.id,
            a.memberId,
            ...guardArgs,
          ),
          env.DB.prepare("UPDATE machines SET owner_id = ? WHERE id = ? AND owner_id IS NULL").bind(a.memberId, m.id),
        );
        details.owner = "me";
      }
      if (stmts.length > 0) {
        const results = await env.DB.batch(stmts);
        if (guard && !results[results.length - 1].meta.changes) {
          throw new ApiError("conflict", "the machine's owner changed meanwhile");
        }
        await auditStatement(env.DB, a, { action: "machine.update", target: m.id, details }, now).run();
      }
      return oneMachine(env, a, m.id);
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/machines/:id",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      await requireMachineManager(env.DB, a, params.id);
      const removed = await revokeMachines(env, a, [params.id]);
      if (removed.length === 0) throw new ApiError("not_found", "machine not found");
      return { ok: true };
    },
  });

  // "My work for this AI member runs on my machine" (personal machines only, by the owner).
  registerRoute({
    method: "PUT",
    path: "/v1/machines/:id/bots/:botId",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const req = await body(SetMachineBotRequest);
      const a = auth as AuthContext;
      await requirePersonalOwner(env.DB, a, params.id);
      const bot = await getMember(env.DB, params.botId);
      if (!bot || bot.kind !== "bot" || bot.deactivated) throw new ApiError("not_found", "bot not found");
      if (!(await loadRunner(env.DB, bot.id))) throw new ApiError("invalid", "the AI member does not run on machines");
      const n = await env.DB.prepare(
        "SELECT count(*) AS n, coalesce(sum(bot_id = ?), 0) AS has FROM machine_bots WHERE machine_id = ?",
      )
        .bind(bot.id, params.id)
        .first<{ n: number; has: number }>();
      if (n && !n.has && n.n >= MAX_PERSONAL_BOTS) throw new ApiError("conflict", `at most ${MAX_PERSONAL_BOTS} AI members on one machine`);
      const now = Date.now();
      // repoPath: absent keeps the stored folder, null clears it.
      const keep = req.repoPath === undefined ? 1 : 0;
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO machine_bots (machine_id, bot_id, repo_path, created_at) VALUES (?, ?, ?, ?)
           ON CONFLICT (machine_id, bot_id) DO UPDATE SET
             repo_path = CASE WHEN ? = 1 THEN machine_bots.repo_path ELSE excluded.repo_path END`,
        ).bind(params.id, bot.id, req.repoPath ?? null, now, keep),
        // The folder itself is the owner's local path: the audit says only whether one is set.
        auditStatement(
          env.DB,
          a,
          { action: "machine.bots.set", target: params.id, details: { botId: bot.id, ...(keep ? {} : { repoPath: req.repoPath !== null }) } },
          now,
        ),
      ]);
      return oneMachine(env, a, params.id);
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/machines/:id/bots/:botId",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      await requirePersonalOwner(env.DB, a, params.id);
      const res = await env.DB.prepare("DELETE FROM machine_bots WHERE machine_id = ? AND bot_id = ?").bind(params.id, params.botId).run();
      if (res.meta.changes) {
        await auditStatement(env.DB, a, { action: "machine.bots.delete", target: params.id, details: { botId: params.botId } }).run();
      }
      return oneMachine(env, a, params.id);
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
      // design-takeover 3.6: a person's command output only for that person and admins.
      const a = auth as AuthContext;
      const page = await deskStub(env, row.bot_id).events(row.bot_id, row.id, afterSeq, n, { id: a.memberId, admin: a.member.level === "owner" });
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

  // design-relay R2: any person in the group stops a whole piece of work (every run of its chain).
  registerRoute({
    method: "POST",
    path: "/v1/groups/:id/chains/:chainId/stop",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      if (a.member.kind !== "human") throw new ApiError("forbidden", "only people stop a piece of work");
      if (!(await isGroupMember(env.DB, params.id, a.memberId))) throw new ApiError("not_found", "group not found");
      if (!/^[0-9A-Za-z]{1,64}$/.test(params.chainId)) throw new ApiError("not_found", "piece of work not found");
      const actor = { id: a.memberId, kind: a.member.kind, level: a.member.level, tokenId: a.tokenId };
      const res = ChainStopResponse.parse(unwrap(await roomStub(env, params.id).rpc(params.id, "bots.chainStop", actor, { chainId: params.chainId })));
      await auditStatement(env.DB, a, { action: "chain.stop", target: params.chainId, details: { groupId: params.id, stoppedRuns: res.stoppedRuns } }).run();
      return res;
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
      // An admin may hand back for the person holding it (design-takeover 3.2): the audit says for whom.
      const before = await deskStub(env, row.bot_id).getRun(row.bot_id, row.id);
      const run = unwrap(await deskStub(env, row.bot_id).resume(row.bot_id, row.id, who, req.note));
      const holder = before?.pausedBy && before.pausedBy !== a.memberId ? { for: before.pausedBy } : {};
      // The note itself goes to the CLI, not into the audit log.
      await auditStatement(env.DB, a, { action: "run.resume", target: row.id, details: { botId: row.bot_id, note: Boolean(req.note), ...(before?.holdKind ? { holdKind: before.holdKind } : {}), ...holder } }).run();
      return run;
    },
  });

  // design-takeover 3.2, 3.6: take over a run (the CLI stops; the person runs commands in its work folder).
  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/takeover",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const run = unwrap(await deskStub(env, row.bot_id).takeover(row.bot_id, row.id, who));
      await auditStatement(env.DB, a, { action: "run.takeover", target: row.id, details: { botId: row.bot_id } }).run();
      return run;
    },
  });

  // design-takeover 3.6 (T3): the person holding a take-over runs one command at a time.
  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/commands",
    auth: "member",
    async handler({ env, params, auth, body }) {
      const a = auth as AuthContext;
      const req = await body(RunCommandRequest);
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const res = unwrap(await deskStub(env, row.bot_id).runCommand(row.bot_id, row.id, who, req.command));
      await auditStatement(env.DB, a, {
        action: "run.command",
        target: row.id,
        details: { botId: row.bot_id, commandId: res.commandId, command: auditValue(req.command) },
      }).run();
      return res;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/runs/:id/commands/:commandId/cancel",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      const row = await visibleRun(env.DB, a, params.id);
      const who = { id: a.memberId, kind: a.member.kind, level: a.member.level } as TakeoverActor;
      const res = unwrap(await deskStub(env, row.bot_id).cancelCommand(row.bot_id, row.id, who, params.commandId));
      await auditStatement(env.DB, a, { action: "run.command.cancel", target: row.id, details: { botId: row.bot_id, commandId: params.commandId } }).run();
      return res;
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
