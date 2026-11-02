// Hook endpoints of docs/protocol.md ("Integrations") and the GitHub login map. See
// docs/integrations.md.
import { ClientMsgId, CreateHookRequest, type Hook, HookInRequest, Id, type SendResult } from "@chat/protocol";
import { z } from "zod";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { type MemberRow, getGroup, getMember, toMember } from "../core/db";
import { gatewayStub, roomStub } from "../core/env";
import { ApiError, type Result, unwrap } from "../core/errors";
import { readText } from "../core/http";
import { ulid } from "../core/ids";
import type { Logger } from "../core/logger";
import { type FrameContext, frameValidators, registerRoomRpc, registerRoute } from "../core/registry";
import { addMemberToGroup, deactivateMember, notifyMembersChanged } from "../core/routes";
import { checkCardSize } from "../notify/messages";
import type { DeliverArgs, HookRef } from "./deliver";
import { PayloadError, normalize } from "./github";
import { type HooksEnv, checkSecret, decryptSecret, encryptSecret, newHookSecret, verifyGithub } from "./secrets";

const IN_BODY_LIMIT = 256 * 1024;
/** GitHub caps payloads at 25 MB; PR, review and CI payloads are far below this. */
const GITHUB_BODY_LIMIT = 5 * 1024 * 1024;

interface HookRow {
  id: string;
  name: string;
  kind: "generic" | "github";
  group_id: string;
  topic_id: string | null;
  member_id: string;
  secret_enc: string;
  created_by: string;
  created_at: number;
  last_used_at: number | null;
  deleted_at: number | null;
}

function toHook(r: HookRow): Hook {
  return {
    id: r.id,
    name: r.name,
    kind: r.kind,
    groupId: r.group_id,
    topicId: r.topic_id,
    memberId: r.member_id,
    createdBy: r.created_by,
    createdAt: r.created_at,
    lastUsedAt: r.last_used_at,
  };
}

const ref = (r: HookRow): HookRef => ({ id: r.id, groupId: r.group_id, topicId: r.topic_id, memberId: r.member_id });

function hookUrl(url: URL, r: HookRow): string {
  return `${url.origin}/v1/hooks/${r.id}/${r.kind === "github" ? "github" : "in"}`;
}

async function activeHook(env: HooksEnv, id: string): Promise<HookRow | null> {
  return env.DB.prepare("SELECT * FROM hooks WHERE id = ? AND deleted_at IS NULL").bind(id).first<HookRow>();
}

registerRoomRpc("hooks.topicExists", (room, _actor, args) => {
  const { topicId } = args as { topicId: string };
  return room.sql.exec("SELECT 1 FROM topics WHERE id = ?", topicId).toArray().length > 0;
});

async function deliver(env: HooksEnv, ctx: ExecutionContext, r: HookRow, args: DeliverArgs): Promise<unknown> {
  const value = unwrap((await gatewayStub(env, r.member_id).rpc("hooks.deliver", args)) as Result<unknown>);
  ctx.waitUntil(
    env.DB.prepare("UPDATE hooks SET last_used_at = ? WHERE id = ?")
      .bind(Date.now(), r.id)
      .run()
      .catch(() => {}),
  );
  return value;
}

// ---------------------------------------------------------------------------
// Owner endpoints
// ---------------------------------------------------------------------------

