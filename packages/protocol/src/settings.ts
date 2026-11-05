// 1.1 settings: notification preferences by event with quiet hours and a daily digest, the team's
// approval rules for AI members, and my signed-in devices. See docs/protocol.md, "Settings".
import { z } from "zod";
import { Count, Id, Millis } from "./common.ts";

/** A wall-clock time "HH:MM" (24 h) in the member's `timeZone`. */
export const ClockTime = z.string().regex(/^([01]\d|2[0-3]):[0-5]\d$/, "HH:MM");
export type ClockTime = z.infer<typeof ClockTime>;

/** An IANA time zone name such as "Asia/Shanghai". The server falls back to UTC for an unknown one. */
export const TimeZoneName = z.string().min(1).max(64).regex(/^[A-Za-z0-9_+\-/]+$/);

/**
 * The events a member can switch notifications for (设置 › 通知 › 提醒我). `waiting` 需要我拍板的事,
 * `result` 我交代的事做完了, `blocked` 成员卡住或出错, `mention` 有人 @我. Open: readers treat an
 * unknown value as a plain notification.
 */
export const NotifyEvent = z.enum(["waiting", "mention", "result", "blocked"]);
export type NotifyEvent = z.infer<typeof NotifyEvent>;

export const NotifyEventSwitches = z.object({
  waiting: z.boolean(),
  mention: z.boolean(),
  result: z.boolean(),
  blocked: z.boolean(),
});
export type NotifyEventSwitches = z.infer<typeof NotifyEventSwitches>;

export const QuietHours = z.object({
  enabled: z.boolean(),
  start: ClockTime,
  end: ClockTime,
});
export type QuietHours = z.infer<typeof QuietHours>;

/** `GET /v1/me/notify-settings`: the member's settings with defaults filled in. */
export const NotifySettings = z.object({
  events: NotifyEventSwitches,
  /** 每日摘要: the local time of the daily digest; null = no digest. */
  digestAt: ClockTime.nullable(),
  quietHours: QuietHours,
  timeZone: TimeZoneName,
  /** 0 = never changed (all defaults). */
  updatedAt: Millis,
});
export type NotifySettings = z.infer<typeof NotifySettings>;

export const DEFAULT_NOTIFY_SETTINGS: NotifySettings = {
  events: { waiting: true, mention: true, result: true, blocked: true },
  digestAt: null,
  quietHours: { enabled: false, start: "22:00", end: "08:00" },
  timeZone: "Asia/Shanghai",
  updatedAt: 0,
};

/** `PATCH /v1/me/notify-settings`: only the given fields change. Response: `NotifySettings`. */
export const UpdateNotifySettingsRequest = z.object({
  events: NotifyEventSwitches.partial().optional(),
  digestAt: ClockTime.nullable().optional(),
  quietHours: QuietHours.partial().optional(),
  timeZone: TimeZoneName.optional(),
});
export type UpdateNotifySettingsRequest = z.infer<typeof UpdateNotifySettingsRequest>;

/** Field added to the `event` server frame next to `notify`: which switchable event it is. */
export const eventFrameSettingsFields = {
  notifyEvent: NotifyEvent.optional(),
};

/** One topic in a digest. */
export const DigestItem = z.object({ groupId: Id, topicId: Id, event: NotifyEvent });
export type DigestItem = z.infer<typeof DigestItem>;

/**
 * Transient `digest` (feature "settings/1" only): what happened since the last digest, counted
 * by event, newest topics first (at most 20).
 */
export const DigestTransient = z.object({
  type: z.literal("digest"),
  at: Millis,
  since: Millis,
  counts: z.object({
    waiting: Count,
    mention: Count,
    result: Count,
    blocked: Count,
  }),
  items: z.array(DigestItem).max(20),
});
export type DigestTransient = z.infer<typeof DigestTransient>;

// ---------------------------------------------------------------------------
// Approval defaults for AI members
// ---------------------------------------------------------------------------

/** 合并到 main, 发布到生产, 改生产环境配置, 删除数据或文件, 对外发布（社媒、邮件）. */
export const ApprovalActions = z.object({
  mergeMain: z.boolean(),
  deployProd: z.boolean(),
  prodConfig: z.boolean(),
  deleteData: z.boolean(),
  externalPublish: z.boolean(),
});
export type ApprovalActions = z.infer<typeof ApprovalActions>;

