// Phone push: device registration, the 等你 badge, the data a push carries and answering a card
// from a notification over HTTP. See docs/protocol.md, "Push".
import { z } from "zod";
import { ActionId } from "./card.ts";
import { CardRev, ClientEventId } from "./card2.ts";
import { Count, Id, Millis, Seq } from "./common.ts";
import { NotifyReason } from "./members.ts";
import { NotifyEvent } from "./settings.ts";

/** The push service. Open: new platforms come with a client release. */
export const PushPlatform = z.enum(["apns"]);
export type PushPlatform = z.infer<typeof PushPlatform>;

/** APNs gateway the token belongs to: development builds get `sandbox` tokens. */
export const PushEnvironment = z.enum(["production", "sandbox"]);
export type PushEnvironment = z.infer<typeof PushEnvironment>;

/** The device token as lower-case hex (APNs gives 32 bytes today). */
export const PushToken = z.string().regex(/^[0-9a-f]{32,400}$/, "lower-case hex");
/** The app's bundle id, the APNs topic. */
export const PushAppId = z
  .string()
  .max(155)
  .regex(/^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$/, "bundle id");

/** `POST /v1/me/push-devices`: this sign-in's phone wants pushes. */
export const RegisterPushDeviceRequest = z.object({
  platform: PushPlatform,
  token: PushToken,
  appId: PushAppId,
  environment: PushEnvironment,
});
export type RegisterPushDeviceRequest = z.infer<typeof RegisterPushDeviceRequest>;

/** One registered phone. The push token itself is never sent back. */
export const PushDevice = z.object({
  id: Id,
  platform: PushPlatform,
  appId: PushAppId,
  environment: PushEnvironment,
  createdAt: Millis,
  updatedAt: Millis,
  /** Registered with this request's sign-in. */
  current: z.boolean(),
});
export type PushDevice = z.infer<typeof PushDevice>;

/** `GET /v1/me/push-devices`. */
export const PushDevicesResponse = z.object({ devices: z.array(PushDevice) });
export type PushDevicesResponse = z.infer<typeof PushDevicesResponse>;

/** `DELETE /v1/me/push-devices[/:id]`. */
export const RemovePushDeviceResponse = z.object({ removed: Count });
export type RemovePushDeviceResponse = z.infer<typeof RemovePushDeviceResponse>;

/** `GET /v1/me/badge`: topics that wait on me (等你), the app icon's badge. */
export const BadgeResponse = z.object({ waiting: Count });
export type BadgeResponse = z.infer<typeof BadgeResponse>;

/** Max quick answers a push offers. */
export const MAX_PUSH_ACTIONS = 3;

/** An answer the notification offers as a button. Only answers that can be taken back. */
export const PushAction = z.object({ id: ActionId, label: z.string().min(1).max(40) });
export type PushAction = z.infer<typeof PushAction>;

/** An open question on a card that can be answered from the notification. */
export const PushAsk = z.object({
  messageId: Id,
  rev: CardRev,
  actions: z.array(PushAction).min(1).max(MAX_PUSH_ACTIONS),
});
export type PushAsk = z.infer<typeof PushAsk>;

/** The `tc` key of an APNs payload, next to `aps`. */
export const PushData = z.object({
  v: z.literal(1),
  reason: NotifyReason,
  notifyEvent: NotifyEvent.optional(),
  groupId: Id,
  topicId: Id.optional(),
  messageId: Id.optional(),
  /** The event's seq in the group. */
  seq: Seq,
  ask: PushAsk.optional(),
});
export type PushData = z.infer<typeof PushData>;

/** `POST /v1/groups/:id/messages/:messageId/answer`: `card.answer` without a socket. */
export const AnswerCardRequest = z.object({
  rev: CardRev,
  actionId: ActionId,
  clientEventId: ClientEventId.optional(),
});
export type AnswerCardRequest = z.infer<typeof AnswerCardRequest>;

/** APNs category of a push that offers answers; others have none (tap opens). */
export const PUSH_CATEGORY_ASK = "tc.ask";
