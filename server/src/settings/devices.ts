// My signed-in devices (docs/protocol.md, "Settings"): one per live token, named by the
// `deviceName` given at sign in. Signing out a device revokes its token and closes its sockets.
import type { Device } from "@chat/protocol";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { revokeTokens } from "../core/routes";

type TokenRow = { id: string; label: string; created_at: number; last_used_at: number | null };

registerRoute({
  method: "GET",
  path: "/v1/me/devices",
  auth: "member",
  async handler({ env, auth }) {
    const a = auth as AuthContext;
    const r = await env.DB.prepare(
      `SELECT id, label, created_at, last_used_at FROM tokens WHERE member_id = ? AND revoked_at IS NULL
       ORDER BY coalesce(last_used_at, created_at) DESC, id DESC LIMIT 100`,
    )
      .bind(a.memberId)
      .all<TokenRow>();
    const devices: Device[] = r.results.map((t) => ({
      id: t.id,
      name: t.label.slice(0, 128),
      createdAt: t.created_at,
      lastUsedAt: t.last_used_at,
      current: t.id === a.tokenId,
    }));
    return { devices };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/me/devices/:id",
  auth: "member",
  async handler({ env, auth, params }) {
    const a = auth as AuthContext;
    const row = await env.DB.prepare("SELECT id FROM tokens WHERE id = ? AND member_id = ? AND revoked_at IS NULL")
      .bind(params.id, a.memberId)
      .first<{ id: string }>();
    if (!row) throw new ApiError("not_found", "device not found");
    return { revoked: await revokeTokens(env, a, a.memberId, [row.id]) };
  },
});