/** 谁可以批准: owners only, or the run's requester (and owners). */
export const Approver = z.enum(["owners", "requester"]);
export type Approver = z.infer<typeof Approver>;

/** Max single spend before asking, in whole yuan (单次花费超过); null = 不限. */
export const SpendLimit = z.int().min(1).max(1_000_000).nullable();

export const ApprovalPolicy = z.object({
  askBefore: ApprovalActions,
  maxSpendCny: SpendLimit,
  approver: Approver,
  /** 0 = never changed. */
  updatedAt: Millis,
});
export type ApprovalPolicy = z.infer<typeof ApprovalPolicy>;

export const DEFAULT_APPROVAL_POLICY: ApprovalPolicy = {
  askBefore: { mergeMain: true, deployProd: true, prodConfig: true, deleteData: true, externalPublish: true },
  maxSpendCny: 50,
  // The released rule: the run's requester or an owner answers its approvals.
  approver: "requester",
  updatedAt: 0,
};

/** `PATCH /v1/settings/approvals` (owner): only the given fields change. Response: `ApprovalPolicy`. */
export const UpdateApprovalPolicyRequest = z.object({
  askBefore: ApprovalActions.partial().optional(),
  maxSpendCny: SpendLimit.optional(),
  approver: Approver.optional(),
});
export type UpdateApprovalPolicyRequest = z.infer<typeof UpdateApprovalPolicyRequest>;

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/** One signed-in device: a live token of mine. `id` is the token's record id, never the token. */
export const Device = z.object({
  id: Id,
  /** The `deviceName` given at sign in; may be empty. */
  name: z.string().max(128),
  createdAt: Millis,
  lastUsedAt: Millis.nullable(),
  /** The token of this request. */
  current: z.boolean(),
});
export type Device = z.infer<typeof Device>;

/** `GET /v1/me/devices`, newest use first. */
export const DevicesResponse = z.object({ devices: z.array(Device) });
export type DevicesResponse = z.infer<typeof DevicesResponse>;

// ---------------------------------------------------------------------------
// Email sign-in
// ---------------------------------------------------------------------------

/** An email address as members type it: trimmed and compared in lower case, at most 254 chars. */
export const EmailAddress = z
  .string()
  .trim()
  .toLowerCase()
  .max(254)
  .regex(/^[^\s@]+@[^\s@]+\.[^\s@]+$/, "not an email address");
export type EmailAddress = z.infer<typeof EmailAddress>;

/** The 6 digits of an email code. */
export const EmailCode = z.string().regex(/^[0-9]{6}$/, "six digits");

/** `POST /v1/auth/email/code` (sign in) and `POST /v1/me/email/code` (add my email). */
export const EmailCodeRequest = z.object({ email: EmailAddress });
export type EmailCodeRequest = z.infer<typeof EmailCodeRequest>;
/** The code was sent, if the address may get one; it stops working at `expiresAt`. */
export const EmailCodeResponse = z.object({ sent: z.literal(true), expiresAt: Millis });
export type EmailCodeResponse = z.infer<typeof EmailCodeResponse>;

/** `POST /v1/auth/email/sign-in`; answers `RedeemResponse` (a new token for that member). */
export const EmailSignInRequest = z.object({
  email: EmailAddress,
  code: EmailCode,
  deviceName: z.string().min(1).max(128),
});
export type EmailSignInRequest = z.infer<typeof EmailSignInRequest>;

/** `POST /v1/me/email`: confirms the code sent to `email` and makes it my sign-in email. */
export const VerifyEmailRequest = z.object({ email: EmailAddress, code: EmailCode });
export type VerifyEmailRequest = z.infer<typeof VerifyEmailRequest>;

/** `GET`, `POST` and `DELETE /v1/me/email`: my sign-in email, `null` when I have none. */
export const MyEmailResponse = z.object({ email: EmailAddress.nullable() });
export type MyEmailResponse = z.infer<typeof MyEmailResponse>;

/** `hello.features` value of a client that handles `digest` and `notifyEvent`. */
export const SETTINGS_FEATURE = "settings/1";