registerRoute({
  method: "POST",
  path: "/v1/hooks",
  auth: "owner",
  async handler({ env: e, auth, body, ctx, url }) {
    const env = e as HooksEnv;
    const a = auth as AuthContext;
    const req = await body(CreateHookRequest);
    const group = await getGroup(env.DB, req.groupId);
    if (!group) throw new ApiError("not_found", "group not found");
    if (group.kind === "dm") throw new ApiError("invalid", "a hook cannot post in a DM");
    if (req.topicId) {
      const exists = unwrap(await roomStub(env, group.id).rpc(group.id, "hooks.topicExists", null, { topicId: req.topicId }));
      if (!exists) throw new ApiError("not_found", "topic not found");
    }
    const now = Date.now();
    const id = ulid(now);
    const memberId = ulid(now);
    const secret = newHookSecret();
    const secretEnc = await encryptSecret(env, id, secret);
    await env.DB.batch([
      env.DB.prepare(
        "INSERT INTO members (id, kind, name, role, level, created_at, created_by) VALUES (?, 'bot', ?, NULL, 'member', ?, ?)",
      ).bind(memberId, req.name, now, a.memberId),
      env.DB.prepare(
        `INSERT INTO hooks (id, name, kind, group_id, topic_id, member_id, secret_enc, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      ).bind(id, req.name, req.kind, group.id, req.topicId ?? null, memberId, secretEnc, a.memberId, now),
      auditStatement(
        env.DB,
        a,
        { action: "hook.create", target: id, details: { name: req.name, kind: req.kind, groupId: group.id, memberId } },
        now,
      ),
    ]);
    const waitUntil = (p: Promise<unknown>) => ctx.waitUntil(p);
    await addMemberToGroup(env, waitUntil, a, group.id, memberId);
    const memberRow = (await getMember(env.DB, memberId)) as MemberRow;
    await notifyMembersChanged(env, waitUntil, [memberRow]);
    const row = (await activeHook(env, id)) as HookRow;
    return { hook: toHook(row), url: hookUrl(url, row), secret };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/hooks",
  auth: "owner",
  async handler({ env }) {
    const r = await env.DB.prepare("SELECT * FROM hooks WHERE deleted_at IS NULL ORDER BY created_at").all<HookRow>();
    return r.results.map(toHook);
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/hooks/:id",
  auth: "owner",
  async handler({ env, auth, params, ctx }) {
    const a = auth as AuthContext;
    const now = Date.now();
    const r = await env.DB.batch([
      env.DB.prepare("UPDATE hooks SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL").bind(now, params.id),
      env.DB.prepare(
        `INSERT INTO audit (actor_id, credential_id, action, target, details, at)
         SELECT ?, ?, 'hook.delete', id, '{}', ? FROM hooks WHERE id = ? AND deleted_at = ?`,
      ).bind(a.memberId, a.tokenId, now, params.id, now),
    ]);
    if (!r[0].meta.changes) throw new ApiError("not_found", "hook not found");
    const row = (await env.DB.prepare("SELECT * FROM hooks WHERE id = ?").bind(params.id).first<HookRow>()) as HookRow;
    await deactivateMember(env, a, row.member_id);
    const memberRow = await getMember(env.DB, row.member_id);
    if (memberRow) await notifyMembersChanged(env, (p) => ctx.waitUntil(p), [memberRow]);
    return { ok: true };
  },
});

// GitHub login map: which member a GitHub login is, for waitingOn (reviewers) and ownerId (author).
const LoginMap = z.object({
  logins: z
    .record(z.string().min(1).max(39).regex(/^[A-Za-z0-9-]+$/), Id)
    .refine((m) => Object.keys(m).length <= 500, { message: "at most 500 logins" }),
});

async function readLogins(env: HooksEnv): Promise<Record<string, string>> {
  const r = await env.DB.prepare("SELECT login, member_id FROM github_logins ORDER BY login").all<{
    login: string;
    member_id: string;
  }>();
  return Object.fromEntries(r.results.map((x) => [x.login, x.member_id]));
}

registerRoute({
  method: "GET",
  path: "/v1/integrations/github/logins",
  auth: "owner",
  handler: async ({ env }) => ({ logins: await readLogins(env) }),
});

registerRoute({
  method: "PUT",
  path: "/v1/integrations/github/logins",
  auth: "owner",
  async handler({ env, auth, body }) {
    const a = auth as AuthContext;
    const { logins } = await body(LoginMap);
    const entries = Object.entries(logins).map(([l, id]) => [l.toLowerCase(), id] as const);
    const ids = [...new Set(entries.map(([, id]) => id))];
    for (const id of ids) {
      if (!(await getMember(env.DB, id))) throw new ApiError("invalid", "unknown member in logins");
    }
    const now = Date.now();
    await env.DB.batch([
      env.DB.prepare("DELETE FROM github_logins"),
      ...entries.map(([l, id]) =>
        env.DB.prepare("INSERT OR REPLACE INTO github_logins (login, member_id, updated_at) VALUES (?, ?, ?)").bind(l, id, now),
      ),
      auditStatement(env.DB, a, { action: "github.logins.update", details: { count: entries.length } }, now),
    ]);
    return { logins: await readLogins(env) };
  },
});

// ---------------------------------------------------------------------------
// Incoming: generic and GitHub
// ---------------------------------------------------------------------------

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    throw new ApiError("invalid", "body is not valid JSON");
  }
}

function frameContext(env: HooksEnv, r: HookRow, log: Logger): FrameContext {
  return {
    env,
    actor: { id: r.member_id, level: "member", kind: "bot", tokenId: r.id },
    log,
    reply: () => {},
    broadcast: () => {},
    requireMember: async (gid) => {
      if (gid !== r.group_id) throw new ApiError("forbidden", "not a member of this group");
    },
    get sql(): SqlStorage {
      throw new ApiError("internal", "no gateway storage on the hook path");
    },
  };
}

registerRoute({
  method: "POST",
  path: "/v1/hooks/:id/in",
  auth: "none",
  async handler({ env: e, request, params, ctx, log }) {
    const env = e as HooksEnv;
    const r = await activeHook(env, params.id);
    if (!r || r.kind !== "generic") throw new ApiError("not_found", "hook not found");
    const given = request.headers.get("x-hook-secret") ?? "";
    if (!(await checkSecret(given, await decryptSecret(env, r.id, r.secret_enc)))) {
      throw new ApiError("unauthorized", "invalid hook secret");
    }
    const raw = parseJson(await readText(request, IN_BODY_LIMIT));
    checkCardSize(raw);
    const parsed = HookInRequest.safeParse(raw);
    if (!parsed.success) {
      const first = parsed.error.issues[0];
      throw new ApiError("invalid", `invalid body: ${first?.path.join(".") || "(root)"} ${first?.message ?? ""}`.trim());
    }
    const req = parsed.data;
    const header = request.headers.get("idempotency-key");
    let clientMsgId = req.clientMsgId;
    if (!clientMsgId && header !== null) {
      const k = ClientMsgId.safeParse(header);
      if (!k.success) throw new ApiError("invalid", "Idempotency-Key must be 1..64 characters");
      clientMsgId = k.data;
    }
    clientMsgId ??= ulid();

    // Feature frame validators run as for a WebSocket send.
    const frame: Record<string, unknown> & { t: string } = {
      t: "send",
      rid: "hook",
      groupId: r.group_id,
      clientMsgId,
      body: req.body ?? "",
    };
    for (const k of ["topicId", "title", "card"] as const) if (req[k] !== undefined) frame[k] = req[k];
    const fc = frameContext(env, r, log);
    for (const v of [...(frameValidators.get("*") ?? []), ...(frameValidators.get("send") ?? [])]) await v(frame, fc);

    const args: DeliverArgs = { op: "in", hook: ref(r), body: req.body ?? "", clientMsgId };
    if (req.topicId) args.topicId = req.topicId;
    if (req.topicKey) args.topicKey = req.topicKey;
    if (req.title) args.title = req.title;
    if (req.card) args.card = req.card;
    const result = (await deliver(env, ctx, r, args)) as SendResult;
    if (!result.duplicate) {
      await auditStatement(
        env.DB,
        { memberId: r.member_id, tokenId: r.id },
        {
          action: "hook.post",
          target: result.message.id,
          details: { credentialKind: "hook", hookId: r.id, groupId: r.group_id, topicId: result.message.topicId },
        },
      ).run();
    }
    return result;
  },
});

registerRoute({
  method: "POST",
  path: "/v1/hooks/:id/github",
  auth: "none",
  async handler({ env: e, request, params, ctx, log }) {
    const env = e as HooksEnv;
    const r = await activeHook(env, params.id);
    if (!r || r.kind !== "github") throw new ApiError("not_found", "hook not found");
    const text = await readText(request, GITHUB_BODY_LIMIT);
    const secret = await decryptSecret(env, r.id, r.secret_enc);
    if (!(await verifyGithub(secret, text, request.headers.get("x-hub-signature-256")))) {
      throw new ApiError("unauthorized", "invalid signature");
    }
    const eventName = request.headers.get("x-github-event") ?? "";
    const deliveryHeader = request.headers.get("x-github-delivery");
    const deliveryId = deliveryHeader && deliveryHeader.length <= 100 ? deliveryHeader : null;
    let event: ReturnType<typeof normalize>;
    try {
      event = normalize(eventName, parseJson(text));
    } catch (err) {
      if (err instanceof PayloadError) throw new ApiError("invalid", err.message);
      throw err;
    }
    if (!event) {
      log.info("github event ignored", { hookId: r.id, event: eventName });
      return { ok: true };
    }
    await deliver(env, ctx, r, { op: "github", hook: ref(r), deliveryId, event, logins: await readLogins(env) });
    return { ok: true };
  },
});
