// The agent harness (docs/design/agent-harness.zh.md): the front desk's routes. The desk logic
// lives in bots/desk/front.ts; the decision rules in ./frontdesk.ts.
import { BotMetrics, FrontDeskDecisionsResponse } from "@chat/protocol";
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

/** `GET /v1/bots/:id/metrics?days=` (owner): what the front desk and the runs did lately (M6). */
registerRoute({
  method: "GET",
  path: "/v1/bots/:id/metrics",
  auth: "owner",
  async handler({ env, params, url }) {
    const days = Number(url.searchParams.get("days") ?? "30");
    if (!Number.isInteger(days) || days < 1 || days > 365) throw new ApiError("invalid", "days must be 1..365");
    const since = Date.now() - days * 86_400_000;
    return BotMetrics.parse(await deskStub(env, params.id).metrics(params.id, since));
  },
});

/**
 * `GET /v1/bots/:id/decisions/export?since=` (owner): the front desk's decisions with what it
 * read, one JSON per line, to build a replay set (scripts/replay-frontdesk.ts). Holds topic text:
 * keep it out of the public repo.
 */
registerRoute({
  method: "GET",
  path: "/v1/bots/:id/decisions/export",
  auth: "owner",
  async handler({ env, params, url }) {
    const since = Number(url.searchParams.get("since") ?? "0");
    if (!Number.isInteger(since) || since < 0) throw new ApiError("invalid", "since must be a timestamp in ms");
    const cases = await deskStub(env, params.id).replayCases(params.id, since);
    const body = cases.map((c) => JSON.stringify({ ...c, expect: c.action })).join("\n");
    return new Response(body === "" ? "" : `${body}\n`, {
      headers: { "content-type": "application/x-ndjson; charset=utf-8", "content-disposition": `attachment; filename="frontdesk-${params.id}.jsonl"` },
    });
  },
});
