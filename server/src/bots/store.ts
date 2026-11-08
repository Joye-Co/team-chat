// D1 access of the bots feature: runner configuration, machines and machine tokens.
import type { BotRunner } from "@chat/protocol";
import { bearerToken } from "../core/auth";
import { sha256Hex } from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { type MachineView, mergeLabels } from "./dispatch";
import { linkStub } from "./env";

export interface RunnerRow {
  member_id: string;
  runtime: string;
  labels: string;
  repo_url: string | null;
  repo_path: string | null;
  base_branch: string | null;
  model: string | null;
  env_allowlist: string;
  enabled: number;
  updated_at: number;
  /** 1.2.2 (migration 0010); undefined on a database before it. */
  machine_id?: string | null;
  /** Migration 0011; undefined on a database before it. */
  effort?: string | null;
}

export function toRunner(r: RunnerRow): BotRunner {
  const out: BotRunner = {
    botId: r.member_id,
    runtime: r.runtime,
    labels: JSON.parse(r.labels) as string[],
    envAllowlist: JSON.parse(r.env_allowlist) as string[],
    enabled: r.enabled === 1,
    updatedAt: r.updated_at,
  };
  if (r.repo_url) out.repo = { url: r.repo_url };
  else if (r.repo_path) out.repo = { path: r.repo_path };
  if (r.base_branch) out.baseBranch = r.base_branch;
  if (r.model) out.model = r.model;
  if (r.machine_id) out.machineId = r.machine_id;
  if (r.effort) out.effort = r.effort;
  return out;
}

export async function loadRunner(db: D1Database, botId: string): Promise<BotRunner | null> {
  const r = await db.prepare("SELECT * FROM bot_runners WHERE member_id = ?").bind(botId).first<RunnerRow>();
  return r ? toRunner(r) : null;
}

export interface MachineRow {
  id: string;
  name: string;
  labels: string;
  created_by: string;
  created_at: number;
  revoked_at: number | null;
  last_seen_at: number | null;
}

/** D1 rows joined with each link's live status. */
export async function loadMachineViews(env: Env, opts: { includeRevoked?: boolean } = {}) {
  const rows = await env.DB.prepare(
    `SELECT id, name, labels, created_by, created_at, revoked_at, last_seen_at FROM machines
     ${opts.includeRevoked ? "" : "WHERE revoked_at IS NULL"} ORDER BY created_at, id`,
  ).all<MachineRow>();
  return Promise.all(
    rows.results.map(async (row) => {
      const owner = JSON.parse(row.labels) as string[];
      const base: MachineView = {
        id: row.id,
        name: row.name,
        labels: owner,
        revoked: row.revoked_at !== null,
        connected: false,
        runningRuns: [],
      };
      if (row.revoked_at !== null) return { row, view: base };
      try {
        const s = await linkStub(env, row.id).status();
        const view: MachineView = {
          ...base,
          labels: mergeLabels(owner, s.info?.labels),
          connected: s.connected,
          lastHeartbeatAt: s.lastHeartbeatAt,
          info: s.info,
          health: s.health,
          clis: s.clis,
          runningRuns: s.runningRuns,
        };
        return { row, view };
      } catch {
        return { row, view: base };
      }
    }),
  );
}

/** Machines for routing runs: only the labels an owner gave count (daemon labels are shown only). */
export async function loadMachines(env: Env): Promise<MachineView[]> {
  return (await loadMachineViews(env)).map(({ row, view }) => ({ ...view, labels: JSON.parse(row.labels) as string[] }));
}

export const MACHINE_TOKEN_PATTERN = /^tm_[A-Za-z0-9_-]{43}$/;

function base64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** `tm_` + 43 base64url characters (32 random bytes). */
export function newMachineToken(): string {
  return `tm_${base64url(crypto.getRandomValues(new Uint8Array(32)))}`;
}

/** The machine of a bearer machine token; `unauthorized` when missing, unknown or revoked. */
export async function authenticateMachine(request: Request, db: D1Database): Promise<MachineRow> {
  const token = bearerToken(request);
  if (!token || !MACHINE_TOKEN_PATTERN.test(token)) throw new ApiError("unauthorized", "invalid machine token");
  const row = await db
    .prepare(
      "SELECT id, name, labels, created_by, created_at, revoked_at, last_seen_at FROM machines WHERE token_hash = ?",
    )
    .bind(await sha256Hex(token))
    .first<MachineRow>();
  if (!row || row.revoked_at !== null) throw new ApiError("unauthorized", "invalid machine token");
  return row;
}

export interface RunIndexRow {
  id: string;
  bot_id: string;
  group_id: string;
  topic_id: string;
  machine_id: string | null;
  created_at: number;
}

export async function runIndex(db: D1Database, runId: string): Promise<RunIndexRow | null> {
  return db.prepare("SELECT * FROM bot_runs WHERE id = ?").bind(runId).first<RunIndexRow>();
}
