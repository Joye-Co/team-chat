// A deactivated person's personal machines are removed (docs/plans/usable/design-roles.zh.md 3.5):
// the same as `DELETE /v1/machines/:id` for each, audited as the person who deactivated him.
import type { Env } from "../core/env";
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

registerMemberDeactivated(async (env, memberId) => {
  await revokePersonalMachines(env, memberId);
});
