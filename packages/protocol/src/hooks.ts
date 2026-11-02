// V1 integrations: incoming webhooks and the GitHub adapter. See docs/protocol.md, "Integrations".
import { z } from "zod";
import { Card } from "./card.ts";
import { Body, ClientMsgId, Id, Millis, Name, Title } from "./common.ts";

export const HookKind = z.enum(["generic", "github"]);
export type HookKind = z.infer<typeof HookKind>;

/** Topic keys: letters, digits and `_ . : # / -`, 1..128 characters. */
export const TopicKey = z.string().min(1).max(128).regex(/^[A-Za-z0-9_.:#\/-]+$/);

export const Hook = z.object({
  id: Id,
  name: Name,
  kind: HookKind,
  groupId: Id,
  /** Default topic. Absent: each post without topicId or topicKey starts a topic. */
  topicId: Id.nullable().optional(),
  /** The bot member the hook posts as. The server makes it with the hook. */
  memberId: Id,
  createdBy: Id,
  createdAt: Millis,
  lastUsedAt: Millis.nullable().optional(),
});
export type Hook = z.infer<typeof Hook>;

/** `POST /v1/hooks` (owner) */
export const CreateHookRequest = z.object({
  name: Name,
  groupId: Id,
  topicId: Id.optional(),
  kind: HookKind,
});
export type CreateHookRequest = z.infer<typeof CreateHookRequest>;
/** The secret is shown only in this response. */
export const CreateHookResponse = z.object({
  hook: Hook,
  /** Absolute URL to post to: `/v1/hooks/:id/in` or `/v1/hooks/:id/github`. */
  url: z.string().min(1),
  secret: z.string().min(1),
});
export type CreateHookResponse = z.infer<typeof CreateHookResponse>;
/** `GET /v1/hooks` (owner) */
export const HooksResponse = z.array(Hook);
export type HooksResponse = z.infer<typeof HooksResponse>;

/** `POST /v1/hooks/:id/in` with header `X-Hook-Secret`. Response: `SendResult`. */
export const HookInRequest = z
  .object({
    /** Post into this topic of the hook's group. */
    topicId: Id.optional(),
    /** Post into the topic made by the first post with this key (per hook). */
    topicKey: TopicKey.optional(),
    /** Title of a topic this post starts. */
    title: Title.optional(),
    body: Body.optional(),
    card: Card.optional(),
    /** Dedupe key, unique per hook. Default: a new id. */
    clientMsgId: ClientMsgId.optional(),
  })
  .refine((r) => !(r.topicId && r.topicKey), { message: "topicId and topicKey exclude each other" })
  .refine((r) => (r.body !== undefined && r.body.length > 0) || r.card !== undefined, {
    message: "body or card is required",
  });
export type HookInRequest = z.infer<typeof HookInRequest>;
