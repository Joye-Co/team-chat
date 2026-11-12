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
  UpdateGroupRequest,
  MAX_GROUP_PURPOSE_CHARS,
  normalizeGroupText,
} from "@chat/protocol";
import { auditStatement } from "./audit";
import { type AuthContext, requireTeamOwner } from "./auth";
import { newInviteCode, newToken, normalizeInviteCode, sha256Hex, timingSafeEqualString } from "./crypto";
import { REDEEM_RATE, refundAttempt, takeAttempt } from "./ratelimit";
import {
  type MemberRow,
  getGroup,
  getMember,
  groupMemberIds,
  isGroupMember,
  listMembers,
  toMember,
  viewGroup,
} from "./db";
import { type Env, gatewayStub, roomStub } from "./env";
import { ApiError, unwrap } from "./errors";
import { ulid } from "./ids";
import { logger } from "./logger";
import { groupMemberRemovedHooks, memberDeactivatedHooks, membersChangedHooks, registerRoute } from "./registry";

export const INVITE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
export const DEFAULT_GROUP_NAME = "全员";
export const MAX_GROUP_NAME_CHARS = 64;

/**
 * A group name as it is stored: trimmed, every run of whitespace (also full-width spaces and
 * line breaks) one space, 1..64 code points. Group create and rename use the same rule.
 */
export function normalizeGroupName(raw: string): string {
  const name = normalizeGroupText(raw);
  const chars = [...name].length;
  if (chars === 0) throw new ApiError("invalid", "name is empty");
  if (chars > MAX_GROUP_NAME_CHARS) throw new ApiError("invalid", `name is longer than ${MAX_GROUP_NAME_CHARS} characters`);
  return name;
}

/** A purpose line as it is stored: normalized like a name, 0..200 code points ("" = none). */
export function normalizeGroupPurpose(raw: string): string {
  const purpose = normalizeGroupText(raw);
  if ([...purpose].length > MAX_GROUP_PURPOSE_CHARS) throw new ApiError("invalid", `purpose is longer than ${MAX_GROUP_PURPOSE_CHARS} characters`);
  return purpose;
}

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
  await membersChanged(env, changed.map((m) => m.id));
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
  const before = new Set(await groupMemberIds(env.DB, groupId));
  before.add(memberId);
  await env.DB.batch([
    env.DB.prepare("DELETE FROM group_members WHERE group_id = ? AND member_id = ?").bind(groupId, memberId),
    auditStatement(env.DB, actor, { action: "group.member.remove", target: groupId, details: { memberId } }),
  ]);
  // Idempotent: a retry after a failed room call (D1 already changed) still updates the room.
  unwrap(await roomStub(env, groupId).memberRemoved(groupId, actor.memberId, memberId));
  for (const hook of groupMemberRemovedHooks) {
    try {
      await hook(env, groupId, memberId);
    } catch (err) {
      logger.error("group member removed hook failed", { groupId, memberId, err });
    }
  }
  notifyGroupChanged(env, waitUntil, groupId, [...before]);
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

/** Runs the feature hooks for a deactivated member (for example: stop a bot's runs). */
export async function afterDeactivate(env: Env, memberId: string): Promise<void> {
  for (const hook of memberDeactivatedHooks) {
    try {
      await hook(env, memberId);
    } catch (err) {
      logger.error("member deactivated hook failed", { memberId, err });
    }
  }
  await membersChanged(env, [memberId]);
}

/** Runs the members-changed hooks (features drop what they cached about these members). */
export async function membersChanged(env: Env, memberIds: string[]): Promise<void> {
  for (const hook of membersChangedHooks) {
    try {
      await hook(env, memberIds);
    } catch (err) {
      logger.error("members changed hook failed", { memberIds, err });
    }
  }
}

