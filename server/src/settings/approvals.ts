// The team's approval rules for AI members (docs/protocol.md, "Settings"; 设置 › 成员 › 默认规则):
// one row in D1 `approval_policies` with scope '*'. BotDesk reads it when it builds a run prompt
// and when someone answers an approval.
import { type ApprovalPolicy, UpdateApprovalPolicyRequest } from "@chat/protocol";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { applyPolicyUpdate, patternProblem, teamPolicy } from "./rules";

/** The scope of the team row. Other scopes are reserved (for example per AI member later). */
const TEAM = "*";

type PolicyRow = { policy: string; updated_at: number };

function parse(text: string): Partial<ApprovalPolicy> | null {
  try {
    const v = JSON.parse(text) as unknown;
    return v && typeof v === "object" ? (v as Partial<ApprovalPolicy>) : null;
  } catch {
    return null;
  }
}

export async function loadTeamPolicy(db: D1Database): Promise<ApprovalPolicy> {
  const row = await db.prepare("SELECT policy, updated_at FROM approval_policies WHERE scope = ?").bind(TEAM).first<PolicyRow>();
  return teamPolicy(row ? { ...parse(row.policy), updatedAt: row.updated_at } : null);
}

/** The policy that applies to AI members; the defaults when D1 cannot be read (never blocks a run). */
export async function effectivePolicy(db: D1Database): Promise<ApprovalPolicy> {
  try {
    return await loadTeamPolicy(db);
  } catch {
    return teamPolicy(null);
  }
}

registerRoute({
  method: "GET",
  path: "/v1/settings/approvals",
  auth: "member",
  handler: ({ env }) => loadTeamPolicy(env.DB),
});

registerRoute({
  method: "PATCH",
  path: "/v1/settings/approvals",
  auth: "owner",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const update = await body(UpdateApprovalPolicyRequest);
    // The daemon compiles the patterns with Rust's regex: only the shared subset goes in.
    for (const [kind, list] of Object.entries(update.patterns ?? {})) {
      for (const p of list ?? []) {
        const why = patternProblem(p);
        if (why) throw new ApiError("invalid", `patterns.${kind}: ${why}`);
      }
    }
    const before = await loadTeamPolicy(env.DB);
    const now = Date.now();
    const after = applyPolicyUpdate(before, update, now);
    const { updatedAt: _a, ...stored } = after;
    const { updatedAt: _b, ...was } = before;
    await env.DB.batch([
      env.DB.prepare(
        `INSERT INTO approval_policies (scope, policy, updated_at, updated_by) VALUES (?, ?, ?, ?)
         ON CONFLICT (scope) DO UPDATE SET policy = excluded.policy, updated_at = excluded.updated_at, updated_by = excluded.updated_by`,
      ).bind(TEAM, JSON.stringify(stored), now, a.memberId),
      auditStatement(env.DB, a, { action: "settings.approvals", target: TEAM, details: { before: was, after: stored } }, now),
    ]);
    return after;
  },
});
