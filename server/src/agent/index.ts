// The agent harness (docs/design/agent-harness.zh.md): the front desk's routes. The desk logic
// lives in bots/desk/front.ts; the decision rules in ./frontdesk.ts.
import { FrontDeskDecisionsResponse } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { deskStub } from "../bots/env";

type Row = {
  message_id: string; topic_id: string; author_id: string; action: string; decision: { reason?: string; confidence?: number; reply?: string; brief?: unknown };
  fallback: string | null; model: string | null; ms: number; input_tokens: number; output_tokens: number; reply_message_id: string | null; at: number;
};

registerRoute({
  method: "GET",
  path: "/v1/bots/:id/decisions",
  auth: "owner",
  async handler({ env, params, url }) {
    const topicId = url.searchParams.get("topicId");
    if (!topicId) throw new ApiError("invalid", "topicId is required");
    const rows = (await deskStub(env, params.id).decisions(params.id, topicId)) as Row[];
    const decisions = rows.map((r) => ({
      messageId: r.message_id,
      topicId: r.topic_id,
      authorId: r.author_id,
      action: r.action,
      reason: r.decision.reason ?? "",
      confidence: r.decision.confidence ?? 0,
      ...(r.decision.reply ? { reply: r.decision.reply } : {}),
      ...(r.decision.brief ? { brief: r.decision.brief } : {}),
      ...(r.fallback ? { fallback: r.fallback } : {}),
      ...(r.model ? { model: r.model } : {}),
      ms: r.ms,
      inputTokens: r.input_tokens,
      outputTokens: r.output_tokens,
      ...(r.reply_message_id ? { replyMessageId: r.reply_message_id } : {}),
      at: r.at,
    }));
    return FrontDeskDecisionsResponse.parse({ decisions });
  },
});