/** Deactivates a member: rejects its tokens from now on and closes its sockets. */
export async function deactivateMember(env: Env, actor: AuthContext, memberId: string): Promise<void> {
  if (actor.member.level !== "owner") throw new ApiError("forbidden", "owner only");
  const target = await getMember(env.DB, memberId);
  if (target?.level === "owner" && memberId !== actor.memberId) requireTeamOwner(actor, "only a team owner can deactivate an admin");
  const r = await env.DB.batch([
    env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ?").bind(memberId),
    auditStatement(env.DB, actor, { action: "member.deactivate", target: memberId }),
  ]);
  if (!r[0].meta.changes) throw new ApiError("not_found", "member not found");
  await gatewayStub(env, memberId).closeSockets({ reason: "member deactivated" });
  await afterDeactivate(env, memberId);
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
          `INSERT INTO members (id, kind, name, role, level, team_owner, created_at)
           SELECT ?, 'human', ?, NULL, 'owner', 1, ? WHERE NOT EXISTS (SELECT 1 FROM members)`,
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
    async handler({ env, body, ctx, request }) {
      // Codes are 8 characters (about 40 bits): limit guesses per client IP. The IP is hashed,
      // never stored or logged.
      const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
      const attempt = await takeAttempt(env.DB, `redeem:${await sha256Hex(ip)}`, REDEEM_RATE);
      const { inviteCode, deviceName } = await body(RedeemRequest);
      const codeHash = await sha256Hex(normalizeInviteCode(inviteCode));
      const now = Date.now();
      const tokenId = ulid(now);
      const token = newToken();
      const tokenHash = await sha256Hex(token);
      const forMember = await env.DB.prepare("SELECT member_id FROM invites WHERE code_hash = ?")
        .bind(codeHash)
        .first<{ member_id: string | null }>();
      if (forMember?.member_id) {
        // A sign-in code for an existing member: one more token (device), same member.
        const existing = forMember.member_id;
        const r = await env.DB.batch([
          env.DB.prepare(
            `UPDATE invites SET used_at = ?, used_by = member_id WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?
               AND EXISTS (SELECT 1 FROM members m WHERE m.id = invites.member_id AND m.deactivated = 0)`,
          ).bind(now, codeHash, now),
          env.DB.prepare(
            "INSERT INTO tokens (id, token_hash, member_id, label, created_at) SELECT ?, ?, ?, ?, ? WHERE changes() = 1",
          ).bind(tokenId, tokenHash, existing, deviceName, now),
          env.DB.prepare(
            `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
             SELECT ?, ?, 'invite.redeem', id, '{"existingMember":true}', ? FROM invites
             WHERE code_hash = ? AND used_at = ? AND EXISTS (SELECT 1 FROM tokens WHERE id = ?)`,
          ).bind(existing, tokenId, now, codeHash, now, tokenId),
        ]);
        if (!r[0].meta.changes) throw new ApiError("unauthorized", "invite code is invalid, used or expired");
        await refundAttempt(env.DB, attempt);
        const row = (await getMember(env.DB, existing)) as MemberRow;
        return { token, member: toMember(row) };
      }
      const memberId = ulid(now);
      const results = await env.DB.batch([
        env.DB.prepare(
          "UPDATE invites SET used_at = ?, used_by = ? WHERE code_hash = ? AND used_at IS NULL AND expires_at > ? AND member_id IS NULL",
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
      await refundAttempt(env.DB, attempt);
      const row = (await getMember(env.DB, memberId)) as MemberRow;
      const waitUntil = (p: Promise<unknown>) => ctx.waitUntil(p);
      const auth: AuthContext = { memberId, tokenId, member: toMember(row), row };
      await addToDefaultGroups(env, waitUntil, auth, memberId);
      await notifyMembersChanged(env, waitUntil, [row]);
      await openFirstDm(env, waitUntil, auth);
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
      if (req.memberId) {
        // The code signs in as this member: its own name, kind and level, not the request's.
        const m = await getMember(env.DB, req.memberId);
        if (!m) throw new ApiError("not_found", "member not found");
        if (m.deactivated) throw new ApiError("invalid", "reactivate the member first");
        // Redeeming the code signs in as the target, so a code for another admin is 所有者 only.
        if (m.level === "owner" && m.id !== a.memberId) requireTeamOwner(a, "only a team owner can sign in as an admin");
        Object.assign(req, { name: m.name, kind: m.kind, level: m.level, role: m.role ?? undefined });
      } else if (req.level === "owner") {
        // A new admin (person or AI member) is a role change: 所有者 only. Invited people arrive
        // as members and get a role afterwards (design-roles 3.2).
        requireTeamOwner(a, "only a team owner can invite an admin");
      }
      const code = newInviteCode();
      const now = Date.now();
      const expiresAt = now + (req.expiresInHours ? req.expiresInHours * 3_600_000 : INVITE_TTL_MS);
      const id = ulid(now);
      await env.DB.batch([
        env.DB.prepare(
          `INSERT INTO invites (id, code_hash, name, kind, level, role, created_by, created_at, expires_at, member_id)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        ).bind(
          id,
          await sha256Hex(normalizeInviteCode(code)),
          req.name,
          req.kind,
          req.level,
          req.role ?? null,
          a.memberId,
          now,
          expiresAt,
          req.memberId ?? null,
        ),
        auditStatement(env.DB, a, {
          action: "invite.create",
          target: id,
          details: { name: req.name, kind: req.kind, level: req.level, role: req.role ?? null, ...(req.memberId ? { memberId: req.memberId } : {}) },
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
      if (target !== a.memberId) {
        const t = await getMember(env.DB, target);
        if (!t) throw new ApiError("not_found", "member not found");
        // An admin's or a 所有者's tokens: 所有者 only (design-roles 3.2).
        if (t.level === "owner") requireTeamOwner(a, "only a team owner can revoke an admin's tokens");
      }
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
      const name = normalizeGroupName(req.name);
      const a = auth as AuthContext;
      const memberIds = [...new Set([a.memberId, ...req.memberIds])];
      if (memberIds.length > 99) throw new ApiError("invalid", "too many members");
      await requireActiveMembers(env, memberIds);
      const now = Date.now();
      const groupId = ulid(now);
      await env.DB.batch([
        env.DB.prepare("INSERT INTO groups (id, kind, name, created_by, created_at) VALUES (?, 'group', ?, ?, ?)").bind(
          groupId,
          name,
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
    method: "PATCH",
    path: "/v1/groups/:id",
    auth: "member",
    async handler({ env, auth, body, params }) {
      const req = await body(UpdateGroupRequest);
      const a = auth as AuthContext;
      const group = await getGroup(env.DB, params.id);
      if (!group) throw new ApiError("not_found", "group not found");
      const member = await isGroupMember(env.DB, group.id, a.memberId);
      if (!member && a.member.level !== "owner") throw new ApiError("forbidden", "not a member of this group");
      if (group.kind === "dm") throw new ApiError("forbidden", "a DM cannot be renamed");
      const name = req.name === undefined ? undefined : normalizeGroupName(req.name);
      const purpose = req.purpose === undefined ? undefined : normalizeGroupPurpose(req.purpose);
      // The room first: it decides whether anything changed and appends `group.updated`, which
      // updates every client's cache. Then D1, the source of truth for GroupSummary, takes the
      // room's values only when their seq is newer than its own (SF-9): concurrent renames end
      // with D1 equal to the newest event of the log, and a retry after a failed D1 write writes
      // D1 again without a new event.
      const before = { name: group.name, purpose: group.purpose ?? "" };
      const r = unwrap(await roomStub(env, group.id).groupUpdated(group.id, a.memberId, { name, purpose }, before));
      const audits = [];
      if (r.seq !== null && r.name !== before.name) {
        audits.push(auditStatement(env.DB, a, { action: "group.rename", target: group.id, details: { from: before.name, to: r.name } }));
      }
      if (r.seq !== null && r.purpose !== before.purpose) {
        audits.push(auditStatement(env.DB, a, { action: "group.purpose", target: group.id, details: { chars: [...r.purpose].length } }));
      }
      await env.DB.batch([
        env.DB.prepare("UPDATE groups SET name = ?, purpose = ?, update_seq = ? WHERE id = ? AND update_seq < ?").bind(
          r.name,
          r.purpose === "" ? null : r.purpose,
          r.updateSeq,
          group.id,
          r.updateSeq,
        ),
        ...audits,
      ]);
      if (member) return summaryFor(env, a.memberId, group.id);
      // An owner outside the group: no read position of its own.
      const memberIds = await groupMemberIds(env.DB, group.id);
      const names = new Map((await listMembers(env.DB)).map((m) => [m.id, m.name]));
      return {
        group: viewGroup({ ...group, name: r.name, purpose: r.purpose }, memberIds, a.memberId, names),
        lastSeq: r.lastSeq,
        readSeq: 0,
        unread: 0,
        mentions: 0,
      } satisfies GroupSummary;
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
      if (!(await isGroupMember(env.DB, group.id, params.memberId))) {
        // D1 says gone. A removal whose room call failed is finished here; otherwise 404.
        const inRoom = unwrap(await roomStub(env, group.id).memberIds(group.id)).includes(params.memberId);
        if (!inRoom) throw new ApiError("not_found", "not a member");
      }
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
      const groupId = await findOrCreateDm(env, (p) => ctx.waitUntil(p), a, memberId);
      return summaryFor(env, a.memberId, groupId);
    },
  });
}

/** The DM of `actor` and `memberId`: the existing one, else a new one (the race with the other
 * member made the same DM is resolved to the winner). Returns its group id. */
async function findOrCreateDm(env: Env, waitUntil: Wait, a: AuthContext, memberId: string): Promise<string> {
  const dmKey = [a.memberId, memberId].sort().join(":");
  const existing = await env.DB.prepare("SELECT id FROM groups WHERE dm_key = ?").bind(dmKey).first<{ id: string }>();
  if (existing) return existing.id;
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
    // Lost a race with the other member: the DM they made.
    const winner = await env.DB.prepare("SELECT id FROM groups WHERE dm_key = ?").bind(dmKey).first<{ id: string }>();
    if (!winner) throw new ApiError("internal", "dm race");
    return winner.id;
  }
  unwrap(await roomStub(env, groupId).init(groupId, [a.memberId, memberId]));
  notifyGroupChanged(env, waitUntil, groupId, [a.memberId, memberId]);
  return groupId;
}

/** A new person starts with a DM with the default bot (Jojo, else the first active bot, the
 * client's 单独问 default), so the first thing they see has someone to talk to. Best effort:
 * joining never fails because of it. */
async function openFirstDm(env: Env, waitUntil: Wait, a: AuthContext): Promise<void> {
  if (a.member.kind !== "human") return;
  const bot = await env.DB.prepare(
    `SELECT id FROM members WHERE kind = 'bot' AND deactivated = 0
     ORDER BY CASE WHEN lower(name) = 'jojo' THEN 0 ELSE 1 END, created_at, id LIMIT 1`,
  ).first<{ id: string }>();
  if (!bot) return;
  try {
    await findOrCreateDm(env, waitUntil, a, bot.id);
  } catch (err) {
    logger.warn("first dm failed", { memberId: a.memberId, err });
  }
}

async function requireActiveMembers(env: Env, ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  const placeholders = ids.map(() => "?").join(",");
  const r = await env.DB.prepare(`SELECT id FROM members WHERE deactivated = 0 AND id IN (${placeholders})`)
    .bind(...ids)
    .all<{ id: string }>();
  if (r.results.length !== new Set(ids).size) throw new ApiError("invalid", "unknown or deactivated member");
}
