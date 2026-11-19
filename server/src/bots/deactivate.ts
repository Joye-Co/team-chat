// A deactivated person's personal machines are removed (docs/plans/usable/design-roles.zh.md 3.5):
// the same as `DELETE /v1/machines/:id` for each, audited as the person who deactivated him.
import type { AuthContext } from "../core/auth";
import { auditStatement } from "../core/audit";
import { type Env, gatewayStub } from "../core/env";
import { afterDeactivate, notifyMembersChanged, revokeTokens } from "../core/routes";
import { type MemberRow, getMember } from "../core/db";
import { registerMemberDeactivated } from "../core/registry";
import { revokeMachines } from "./store";

/** Who deactivated the member: the latest audit of it (written before the hooks run). */
async function deactivatedBy(env: Env, memberId: string) {
  const row = await env.DB.prepare(
    `SELECT actor_id, credential_id FROM audit
     WHERE target = ? AND (action = 'member.deactivate' OR (action = 'member.update' AND json_extract(details, '$.deactivated') IS NOT NULL))
     ORDER BY id DESC LIMIT 1`,
  )
    .bind(memberId)
    .first<{ actor_id: string | null; credential_id: string | null }>();
  return row?.actor_id ? { memberId: row.actor_id, tokenId: row.credential_id ?? "" } : null;
}

export async function revokePersonalMachines(
  env: Env,
  memberId: string,
  opts: { closeLink?: (id: string) => Promise<unknown> } = {},
): Promise<string[]> {
  const rows = await env.DB.prepare("SELECT id FROM machines WHERE owner_id = ? AND revoked_at IS NULL ORDER BY id")
    .bind(memberId)
    .all<{ id: string }>();
  if (rows.results.length === 0) return [];
  return revokeMachines(
    env,
    await deactivatedBy(env, memberId),
    rows.results.map((r) => r.id),
    opts,
  );
}

/**
 * design-personal-ai: a deactivated person's own AI members are deactivated too, as if he who
 * deactivated the person did it (audited as `member.update`).
 */
export async function deactivateOwnBots(env: Env, memberId: string): Promise<string[]> {
  const rows = await env.DB.prepare("SELECT id FROM members WHERE owner_id = ? AND kind = 'bot' AND deactivated = 0 ORDER BY id")
    .bind(memberId)
    .all<{ id: string }>();
  if (rows.results.length === 0) return [];
  const by = await deactivatedBy(env, memberId);
  const now = Date.now();
  const done: string[] = [];
  for (const { id } of rows.results) {
    const r = await env.DB.batch([
      env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ? AND deactivated = 0").bind(id),
      auditStatement(env.DB, by, { action: "member.update", target: id, details: { deactivated: [false, true], ownerDeactivated: memberId } }, now),
    ]);
    if (!r[0].meta.changes) continue;
    done.push(id);
    await revokeTokens(env, (by ?? { memberId, tokenId: "" }) as AuthContext, id, "all");
    await gatewayStub(env, id).closeSockets({ reason: "member deactivated" });
    await afterDeactivate(env, id);
  }
  const changed = (await Promise.all(done.map((id) => getMember(env.DB, id)))).filter((m): m is MemberRow => m !== null);
  await notifyMembersChanged(env, () => undefined, changed);
  return done;
}

registerMemberDeactivated(async (env, memberId) => {
  await revokePersonalMachines(env, memberId);
  await deactivateOwnBots(env, memberId);
});
