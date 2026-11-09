// The team's models for the executors (docs/protocol.md, "Worker models"; 设置 › 模型): one row
// per CLI runtime in D1 `worker_models`. BotDesk reads them when it assigns a run whose runner has
// no model or effort of its own. The front desk's model is the AI 服务 (settings/ai).
import { SetWorkerModelRequest, type WorkerModel, WorkerRuntime, type WorkerModelsResponse } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";

type Row = { runtime: string; model: string | null; effort: string | null; updated_at: number };

const view = (r: Row): WorkerModel => ({ runtime: r.runtime, model: r.model, effort: r.effort, updatedAt: r.updated_at });

export async function loadWorkerModels(db: D1Database): Promise<WorkerModel[]> {
  const rows = await db.prepare("SELECT runtime, model, effort, updated_at FROM worker_models ORDER BY runtime").all<Row>();
  return rows.results.map(view);
}

/** The team's choice for one runtime; null when unset or when D1 cannot be read (never blocks a run). */
export async function workerModel(db: D1Database, runtime: string): Promise<WorkerModel | null> {
  try {
    const r = await db.prepare("SELECT runtime, model, effort, updated_at FROM worker_models WHERE runtime = ?").bind(runtime).first<Row>();
    return r ? view(r) : null;
  } catch {
    return null;
  }
}

registerRoute({
  method: "GET",
  path: "/v1/settings/models",
  auth: "member",
  handler: async ({ env }): Promise<WorkerModelsResponse> => ({ workers: await loadWorkerModels(env.DB) }),
});

registerRoute({
  method: "PUT",
  path: "/v1/settings/models/:runtime",
  auth: "owner",
  async handler({ env, auth, body, params }) {
    const a = auth as AuthContext;
    const runtime = WorkerRuntime.safeParse(params.runtime);
    if (!runtime.success) throw new ApiError("invalid", "runtime is a lower-case runtime name");
    const req = await body(SetWorkerModelRequest);
    const before = await workerModel(env.DB, runtime.data);
    const now = Date.now();
    const after: WorkerModel = {
      runtime: runtime.data,
      model: req.model === undefined ? (before?.model ?? null) : req.model,
      effort: req.effort === undefined ? (before?.effort ?? null) : req.effort,
      updatedAt: now,
    };
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO worker_models (runtime, model, effort, updated_at, updated_by) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (runtime) DO UPDATE SET model = excluded.model, effort = excluded.effort, updated_at = excluded.updated_at, updated_by = excluded.updated_by`,
      ).bind(after.runtime, after.model, after.effort, now, a.memberId),
      auditStatement(env.DB, a, { action: "settings.models", target: after.runtime, details: { before: before ? { model: before.model, effort: before.effort } : null, after: { model: after.model, effort: after.effort } } }, now),
    ]);
    return after;
  },
});
