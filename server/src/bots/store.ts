// D1 access of the bots feature: runner configuration, machines and machine tokens.
import type { BotRunner } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import { type AuthContext, bearerToken } from "../core/auth";
import { sha256Hex } from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { logger } from "../core/logger";
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
  /** Usable (migration 0017_machine_owner): the person whose own machine it is; NULL = a team machine. */
  owner_id?: string | null;
  /** Usable (migration 0020_personal_ai): the VM size an admin set; NULL = the daemon's own. */
  vm_vcpus?: number | null;
  vm_mem_mib?: number | null;
}

/** D1 rows joined with each link's live status. */
export async function loadMachineViews(env: Env, opts: { includeRevoked?: boolean } = {}) {
  const [rows, checked] = await Promise.all([
    env.DB.prepare(
      `SELECT id, name, labels, created_by, created_at, revoked_at, last_seen_at, owner_id, vm_vcpus, vm_mem_mib FROM machines
       ${opts.includeRevoked ? "" : "WHERE revoked_at IS NULL"} ORDER BY created_at, id`,
    ).all<MachineRow>(),
    env.DB.prepare("SELECT machine_id, bot_id, repo_path FROM machine_bots ORDER BY created_at, bot_id").all<{
      machine_id: string;
      bot_id: string;
      repo_path: string | null;
    }>(),
  ]);
  const checkedBots = new Map<string, Map<string, string | null>>();
  for (const c of checked.results) {
    let m = checkedBots.get(c.machine_id);
    if (!m) checkedBots.set(c.machine_id, (m = new Map()));
    m.set(c.bot_id, c.repo_path);
  }
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
      if (row.owner_id) {
        base.ownerId = row.owner_id;
        base.personalBots = checkedBots.get(row.id) ?? new Map();
      }
      if (row.vm_vcpus && row.vm_mem_mib) base.vm = { vcpus: row.vm_vcpus, memMib: row.vm_mem_mib };
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
          ...(s.signedOut ? { signedOut: s.signedOut } : {}),
          ...(s.limited ? { limited: s.limited } : {}),
          // design-github-identity 3.6: on the host git may sign in some other way (SSH, the
          // keychain), so only a sign-in is news there; in VMs the host's sign-in is the only way.
          ...(s.github && (s.info?.runsIn === "vm" || s.github.state === "signed_in") ? { github: s.github } : {}),
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

/** Who may manage a machine: an admin for a team machine, only its owner for a personal one. */
export function mayManageMachine(auth: Pick<AuthContext, "memberId" | "member">, ownerId: string | null | undefined): boolean {
  return ownerId ? auth.memberId === ownerId : auth.member.level === "owner";
}

export interface ManagedMachine {
  id: string;
  name: string;
  owner_id: string | null;
  revoked_at: number | null;
}

/**
 * The machine, when the caller may manage it (`mayManageMachine`): `not_found` for an unknown
 * (or, unless `includeRevoked`, a removed) machine, `forbidden` with `machine.not_yours` otherwise.
 */
/** A machine that is not removed, whoever asks (the caller checks who may change what). */
export async function requireMachineRow(db: D1Database, machineId: string): Promise<ManagedMachine> {
  const row = await db
    .prepare("SELECT id, name, owner_id, revoked_at FROM machines WHERE id = ?")
    .bind(machineId)
    .first<ManagedMachine>();
  if (!row || row.revoked_at !== null) throw new ApiError("not_found", "machine not found");
  return row;
}

export async function requireMachineManager(
  db: D1Database,
  auth: Pick<AuthContext, "memberId" | "member">,
  machineId: string,
  opts: { includeRevoked?: boolean } = {},
): Promise<ManagedMachine> {
  const row = await db
    .prepare("SELECT id, name, owner_id, revoked_at FROM machines WHERE id = ?")
    .bind(machineId)
    .first<ManagedMachine>();
  if (!row || (row.revoked_at !== null && !opts.includeRevoked)) throw new ApiError("not_found", "machine not found");
  if (!mayManageMachine(auth, row.owner_id)) {
    throw new ApiError(
      "forbidden",
      row.owner_id ? "only its owner manages a personal machine" : "only an admin manages a team machine",
      "machine.not_yours",
    );
  }
  return row;
}

/**
 * Removes machines (`DELETE /v1/machines/:id`, and a deactivated person's personal machines):
 * one batch sets `revoked_at` (the token stops working at once), unpins the AI members placed on
 * them, drops their checked AI members and audits `machine.revoke`; then each link is closed,
 * retried once, and a link that still fails is logged (the daemon's next request is refused
 * anyway). Returns the ids removed by this call.
 */
export async function revokeMachines(
  env: Env,
  actor: Pick<AuthContext, "memberId" | "tokenId"> | null,
  machineIds: string[],
  opts: { now?: number; closeLink?: (id: string) => Promise<unknown> } = {},
): Promise<string[]> {
  if (machineIds.length === 0) return [];
  const now = opts.now ?? Date.now();
  const stmts: D1PreparedStatement[] = [];
  for (const id of machineIds) {
    stmts.push(
      env.DB.prepare("UPDATE machines SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL").bind(now, id),
      // Bots pinned to it go back to any machine with their labels.
      env.DB.prepare("UPDATE bot_runners SET machine_id = NULL WHERE machine_id = ?").bind(id),
      env.DB.prepare("DELETE FROM machine_bots WHERE machine_id = ?").bind(id),
      auditStatement(env.DB, actor, { action: "machine.revoke", target: id }, now),
    );
  }
  const res = await env.DB.batch(stmts);
  const revoked = machineIds.filter((_, i) => (res[i * 4].meta.changes ?? 0) > 0);
  const close = opts.closeLink ?? ((id: string) => linkStub(env, id).revoke(id));
  for (const id of revoked) {
    try {
      await close(id);
    } catch {
      try {
        await close(id);
      } catch (err) {
        logger.warn("closing a removed machine's link failed", { machineId: id, err });
      }
    }
  }
  return revoked;
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
