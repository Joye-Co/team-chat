// Members admin (PATCH /v1/members/:id) and the audit log reader (GET /v1/audit).
// See docs/protocol.md, "Members and preferences" and "Audit".
import { type AuditEntry, MAX_AUDIT_LIMIT, MAX_OWN_BOTS, UpdateMemberRequest } from "@chat/protocol";
import { type AuthContext, requireTeamOwner } from "../core/auth";
import { type MemberRow, getMember, isTeamOwner, toMember } from "../core/db";
import { gatewayStub } from "../core/env";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { afterDeactivate, membersChanged, notifyMembersChanged, revokeTokens } from "../core/routes";

registerRoute({
  method: "PATCH",
  path: "/v1/members/:id",
  auth: "member",
  async handler({ env, auth, params, body, ctx }) {
    const a = auth as AuthContext;
    const req = await body(UpdateMemberRequest);
    const isOwner = a.member.level === "owner";
    const self = params.id === a.memberId;
    const target = await getMember(env.DB, params.id);
    // Usable (design-personal-ai): a person edits his own AI member's name, 岗位, avatar and
    // whether it is in use, nothing else.
    const myBot = !isOwner && !!target && target.kind === "bot" && target.owner_id === a.memberId;
    if (!isOwner && !self && !myBot) throw new ApiError("forbidden", "only an owner can change other members");
    if (
      !isOwner &&
      (req.level !== undefined || req.teamOwner !== undefined || req.reportsTo !== undefined || req.weeklyBudgetCents !== undefined ||
        (!myBot && (req.deactivated !== undefined || req.character !== undefined)))
    ) {
      throw new ApiError("forbidden", "only an owner can change level, character, reportsTo, the budget or deactivate");
    }
    if (!target) throw new ApiError("not_found", "member not found");
    if (myBot && req.deactivated === false && target.deactivated) {
      const n = await env.DB.prepare("SELECT COUNT(*) AS n FROM members WHERE kind = 'bot' AND owner_id = ? AND deactivated = 0")
        .bind(a.memberId)
        .first<{ n: number }>();
      if ((n?.n ?? 0) >= MAX_OWN_BOTS) throw new ApiError("forbidden", "you already have an AI member of your own", "bot.own_limit");
    }
    // design-roles 3.2: an admin's or a 所有者's record is the 所有者's to change.
    if (!self && target.level === "owner") requireTeamOwner(a, "only a team owner can change an admin");
    if (req.teamOwner === true && target.kind !== "human") throw new ApiError("invalid", "an AI member cannot be the team owner");
    if (req.teamOwner === true && req.level === "member") throw new ApiError("invalid", "a team owner is an admin");
    if (req.reportsTo) {
      if (req.reportsTo === target.id) throw new ApiError("invalid", "a member cannot report to itself");
      const boss = await getMember(env.DB, req.reportsTo);
      if (!boss) throw new ApiError("invalid", "reportsTo is not a member");
    }

    const next = {
      name: req.name ?? target.name,
      role: req.role === undefined ? target.role : req.role,
      level: req.teamOwner ? "owner" : (req.level ?? target.level),
      deactivated: req.deactivated === undefined ? target.deactivated === 1 : req.deactivated,
      character: req.character === undefined ? (target.character ?? null) : req.character,
      reportsTo: req.reportsTo === undefined ? (target.reports_to ?? null) : req.reportsTo,
      weeklyBudgetCents: req.weeklyBudgetCents === undefined ? (target.weekly_budget_cents ?? null) : req.weeklyBudgetCents,
    };
    const wasTeamOwner = isTeamOwner(target);
    const nextTeamOwner = next.level === "owner" && target.kind === "human" && (req.teamOwner ?? wasTeamOwner);
    // Changing anyone's role, own included, is the 所有者's (design-roles 3.2).
    if (next.level !== target.level || nextTeamOwner !== wasTeamOwner) requireTeamOwner(a, "only a team owner can change roles");
    const details: Record<string, unknown> = {};
    if (next.name !== target.name) details.name = [target.name, next.name];
    if (next.role !== target.role) details.role = [target.role, next.role];
    if (next.level !== target.level) details.level = [target.level, next.level];
    if (nextTeamOwner !== wasTeamOwner) {
      // `level` too, so the activity trigger (0013) files it as a level change.
      details.level = [target.level, next.level];
      details.teamOwner = [wasTeamOwner, nextTeamOwner];
    }
    if (next.deactivated !== (target.deactivated === 1)) details.deactivated = [target.deactivated === 1, next.deactivated];
    if (next.character !== (target.character ?? null)) details.character = [target.character ?? null, next.character];
    if (next.reportsTo !== (target.reports_to ?? null)) details.reportsTo = [target.reports_to ?? null, next.reportsTo];
    if (next.weeklyBudgetCents !== (target.weekly_budget_cents ?? null)) details.weeklyBudgetCents = [target.weekly_budget_cents ?? null, next.weeklyBudgetCents];
    if (Object.keys(details).length === 0) return toMember(target);

    // A change that takes away an active 所有者 needs another active human 所有者. The check is part
    // of the UPDATE so two 所有者 demoting each other at once cannot both win.
    const losesOwner = wasTeamOwner && target.deactivated === 0 && (!nextTeamOwner || next.deactivated);
    const now = Date.now();
    const results = await env.DB.batch([
      env.DB.prepare(
        `UPDATE members SET name = ?, role = ?, level = ?, team_owner = ?, deactivated = ?, character = ?, reports_to = ?, weekly_budget_cents = ?
         WHERE id = ? AND (? = 0 OR EXISTS
           (SELECT 1 FROM members o WHERE o.team_owner = 1 AND o.level = 'owner' AND o.kind = 'human' AND o.deactivated = 0 AND o.id != ?))`,
      ).bind(
        next.name,
        next.role,
        next.level,
        nextTeamOwner ? 1 : 0,
        next.deactivated ? 1 : 0,
        next.character,
        next.reportsTo,
        next.weeklyBudgetCents,
        target.id,
        losesOwner ? 1 : 0,
        target.id,
      ),
      env.DB.prepare(
        `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
         SELECT ?, ?, 'member.update', ?, ?, ? WHERE changes() = 1`,
      ).bind(a.memberId, a.tokenId, target.id, JSON.stringify(details), now),
    ]);
    if (!results[0].meta.changes) throw new ApiError("conflict", "the team needs at least one active team owner", "role.last_owner");

    if (details.deactivated && next.deactivated) {
      await revokeTokens(env, a, target.id, "all");
      await gatewayStub(env, target.id).closeSockets({ reason: "member deactivated" });
      await afterDeactivate(env, target.id);
    } else if (details.level) {
      await gatewayStub(env, target.id).setLevel(next.level);
    }
    const row = (await getMember(env.DB, target.id)) as MemberRow;
    await notifyMembersChanged(env, (p) => ctx.waitUntil(p), [row]);
    // A new weekly budget may let an AI member's held runs go (budget/index.ts).
    if (details.weeklyBudgetCents) await membersChanged(env, [target.id]);
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
