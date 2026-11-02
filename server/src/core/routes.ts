// Core HTTP routes of docs/protocol.md. Feature routes use the same `registerRoute`.
import {
  AddGroupMemberRequest,
  BootstrapRequest,
  CreateBotRequest,
  CreateDmRequest,
  CreateGroupRequest,
  CreateInviteRequest,
  type GroupSummary,
  RedeemRequest,
  RevokeTokensRequest,
} from "@chat/protocol";
import { auditStatement } from "./audit";
import type { AuthContext } from "./auth";
import { newInviteCode, newToken, normalizeInviteCode, sha256Hex, timingSafeEqualString } from "./crypto";
import { type MemberRow, getGroup, getMember, groupMemberIds, isGroupMember, listMembers, toMember } from "./db";
import { type Env, gatewayStub, roomStub } from "./env";
import { ApiError, unwrap } from "./errors";
import { ulid } from "./ids";
import { logger } from "./logger";
import { registerRoute } from "./registry";

export const INVITE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
export const DEFAULT_GROUP_NAME = "General";

type Wait = (p: Promise<unknown>) => void;

// ---------------------------------------------------------------------------
// Shared operations (also usable by feature modules)
// ---------------------------------------------------------------------------

/** Tells the gateways of these members that a group changed (directory frames). */
export function notifyGroupChanged(env: Env, waitUntil: Wait, groupId: string, memberIds: string[]): void {
  for (const id of new Set(memberIds)) {
    waitUntil(
      gatewayStub(env, id)
        .groupChanged(id, groupId)
        .catch((err) => logger.warn("groupChanged failed", { memberId: id, groupId, err })),
    );
  }
}

/** Pushes a directory frame with changed members to every active member. */
export async function notifyMembersChanged(env: Env, waitUntil: Wait, changed: MemberRow[]): Promise<void> {
  const all = await listMembers(env.DB);
  const frame = { t: "directory", members: changed.map(toMember) };
  for (const m of all) {
    if (m.deactivated) continue;
    waitUntil(gatewayStub(env, m.id).deliver([frame]).catch(() => {}));
  }
}

export async function summaryFor(env: Env, memberId: string, groupId: string): Promise<GroupSummary> {
  const [s] = await gatewayStub(env, memberId).summaries(memberId, [groupId]);
  if (!s) throw new ApiError("not_found", "group not found");
  return s;
}

/** Adds a member to a group: D1 first (source of truth), then the room log, then directory frames. */
export async function addMemberToGroup(
  env: Env,
  waitUntil: Wait,
  actor: Pick<AuthContext, "memberId" | "tokenId"> | null,
  groupId: string,
  memberId: string,
): Promise<void> {
  const now = Date.now();
  await env.DB.batch([
    env.DB.prepare(
      "INSERT OR IGNORE INTO group_members (group_id, member_id, added_at, added_by) VALUES (?, ?, ?, ?)",
    ).bind(groupId, memberId, now, actor?.memberId ?? null),
    auditStatement(env.DB, actor, { action: "group.member.add", target: groupId, details: { memberId } }, now),
  ]);
  unwrap(await roomStub(env, groupId).memberAdded(groupId, actor?.memberId ?? memberId, memberId));
  notifyGroupChanged(env, waitUntil, groupId, await groupMemberIds(env.DB, groupId));
}

export async function removeMemberFromGroup(
  env: Env,
  waitUntil: Wait,
  actor: Pick<AuthContext, "memberId" | "tokenId">,
  groupId: string,
  memberId: string,
): Promise<void> {
  const before = await groupMemberIds(env.DB, groupId);
  await env.DB.batch([
    env.DB.prepare("DELETE FROM group_members WHERE group_id = ? AND member_id = ?").bind(groupId, memberId),
    auditStatement(env.DB, actor, { action: "group.member.remove", target: groupId, details: { memberId } }),
  ]);
  unwrap(await roomStub(env, groupId).memberRemoved(groupId, actor.memberId, memberId));
  notifyGroupChanged(env, waitUntil, groupId, before);
}

