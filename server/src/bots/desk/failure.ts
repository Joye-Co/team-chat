// Why a run failed, in the topic (模 2.2, 3.1, 3.5; 接 5.3; 盯 1.13; 机 2.5, 5.2): one sentence for
// the kind the daemon sorted it into and one next step. A computer whose CLI is not signed in
// names the computer; the requester is pointed to its sign-in when they may manage it, and the
// people who may are mentioned otherwise.
import { teamZone } from "../../memory/consolidate";
import { type FailureKind, type SignInBy, TEXT, cliName, failureWords } from "../dispatch";
import type { BotsEnv } from "../env";
import { linkStub } from "../env";

/** A computer the note is about. */
export interface FailedOn {
  id: string;
  name: string;
  /** A personal computer's owner; absent = the team's. */
  ownerId?: string;
  /** The CLI kind of the runtime there (`CliInfo.kind`), when known. */
  kind?: string;
}

/** The text of the note and the people it mentions. */
export interface FailureNote {
  text: string;
  mentions: string[];
}

/** The computer of a run from D1 and its link (the CLI kind of `runtime`). */
export async function failedOn(env: BotsEnv, machineId: string, runtime: string | null): Promise<FailedOn | null> {
  const row = await env.DB.prepare("SELECT id, name, owner_id FROM machines WHERE id = ?")
    .bind(machineId)
    .first<{ id: string; name: string; owner_id: string | null }>()
    .catch(() => null);
  if (!row) return null;
  let kind: string | undefined;
  try {
    kind = (await linkStub(env, machineId).status()).clis?.find((c) => c.runtime === runtime)?.kind;
  } catch {
    // The kind only names the CLI; the runtime's own name stands in.
  }
  return { id: row.id, name: row.name, ...(row.owner_id ? { ownerId: row.owner_id } : {}), ...(kind ? { kind } : {}) };
}

/**
 * Who may sign the computers' CLI in again, as the requester reads it: the requester when they
 * may manage every one of them (a personal computer's owner; an admin for a team computer), else
 * those who may (their names, and the ones in the group are mentioned).
 */
async function signInBy(env: BotsEnv, machines: FailedOn[], requesterId: string, groupId: string): Promise<{ by: SignInBy; mentions: string[] }> {
  const requester = await env.DB.prepare("SELECT level FROM members WHERE id = ? AND deactivated = 0")
    .bind(requesterId)
    .first<{ level: string }>()
    .catch(() => null);
  const manages = (m: FailedOn) => (m.ownerId ? m.ownerId === requesterId : requester?.level === "owner");
  if (machines.length > 0 && machines.every(manages)) return { by: { self: true }, mentions: [] };
  const owners = [...new Set(machines.filter((m) => m.ownerId).map((m) => m.ownerId as string))];
  const admins = machines.some((m) => !m.ownerId);
  const rows = await env.DB.prepare(
    `SELECT m.id, m.name, EXISTS (SELECT 1 FROM group_members g WHERE g.group_id = ? AND g.member_id = m.id) AS here
     FROM members m WHERE m.kind = 'human' AND m.deactivated = 0
       AND (m.id IN (SELECT value FROM json_each(?)) OR (? AND m.level = 'owner'))
     ORDER BY here DESC, m.id LIMIT 20`,
  )
    .bind(groupId, JSON.stringify(owners), admins ? 1 : 0)
    .all<{ id: string; name: string; here: number }>()
    .catch(() => ({ results: [] as { id: string; name: string; here: number }[] }));
  const all = rows.results.filter((p) => p.id !== requesterId);
  // The ones in the group read the topic and are mentioned; without any, up to three by name.
  const here = all.filter((p) => p.here);
  const people = here.length > 0 ? here.slice(0, 5) : all.slice(0, 3);
  return { by: { names: people.map((p) => p.name) }, mentions: here.slice(0, 5).map((p) => p.id) };
}

/**
 * The note for a run that failed with `kind` (`started`), or that did not start because every
 * computer with the runtime is signed out (`started: false`, `kind` "signed_out").
 */
export async function failureNote(
  env: BotsEnv,
  o: {
    kind: FailureKind;
    machines: FailedOn[];
    runtime: string;
    requesterId: string;
    groupId: string;
    resetsAt?: number;
    started: boolean;
    now?: number;
  },
): Promise<FailureNote> {
  const now = o.now ?? Date.now();
  const cli = cliName(o.machines.find((m) => m.kind)?.kind, o.runtime);
  const sign: { by: SignInBy; mentions: string[] } =
    o.kind === "signed_out" ? await signInBy(env, o.machines, o.requesterId, o.groupId) : { by: { names: [] }, mentions: [] };
  const words = failureWords(o.kind, { machines: o.machines.map((m) => m.name), cli, resetsAt: o.resetsAt, now, zone: teamZone(env), signIn: sign.by });
  const text = o.started ? TEXT.failedWith(words.sentence, words.next) : TEXT.notStartedWith(words.sentence, words.next);
  return { text, mentions: sign.mentions };
}
