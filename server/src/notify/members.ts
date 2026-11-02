// Members admin (PATCH /v1/members/:id) and the audit log reader (GET /v1/audit).
// See docs/protocol.md, "Members and preferences" and "Audit".
import { type AuditEntry, MAX_AUDIT_LIMIT, UpdateMemberRequest } from "@chat/protocol";
import type { AuthContext } from "../core/auth";
import { type MemberRow, getMember, toMember } from "../core/db";
import { gatewayStub } from "../core/env";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { notifyMembersChanged, revokeTokens } from "../core/routes";

registerRoute({
  method: "PATCH",
  path: "/v1/members/:id",
  auth: "member",
  async handler({ env, auth, params, body, ctx }) {
    const a = auth as AuthContext;
    const req = await body(UpdateMemberRequest);
    const isOwner = a.member.level === "owner";
    const self = params.id === a.memberId;
    if (!isOwner && !self) throw new ApiError("forbidden", "only an owner can change other members");
    if (!isOwner && (req.level !== undefined || req.deactivated !== undefined)) {
      throw new ApiError("forbidden", "only an owner can change level or deactivate");
    }
    const target = await getMember(env.DB, params.id);
    if (!target) throw new ApiError("not_found", "member not found");

    const next = {
      name: req.name ?? target.name,
      role: req.role === undefined ? target.role : req.role,
      level: req.level ?? target.level,
      deactivated: req.deactivated === undefined ? target.deactivated === 1 : req.deactivated,
    };
    const details: Record<string, unknown> = {};
    if (next.name !== target.name) details.name = [target.name, next.name];
    if (next.role !== target.role) details.role = [target.role, next.role];
    if (next.level !== target.level) details.level = [target.level, next.level];
    if (next.deactivated !== (target.deactivated === 1)) details.deactivated = [target.deactivated === 1, next.deactivated];
    if (Object.keys(details).length === 0) return toMember(target);

    // A change that takes away an active owner needs another active owner. The check is part of
    // the UPDATE so two owners demoting each other at once cannot both win.
    const losesOwner = target.level === "owner" && target.deactivated === 0 && (next.level !== "owner" || next.deactivated);
    const now = Date.now();
    const results = await env.DB.batch([
      env.DB.prepare(
        `UPDATE members SET name = ?, role = ?, level = ?, deactivated = ?
         WHERE id = ? AND (? = 0 OR EXISTS
           (SELECT 1 FROM members o WHERE o.level = 'owner' AND o.deactivated = 0 AND o.id != ?))`,
      ).bind(next.name, next.role, next.level, next.deactivated ? 1 : 0, target.id, losesOwner ? 1 : 0, target.id),
      env.DB.prepare(
        `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
         SELECT ?, ?, 'member.update', ?, ?, ? WHERE changes() = 1`,
      ).bind(a.memberId, a.tokenId, target.id, JSON.stringify(details), now),
    ]);
    if (!results[0].meta.changes) throw new ApiError("conflict", "the team needs at least one active owner");

    if (details.deactivated && next.deactivated) {
      await revokeTokens(env, a, target.id, "all");
      await gatewayStub(env, target.id).closeSockets({ reason: "member deactivated" });
    } else if (details.level) {
      await gatewayStub(env, target.id).setLevel(next.level);
    }
    const row = (await getMember(env.DB, target.id)) as MemberRow;
    await notifyMembersChanged(env, (p) => ctx.waitUntil(p), [row]);
    return toMember(row);
  },
});

// ---------------------------------------------------------------------------
// GET /v1/audit
// ---------------------------------------------------------------------------

type AuditRow = {
  id: number;
  actor_id: string | null;
  credential_id: string | null;
  action: string;
  target: string | null;
  details: string;
  at: number;
};

/** What kind of record an action's target is (the core stores only its id). */
const TARGET_KIND: Record<string, string> = {
  member: "member",
  bot: "member",
  token: "member",
  invite: "invite",
  group: "group",
  dm: "group",
  prefs: "group",
  admin: "member",
  message: "message",
  hook: "hook",
  release: "release",
  file: "file",
  artifact: "artifact",
};

function toEntry(r: AuditRow): AuditEntry {
  let details: Record<string, unknown> = {};
  try {
    details = JSON.parse(r.details) as Record<string, unknown>;
  } catch {
    // keep empty
  }
  const prefix = r.action.split(".")[0] ?? "";
  const kind = TARGET_KIND[prefix] ?? prefix.slice(0, 32);
  const credentialKind = typeof details.credentialKind === "string" ? details.credentialKind : null;
  const entry: AuditEntry = {
    id: String(r.id),
    at: r.at,
    actorId: r.actor_id,
    credential: r.credential_id
      ? { kind: (credentialKind as AuditEntry["credential"]["kind"] | null) ?? "token", id: r.credential_id }
      : { kind: "admin" },
    action: r.action.slice(0, 64),
  };
  if (r.target && kind) entry.target = { kind, id: r.target };
  const groupId = typeof details.groupId === "string" ? details.groupId : kind === "group" ? r.target : null;
  if (groupId) entry.groupId = groupId;
  if (typeof details.requestedBy === "string") entry.requestedBy = details.requestedBy;
  const { credentialKind: _c, requestedBy: _r, ...rest } = details;
  if (Object.keys(rest).length > 0) entry.details = rest as AuditEntry["details"];
  return entry;
}

registerRoute({
  method: "GET",
  path: "/v1/audit",
  auth: "owner",
  async handler({ env, url }) {
    const limitText = url.searchParams.get("limit");
    const beforeText = url.searchParams.get("before");
    const limit = limitText === null ? 50 : Number(limitText);
    if (!Number.isInteger(limit) || limit < 1 || limit > MAX_AUDIT_LIMIT) throw new ApiError("invalid", "limit is 1..200");
    let before = Number.MAX_SAFE_INTEGER;
    if (beforeText !== null) {
      if (!/^\d{1,15}$/.test(beforeText)) throw new ApiError("invalid", "before is an entry id");
      before = Number(beforeText);
    }
    const r = await env.DB.prepare("SELECT * FROM audit WHERE id < ? ORDER BY id DESC LIMIT ?")
      .bind(before, limit + 1)
      .all<AuditRow>();
    return { entries: r.results.slice(0, limit).map(toEntry), hasMore: r.results.length > limit };
  },
});