async function addToDefaultGroups(env: Env, waitUntil: Wait, actor: AuthContext | null, memberId: string) {
  const rows = await env.DB.prepare("SELECT id FROM groups WHERE is_default = 1").all<{ id: string }>();
  for (const g of rows.results) await addMemberToGroup(env, waitUntil, actor, g.id, memberId);
}

/** Revokes tokens and closes the sockets that used them. */
export async function revokeTokens(
  env: Env,
  actor: AuthContext,
  memberId: string,
  tokenIds: string[] | "all",
): Promise<number> {
  const now = Date.now();
  let ids: string[];
  if (tokenIds === "all") {
    const r = await env.DB.prepare("SELECT id FROM tokens WHERE member_id = ? AND revoked_at IS NULL")
      .bind(memberId)
      .all<{ id: string }>();
    ids = r.results.map((x) => x.id);
  } else {
    ids = tokenIds;
  }
  if (ids.length === 0) return 0;
  const stmts = ids.map((id) =>
    env.DB.prepare("UPDATE tokens SET revoked_at = ? WHERE id = ? AND member_id = ? AND revoked_at IS NULL").bind(
      now,
      id,
      memberId,
    ),
  );
  stmts.push(auditStatement(env.DB, actor, { action: "token.revoke", target: memberId, details: { tokenIds: ids } }, now));
  const results = await env.DB.batch(stmts);
  const revoked = results.slice(0, ids.length).reduce((n, r) => n + (r.meta.changes ?? 0), 0);
  await gatewayStub(env, memberId).closeSockets({ tokenIds: ids, reason: "token revoked" });
  return revoked;
}

