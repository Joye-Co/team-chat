// HTTP send for bots and scripts: POST /v1/groups/:id/messages runs the `send` path of the
// group room (same checks, idempotency, events and fan-out) and is audited.
import { MAX_CARD_BYTES, MAX_CARD_V2_BYTES, PostMessageRequest, type SendResult, utf8Bytes } from "@chat/protocol";
import { z } from "zod";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { getGroup, isGroupMember } from "../core/db";
import { gatewayStub, roomStub } from "../core/env";
import { ApiError, schemaReason, unwrap } from "../core/errors";
import { LIMITS, TokenBucket } from "../core/ratelimit";
import { type Actor, type FrameContext, frameValidators, registerGatewayRpc, registerRoute } from "../core/registry";

/** Card plus body plus quotes and attachments fit easily; the room enforces the field limits. */
const POST_BODY_LIMIT = 512 * 1024;

/** Throws `too_large` when a raw frame or body carries a card above 64 KiB. */
export function checkCardSize(raw: unknown): void {
  if (!raw || typeof raw !== "object") return;
  const { card, cardV2 } = raw as { card?: unknown; cardV2?: unknown };
  if (card !== undefined && utf8Bytes(JSON.stringify(card)) > MAX_CARD_BYTES) {
    throw new ApiError("too_large", "card exceeds 64 KiB");
  }
  if (cardV2 !== undefined && utf8Bytes(JSON.stringify(cardV2)) > MAX_CARD_V2_BYTES) {
    throw new ApiError("too_large", "card exceeds 64 KiB");
  }
}

// Per-credential HTTP post limit. The bucket lives in the member's gateway (one object per
// member), so all Worker isolates share it.
const httpBuckets = new Map<string, TokenBucket>();
registerGatewayRpc("http.post.take", (_gw, args) => {
  const { tokenId } = args as { tokenId: string };
  let b = httpBuckets.get(tokenId);
  if (!b) {
    b = new TokenBucket(LIMITS.write.rate, LIMITS.write.burst);
    httpBuckets.set(tokenId, b);
  }
  return b.take();
});

registerRoute({
  method: "POST",
  path: "/v1/groups/:id/messages",
  auth: "member",
  bodyLimit: POST_BODY_LIMIT,
  async handler({ env, auth, params, body, log }) {
    const a = auth as AuthContext;
    if (!(await gatewayStub(env, a.memberId).rpc("http.post.take", { tokenId: a.tokenId }))) {
      throw new ApiError("rate_limited", "too many posts");
    }
    const group = await getGroup(env.DB, params.id);
    if (!group) throw new ApiError("not_found", "group not found");
    if (!(await isGroupMember(env.DB, group.id, a.memberId))) throw new ApiError("forbidden", "not a member of this group");

    const raw = await body(z.record(z.string(), z.unknown()));
    checkCardSize(raw);
    const parsed = PostMessageRequest.safeParse(raw);
    if (!parsed.success) {
      const first = parsed.error.issues.find((i) => schemaReason({}, i.message)) ?? parsed.error.issues[0];
      throw new ApiError("invalid", `invalid body: ${first?.path.join(".") || "(root)"} ${first?.message ?? ""}`.trim(), schemaReason(raw, first?.message));
    }
    const frame = { ...parsed.data, t: "send" as const, rid: "http", groupId: group.id };
    const actor: Actor = { id: a.memberId, level: a.member.level, kind: a.member.kind, tokenId: a.tokenId };

    // Frame validators of feature modules run as for a WebSocket send. There is no socket and
    // no gateway SQLite here.
    const c: FrameContext = {
      env,
      actor,
      log,
      reply: () => {},
      broadcast: () => {},
      requireMember: async (gid) => {
        if (!(await isGroupMember(env.DB, gid, a.memberId))) throw new ApiError("forbidden", "not a member of this group");
      },
      get sql(): SqlStorage {
        throw new ApiError("internal", "no gateway storage on the HTTP path");
      },
    };
    for (const v of [...(frameValidators.get("*") ?? []), ...(frameValidators.get("send") ?? [])]) await v(frame, c);

    const result: SendResult = unwrap(await roomStub(env, group.id).send(group.id, actor, frame));
    if (!result.duplicate) {
      await auditStatement(env.DB, a, {
        action: "message.post",
        target: result.message.id,
        details: { groupId: group.id, topicId: result.message.topicId },
      }).run();
    }
    return result;
  },
});
