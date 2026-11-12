// The `run.assign.github` field (design-github-identity 3.2): who a run works for (the identity
// person), who commits (the AI member) and who asked. Git and gh use the computer's own GitHub
// sign-in. Only for a daemon with feature `github` that reports `runsIn`; any other daemon gets no
// field and works as before.
import { MACHINE_FEATURE_GITHUB, type RunGithub } from "@chat/protocol";
import { type MachineView, hasFeature } from "../bots/dispatch";
import type { Env } from "../core/env";
import { ANONYMOUS_ID } from "../cards2/privacy";

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

const cut = (s: string, n: number) => [...s].slice(0, n).join("");

/** The git committer and author: the AI member, with an address that is never a real mailbox. */
export function committerOf(botId: string, botName: string) {
  return { name: cut(botName.trim() || "AI", 200), email: `ai-${botId}@users.noreply.invalid` };
}

/**
 * `run.assign.github` for a run going to `machine` (3.2): always the computer's own sign-in; the
 * commits name the AI member; `askedBy` names who asked, for the pull request body (not for a
 * private card's anonymous answer). Null when the machine does not take the field (no feature
 * `github`, no `runsIn`) or nobody can answer for the run. No GitHub account is looked up.
 */
export async function githubFor(env: Env, x: RunPerson, machine: MachineView): Promise<RunGithub | null> {
  if (!hasFeature(machine, MACHINE_FEATURE_GITHUB)) return null;
  if (!machine.info?.runsIn) return null;
  const memberId = await identityPerson(env.DB, x);
  if (!memberId) return null;
  const asker = x.requestedBy === ANONYMOUS_ID ? null : x.requestedBy;
  const names = await env.DB.prepare("SELECT id, name FROM members WHERE id IN (?, ?)")
    .bind(x.botId, asker ?? x.botId)
    .all<{ id: string; name: string }>();
  const name = (id: string) => names.results.find((r) => r.id === id)?.name ?? "";
  const ai = committerOf(x.botId, name(x.botId));
  const github: RunGithub = { source: "machine", memberId, committer: ai, author: ai };
  const askedBy = asker ? cut(name(asker).trim(), 100) : "";
  if (askedBy) github.askedBy = askedBy;
  return github;
}