/** Deactivates a member: rejects its tokens from now on and closes its sockets. */
export async function deactivateMember(env: Env, actor: AuthContext, memberId: string): Promise<void> {
  if (actor.member.level !== "owner") throw new ApiError("forbidden", "owner only");
  const r = await env.DB.batch([
    env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(memberId),
    auditStatement(env.DB, actor, { action: "member.deactivate", target: memberId }),
  ]);
  if (!r[0].meta.changes) throw new ApiError("not_found", "member not found");
  await gatewayStub(env, memberId).closeSockets({ reason: "member deactivated" });
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

export function registerCoreRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/health",
    auth: "none",
    handler: ({ env }) => ({ ok: true, version: env.VERSION }),
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/bootstrap",
    auth: "none",
    async handler({ request, env, body }) {
      const given = request.headers.get("x-admin-secret") ?? "";
      const expected = env.ADMIN_SECRET ?? "";
      const ok = await timingSafeEqualString(given, expected);
      if (!expected || !ok) throw new ApiError("unauthorized", "invalid admin secret");
      const { ownerName } = await body(BootstrapRequest);
      const now = Date.now();
      const memberId = ulid(now);
      const tokenId = ulid(now);
      const groupId = ulid(now);
      const token = newToken();
      const hash = await sha256Hex(token);
      const exists = "EXISTS (SELECT 1 FROM members WHERE id = ?)";
      // One batch = one transaction; every insert depends on the first one winning.
      const results = await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO members (id, kind, name, role, level, created_at)
           SELECT ?, 'human', ?, NULL, 'owner', ? WHERE NOT EXISTS (SELECT 1 FROM members)`,
        ).bind(memberId, ownerName, now),
        env.DB.prepare(
          `INSERT INTO tokens (id, token_hash, member_id, label, created_at) SELECT ?, ?, ?, 'bootstrap', ? WHERE ${exists}`,
        ).bind(tokenId, hash, memberId, now, memberId),
        env.DB.prepare(
          `INSERT INTO groups (id, kind, name, is_default, created_by, created_at)
           SELECT ?, 'group', ?, 1, ?, ? WHERE ${exists}`,
        ).bind(groupId, DEFAULT_GROUP_NAME, memberId, now, memberId),
        env.DB.prepare(
          `INSERT INTO group_members (group_id, member_id, added_at, added_by) SELECT ?, ?, ?, ? WHERE ${exists}`,
        ).bind(groupId, memberId, now, memberId, memberId),
        env.DB.prepare(
          `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
           SELECT ?, ?, 'admin.bootstrap', ?, '{}', ? WHERE ${exists}`,
        ).bind(memberId, tokenId, memberId, now, memberId),
      ]);
      if (!results[0].meta.changes) throw new ApiError("conflict", "already bootstrapped");
      unwrap(await roomStub(env, groupId).init(groupId, [memberId]));
      const member = toMember((await getMember(env.DB, memberId)) as MemberRow);
      return {
        token,
        member,
        group: { id: groupId, kind: "group", name: DEFAULT_GROUP_NAME, memberIds: [memberId], createdBy: memberId, createdAt: now },
      };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/auth/redeem",
    auth: "none",
    async handler({ env, body, ctx }) {
      const { inviteCode, deviceName } = await body(RedeemRequest);
      const codeHash = await sha256Hex(normalizeInviteCode(inviteCode));
      const now = Date.now();
      const memberId = ulid(now);
      const tokenId = ulid(now);
      const token = newToken();
      const tokenHash = await sha256Hex(token);
      const results = await env.DB.batch([
        env.DB.prepare(
          "UPDATE invites SET used_at = ?, used_by = ? WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?",
        ).bind(now, memberId, codeHash, now),
        env.DB.prepare(
          `INSERT INTO members (id, kind, name, role, level, created_at, created_by)
           SELECT ?, kind, name, role, level, ?, created_by FROM invites WHERE code_hash = ? AND used_by = ?`,
        ).bind(memberId, now, codeHash, memberId),
        env.DB.prepare(
          `INSERT INTO tokens (id, token_hash, member_id, label, created_at)
           SELECT ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM members WHERE id = ?)`,
        ).bind(tokenId, tokenHash, memberId, deviceName, now, memberId),
        env.DB.prepare(
          `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
           SELECT ?, ?, 'invite.redeem', id, '{}', ? FROM invites WHERE code_hash = ? AND used_by = ?`,
        ).bind(memberId, tokenId, now, codeHash, memberId),
      ]);
      if (!results[0].meta.changes) throw new ApiError("unauthorized", "invite code is invalid, used or expired");
      const row = (await getMember(env.DB, memberId)) as MemberRow;
      const waitUntil = (p: Promise<unknown>) => ctx.waitUntil(p);
      const auth: AuthContext = { memberId, tokenId, member: toMember(row), row };
      await addToDefaultGroups(env, waitUntil, auth, memberId);
      await notifyMembersChanged(env, waitUntil, [row]);
      return { token, member: toMember(row) };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/me",
    auth: "member",
    handler: ({ auth }) => (auth as AuthContext).member,
  });

  registerRoute({
    method: "GET",
    path: "/v1/members",
    auth: "member",
    handler: async ({ env }) => (await listMembers(env.DB)).map(toMember),
  });

  registerRoute({
    method: "POST",
    path: "/v1/invites",
    auth: "owner",
    async handler({ env, auth, body }) {
      const req = await body(CreateInviteRequest);
      const a = auth as AuthContext;
      const code = newInviteCode();
      const now = Date.now();
      const expiresAt = now + INVITE_TTL_MS;
      const id = ulid(now);
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO invites (id, code_hash, name, kind, level, role, created_by, created_at, expires_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        ).bind(id, await sha256Hex(normalizeInviteCode(code)), req.name, req.kind, req.level, req.role ?? null, a.memberId, now, expiresAt),
        auditStatement(env.DB, a, {
          action: "invite.create",
          target: id,
          details: { name: req.name, kind: req.kind, level: req.level, role: req.role ?? null },
        }, now),
      ]);
      return { code, expiresAt };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/bots",
    auth: "owner",
    async handler({ env, auth, body, ctx }) {
      const req = await body(CreateBotRequest);
      const a = auth as AuthContext;
      const now = Date.now();
      const memberId = ulid(now);
      const tokenId = ulid(now);
      const token = newToken();
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO members (id, kind, name, role, level, created_at, created_by, character) VALUES (?, 'bot', ?, ?, 'member', ?, ?, ?)`,
        ).bind(memberId, req.name, req.role ?? null, now, a.memberId, req.character ?? null),
        env.DB.prepare(
          "INSERT INTO tokens (id, token_hash, member_id, label, created_at) VALUES (?, ?, ?, 'bot', ?)",
        ).bind(tokenId, await sha256Hex(token), memberId, now),
        auditStatement(env.DB, a, {
          action: "bot.create",
          target: memberId,
          details: { name: req.name, tokenId, ...(req.character ? { character: req.character } : {}) },
        }, now),
      ]);
      const row = (await getMember(env.DB, memberId)) as MemberRow;
      const waitUntil = (p: Promise<unknown>) => ctx.waitUntil(p);
      await addToDefaultGroups(env, waitUntil, a, memberId);
      await notifyMembersChanged(env, waitUntil, [row]);
      return { member: toMember(row), token };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/tokens/revoke",
    auth: "member",
    async handler({ env, auth, body }) {
      const req = await body(RevokeTokensRequest);
      const a = auth as AuthContext;
      const target = req.memberId ?? a.memberId;
      if (target !== a.memberId && a.member.level !== "owner") throw new ApiError("forbidden", "owner only");
      if (target !== a.memberId && !(await getMember(env.DB, target))) throw new ApiError("not_found", "member not found");
      // No memberId and no all: revoke the calling token. Otherwise every token of the target.
      const which = req.memberId === undefined && !req.all ? [a.tokenId] : "all";
      return { revoked: await revokeTokens(env, a, target, which) };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/groups",
    auth: "member",
    handler: async ({ env, auth }) => {
      const a = auth as AuthContext;
      return gatewayStub(env, a.memberId).summaries(a.memberId);
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/groups",
    auth: "member",
    async handler({ env, auth, body, ctx }) {
      const req = await body(CreateGroupRequest);
      const a = auth as AuthContext;
      const memberIds = [...new Set([a.memberId, ...req.memberIds])];
      if (memberIds.length > 99) throw new ApiError("invalid", "too many members");
      await requireActiveMembers(env, memberIds);
      const now = Date.now();
      const groupId = ulid(now);
      await env.DB.batch([
        env.DB.prepare("INSERT INTO groups (id, kind, name, created_by, created_at) VALUES (?, 'group', ?, ?, ?)").bind(
          groupId,
          req.name,
          a.memberId,
          now,
        ),
        ...memberIds.map((id) =>
          env.DB.prepare("INSERT INTO group_members (group_id, member_id, added_at, added_by) VALUES (?, ?, ?, ?)").bind(
            groupId,
            id,
            now,
            a.memberId,
          ),
        ),
        auditStatement(env.DB, a, { action: "group.create", target: groupId, details: { memberIds } }, now),
      ]);
      unwrap(await roomStub(env, groupId).init(groupId, memberIds));
      notifyGroupChanged(env, (p) => ctx.waitUntil(p), groupId, memberIds.filter((id) => id !== a.memberId));
      const summary = await summaryFor(env, a.memberId, groupId);
      ctx.waitUntil(gatewayStub(env, a.memberId).groupChanged(a.memberId, groupId).catch(() => {}));
      return summary;
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/groups/:id/members",
    auth: "member",
    async handler({ env, auth, body, params, ctx }) {
      const req = await body(AddGroupMemberRequest);
      const a = auth as AuthContext;
      const group = await getGroup(env.DB, params.id);
      if (!group) throw new ApiError("not_found", "group not found");
      if (!(await isGroupMember(env.DB, group.id, a.memberId))) throw new ApiError("forbidden", "not a member of this group");
      if (group.kind === "dm") throw new ApiError("forbidden", "cannot add members to a DM");
      await requireActiveMembers(env, [req.memberId]);
      await addMemberToGroup(env, (p) => ctx.waitUntil(p), a, group.id, req.memberId);
      return summaryFor(env, a.memberId, group.id);
    },
  });

  registerRoute({
    method: "DELETE",
    path: "/v1/groups/:id/members/:memberId",
    auth: "member",
    async handler({ env, auth, params, ctx }) {
      const a = auth as AuthContext;
      const group = await getGroup(env.DB, params.id);
      if (!group) throw new ApiError("not_found", "group not found");
      if (params.memberId !== a.memberId && a.member.level !== "owner") {
        throw new ApiError("forbidden", "only an owner can remove other members");
      }
      if (group.kind === "dm") throw new ApiError("forbidden", "cannot leave a DM");
      if (!(await isGroupMember(env.DB, group.id, params.memberId))) throw new ApiError("not_found", "not a member");
      await removeMemberFromGroup(env, (p) => ctx.waitUntil(p), a, group.id, params.memberId);
      return { ok: true };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/dms",
    auth: "member",
    async handler({ env, auth, body, ctx }) {
      const { memberId } = await body(CreateDmRequest);
      const a = auth as AuthContext;
      if (memberId === a.memberId) throw new ApiError("invalid", "cannot DM yourself");
      await requireActiveMembers(env, [memberId]);
      const dmKey = [a.memberId, memberId].sort().join(":");
      const existing = await env.DB.prepare("SELECT id FROM groups WHERE dm_key = ?").bind(dmKey).first<{ id: string }>();
      if (existing) return summaryFor(env, a.memberId, existing.id);
      const now = Date.now();
      const groupId = ulid(now);
      const results = await env.DB.batch([
        env.DB.prepare(
          "INSERT OR IGNORE INTO groups (id, kind, name, dm_key, created_by, created_at) VALUES (?, 'dm', '', ?, ?, ?)",
        ).bind(groupId, dmKey, a.memberId, now),
        env.DB.prepare(
          `INSERT INTO group_members (group_id, member_id, added_at, added_by)
           SELECT ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM groups WHERE id = ?)`,
        ).bind(groupId, a.memberId, now, a.memberId, groupId),
        env.DB.prepare(
          `INSERT INTO group_members (group_id, member_id, added_at, added_by)
           SELECT ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM groups WHERE id = ?)`,
        ).bind(groupId, memberId, now, a.memberId, groupId),
        auditStatement(env.DB, a, { action: "dm.create", target: groupId, details: { memberId } }, now),
      ]);
      if (!results[0].meta.changes) {
        // Lost a race with the other member: return the DM they made.
        const winner = await env.DB.prepare("SELECT id FROM groups WHERE dm_key = ?").bind(dmKey).first<{ id: string }>();
        if (!winner) throw new ApiError("internal", "dm race");
        return summaryFor(env, a.memberId, winner.id);
      }
      unwrap(await roomStub(env, groupId).init(groupId, [a.memberId, memberId]));
      notifyGroupChanged(env, (p) => ctx.waitUntil(p), groupId, [a.memberId, memberId]);
      return summaryFor(env, a.memberId, groupId);
    },
  });
}

async function requireActiveMembers(env: Env, ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  const placeholders = ids.map(() => "?").join(",");
  const r = await env.DB.prepare(`SELECT id FROM members WHERE deactivated = 0 AND id IN (${placeholders})`)
    .bind(...ids)
    .all<{ id: string }>();
  if (r.results.length !== new Set(ids).size) throw new ApiError("invalid", "unknown or deactivated member");
}
