// Whose GitHub a run uses and how (design-github-identity 3.2, unit G3): the identity person, the
// machine's mode ("app": git and gh ask for the person's token; "machine": the machine's own
// sign-in) and the `run.assign.github` field. Only for a daemon with feature `github` that reports
// `runsIn`; any other daemon gets no field and works as before.
import { MACHINE_FEATURE_GITHUB, type RunGithub } from "@chat/protocol";
import { type MachineView, hasFeature } from "../bots/dispatch";
import type { Env } from "../core/env";
import { type AccountRow, loadAccount, unavailableReason } from "./store";

/** Who a run works for, as the desk knows it. */
export interface RunPerson {
  /** `runs.requested_for`: the person the run works for, or null (a hook, an older run). */
  requestedFor: string | null;
  /** `runs.requested_by`: the author of the message that started it ("anonymous" for a private card answer). */
  requestedBy: string;
  botId: string;
}

type MemberRow = { id: string; kind: string; deactivated: number; created_by: string | null };

async function member(db: D1Database, id: string): Promise<MemberRow | null> {
  return db.prepare("SELECT id, kind, deactivated, created_by FROM members WHERE id = ?").bind(id).first<MemberRow>();
}

/** The earliest active human admin (`level = 'owner'`, which includes 所有者), or null. */
export async function earliestAdmin(db: D1Database): Promise<string | null> {
  const row = await db
    .prepare("SELECT id FROM members WHERE level = 'owner' AND kind = 'human' AND deactivated = 0 ORDER BY created_at, id LIMIT 1")
    .first<{ id: string }>();
  return row?.id ?? null;
}

/**
 * The identity person (3.2), first rule that hits: the person the run works for (a run from
 * before `requested_for` uses its requester when that is a person); for a hook's run the person
 * who made the hook; else the person who added the AI member. Someone who left the team (or is no
 * person) is replaced by the earliest active human admin.
 */
export async function identityPerson(db: D1Database, x: RunPerson): Promise<string | null> {
  let id: string | null = x.requestedFor;
  if (!id) {
    const by = await member(db, x.requestedBy);
    if (by?.kind === "human") id = by.id;
    else if (by) {
      // A hook posts as its own bot member: the hook's maker answers for it.
      const hook = await db
        .prepare("SELECT created_by FROM hooks WHERE member_id = ? ORDER BY deleted_at IS NOT NULL, created_at DESC LIMIT 1")
        .bind(by.id)
        .first<{ created_by: string }>();
      if (hook) id = hook.created_by;
    }
  }
  if (!id) id = (await member(db, x.botId))?.created_by ?? null;
  const person = id ? await member(db, id) : null;
  if (person && person.kind === "human" && !person.deactivated) return person.id;
  return earliestAdmin(db);
}

/** A machine for one person: their own personal machine, another person's, or a team machine. */
export type MachineKind = "own" | "other" | "team";

export function machineKind(ownerId: string | null | undefined, memberId: string): MachineKind {
  if (!ownerId) return "team";
  return ownerId === memberId ? "own" : "other";
}

/**
 * The 3.2 table. Runs on the host of the person's own personal machine use that machine's sign-in
 * until they connect; another person's personal machine always uses its own sign-in; a team
 * machine and any VM use the person's connected GitHub. An unknown `runsIn` counts as a VM.
 */
export function sourceFor(kind: MachineKind, runsIn: string, connected: boolean): "app" | "machine" {
  if (runsIn !== "host") return "app";
  if (kind === "team") return "app";
  if (kind === "other") return "machine";
  return connected ? "app" : "machine";
}

/** May this machine get the person's token at all (the cells of the table that are not "machine" when connected)? */
export function mayUseToken(kind: MachineKind, runsIn: string | undefined): boolean {
  return !(kind === "other" && runsIn === "host");
}

const cut = (s: string, n: number) => [...s].slice(0, n).join("");

/** The git committer: the AI member, with an address that is never a real mailbox. */
export function committerOf(botId: string, botName: string) {
  return { name: cut(botName.trim() || "AI", 200), email: `ai-${botId}@users.noreply.invalid` };
}

/** The git author: the person's GitHub identity when connected, else their name in the team. */
export function authorOf(memberId: string, memberName: string, account: AccountRow | null) {
  if (account && account.status === "ok") {
    return { name: cut(account.name?.trim() || account.login, 200), email: `${account.github_user_id}+${account.login}@users.noreply.github.com` };
  }
  return { name: cut(memberName.trim() || memberId, 200), email: `${memberId}@users.noreply.invalid` };
}

/**
 * `run.assign.github` for a run going to `machine`, with the identity person; null when the
 * machine does not take the field (no feature `github`, no `runsIn`), nobody can answer for the
 * run, or the field would ask for a token the server cannot keep (GitHub sign-in not set up: team
 * machines and VMs then run as before).
 */
export async function githubFor(env: Env, x: RunPerson, machine: MachineView): Promise<{ github: RunGithub; memberId: string } | null> {
  if (!hasFeature(machine, MACHINE_FEATURE_GITHUB)) return null;
  const runsIn = machine.info?.runsIn;
  if (!runsIn) return null;
  const memberId = await identityPerson(env.DB, x);
  if (!memberId) return null;
  const available = unavailableReason(env) === null;
  const account = available ? await loadAccount(env.DB, memberId) : null;
  const connected = account?.status === "ok";
  const kind = machineKind(machine.ownerId, memberId);
  const source = sourceFor(kind, runsIn, connected);
  if (source === "app" && !available) return null;
  const names = await env.DB.prepare("SELECT id, name FROM members WHERE id IN (?, ?)").bind(memberId, x.botId).all<{ id: string; name: string }>();
  const name = (id: string) => names.results.find((r) => r.id === id)?.name ?? "";
  const github: RunGithub = { source, memberId, committer: committerOf(x.botId, name(x.botId)) };
  if (source === "app") {
    if (connected && account) github.login = account.login;
    // On the person's own machine (host) git keeps that machine's config; elsewhere the author
    // is given and SSH is rewritten to HTTPS, so the host's own key is never used.
    if (!(kind === "own" && runsIn === "host")) {
      github.author = authorOf(memberId, name(memberId), account);
      github.rewriteSsh = true;
    }
  }
  return { github, memberId };
}
