// 成员 of 团队管理 (docs/protocol.md "Team roster"): for owners, when each member joined and who
// added them, when they were last seen, their signed-in devices, and invites not accepted yet.
import type { TeamRoster } from "@chat/protocol";
import { registerRoute } from "../core/registry";

registerRoute({
  method: "GET",
  path: "/v1/team/roster",
  auth: "owner",
  async handler({ env }) {
    const now = Date.now();
    const [members, tokens, invites] = await env.DB.batch([
      env.DB.prepare("SELECT id, created_at, created_by FROM members ORDER BY created_at, id"),
      env.DB.prepare(
        "SELECT member_id, label, created_at, last_used_at FROM tokens WHERE revoked_at IS NULL ORDER BY COALESCE(last_used_at, created_at) DESC",
      ),
      env.DB.prepare(
        "SELECT id, name, kind, level, role, created_by, created_at, expires_at FROM invites WHERE used_at IS NULL AND expires_at > ? ORDER BY created_at DESC LIMIT 200",
      ).bind(now),
    ]);
    const byMember = new Map<string, { label: string; created_at: number; last_used_at: number | null }[]>();
    for (const t of tokens.results as { member_id: string; label: string; created_at: number; last_used_at: number | null }[]) {
      const list = byMember.get(t.member_id) ?? [];
      list.push(t);
      byMember.set(t.member_id, list);
    }
    const out: TeamRoster = {
      members: (members.results as { id: string; created_at: number; created_by: string | null }[]).map((m) => {
        const devices = byMember.get(m.id) ?? [];
        const seen = devices.reduce<number | undefined>((a, d) => (d.last_used_at && (!a || d.last_used_at > a) ? d.last_used_at : a), undefined);
        return {
          memberId: m.id,
          joinedAt: m.created_at,
          ...(m.created_by ? { addedBy: m.created_by } : {}),
          ...(seen ? { lastSeenAt: seen } : {}),
          devices: devices.slice(0, 50).map((d) => ({
            label: d.label.slice(0, 200),
            createdAt: d.created_at,
            ...(d.last_used_at ? { lastUsedAt: d.last_used_at } : {}),
          })),
        };
      }),
      invites: (invites.results as { id: string; name: string; kind: "human" | "bot"; level: "owner" | "member"; role: string | null; created_by: string; created_at: number; expires_at: number }[]).map((i) => ({
        id: i.id,
        name: i.name,
        kind: i.kind,
        level: i.level,
        ...(i.role ? { role: i.role } : {}),
        createdBy: i.created_by,
        createdAt: i.created_at,
        expiresAt: i.expires_at,
      })),
    };
    return out;
  },
});
