// 1.1 settings: notification preferences by event with quiet hours and a daily digest, the team's
// approval rules for AI members, the team's AI 服务 and my signed-in devices. See docs/protocol.md, "Settings".
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

/**
 * The action kinds the machine stops at the moment a command would run (要批准的动作): the
 * `ApprovalActions` keys. docs/protocol.md, "Approval rules for AI members".
 */
export const GATE_KINDS = ["mergeMain", "deployProd", "prodConfig", "deleteData", "externalPublish"] as const;
export const GateKind = z.enum(GATE_KINDS);
export type GateKind = z.infer<typeof GateKind>;

/**
 * One matching rule: a regular expression matched against each shell command a tool would run
 * (case-insensitive). Use the subset JavaScript and Rust's `regex` share: no lookaround, no
 * backreferences. The server rejects a pattern JavaScript cannot compile.
 */
export const GatePattern = z.string().min(1).max(200);
export const MAX_GATE_PATTERNS = 20;
export const GatePatterns = z.array(GatePattern).max(MAX_GATE_PATTERNS);

/** The team's own patterns per kind (团队自己的匹配规则), added to the defaults. */
export const ApprovalPatterns = z.object({
  mergeMain: GatePatterns.optional(),
  deployProd: GatePatterns.optional(),
  prodConfig: GatePatterns.optional(),
  deleteData: GatePatterns.optional(),
  externalPublish: GatePatterns.optional(),
});
export type ApprovalPatterns = z.infer<typeof ApprovalPatterns>;

/** Who approves each kind (每类的批准人); a kind without one uses `approver`. */
export const ApprovalApprovers = z.object({
  mergeMain: Approver.optional(),
  deployProd: Approver.optional(),
  prodConfig: Approver.optional(),
  deleteData: Approver.optional(),
  externalPublish: Approver.optional(),
});
export type ApprovalApprovers = z.infer<typeof ApprovalApprovers>;

/**
 * The built-in patterns of each kind. Conservative on purpose: a wrong match is a useless card,
 * so only commands that clearly do the action are listed (docs/protocol.md lists them too).
 */
export const DEFAULT_GATE_PATTERNS: Record<GateKind, readonly string[]> = {
  mergeMain: [
    "\\bgh pr merge\\b",
    // `git push origin main`, `git push origin HEAD:main`, `git push origin x:refs/heads/master`
    "\\bgit push\\b.*[\\s:](refs/heads/)?(main|master)(\\s|$)",
  ],
  deployProd: [
    "\\bwrangler (deploy|publish)\\b.*(--env|-e)[ =](prod|production)\\b",
    "\\bvercel\\b.*--prod\\b",
    "\\bnetlify deploy\\b.*--prod\\b",
    "\\bfly(ctl)? deploy\\b",
    "\\b(npm|bun|pnpm|yarn) run deploy[:_-](prod|production)\\b",
  ],
  prodConfig: [
    "\\bwrangler secret (put|delete|bulk)\\b.*(--env|-e)[ =](prod|production)\\b",
    "\\bvercel env (add|rm|remove)\\b.*\\bproduction\\b",
    "\\bfly(ctl)? secrets (set|unset|import)\\b",
  ],
  deleteData: [
    // rm with flags of the root or the home folder itself
    "\\brm\\s+(-\\S+\\s+)+(/|/\\*|~/?|~/\\*|\\$HOME/?|\\$HOME/\\*)(\\s|$)",
    "\\b(wrangler d1 execute|psql|mysql|sqlite3)\\b.*\\b(drop\\s+(table|database|schema)|truncate\\s+table)\\b",
    "\\bwrangler d1 execute\\b.*(--remote\\b.*\\bdelete\\s+from\\b|\\bdelete\\s+from\\b.*--remote\\b)",
    "\\bwrangler (d1|kv (namespace|key|bulk)|r2 (bucket|object)) delete\\b",
    "\\bgh repo delete\\b",
  ],
  externalPublish: [
    "\\b(npm|bun|pnpm|yarn) publish\\b",
    "\\bcargo publish\\b",
    "\\bgh release create\\b",
    "\\btwine upload\\b",
  ],
};

export const ApprovalPolicy = z.object({
  askBefore: ApprovalActions,
  maxSpendCny: SpendLimit,
  approver: Approver,
  /** 0 = never changed. */
  updatedAt: Millis,
  /** The team's own patterns per kind, added to `DEFAULT_GATE_PATTERNS`; absent = none. */
  patterns: ApprovalPatterns.optional(),
  /** Who approves each kind; absent or a kind left out = `approver`. */
  approvers: ApprovalApprovers.optional(),
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
  /** Replaces the team patterns of each kind given (`[]` clears that kind). */
  patterns: ApprovalPatterns.optional(),
  /** Sets the approver of each kind given. */
  approvers: ApprovalApprovers.optional(),
});
export type UpdateApprovalPolicyRequest = z.infer<typeof UpdateApprovalPolicyRequest>;

// ---------------------------------------------------------------------------
// AI 服务: the team's model provider (owner only)
// ---------------------------------------------------------------------------

/** Who answers the server's own model calls (AI 话题标题). Open: Rust reads an unknown one as `Unknown`. */
export const AiProvider = z.enum(["anthropic", "deepseek", "openai-compatible"]);
export type AiProvider = z.infer<typeof AiProvider>;

/** Printable ASCII without spaces. */
const Token = /^[\x21-\x7e]+$/;
/** A model id as the provider names it: `deepseek-chat`, `claude-haiku-5-5`, `vendor/model`. */
export const AiModel = z.string().min(1).max(128).regex(Token);
/** The API root of an OpenAI-compatible provider; `/chat/completions` is added to it. https only. */
export const AiBaseUrl = z.string().max(256).regex(/^https:\/\/[\x21-\x7e]+$/, "https URL");
/** An API key. Sent only to the server; never returned. */
export const AiApiKey = z.string().min(8).max(512).regex(Token);

/** `GET /v1/settings/ai` (owner). Never carries the key: only whether one is set and its last 4 characters. */
export const AiServiceConfig = z.object({
  /** null = not set up: topics keep their first line as the title. */
  provider: AiProvider.nullable(),
  /** "" when not set up. */
  model: z.string().max(128),
  /** Only for `openai-compatible`; null otherwise. */
  baseUrl: AiBaseUrl.nullable(),
  keySet: z.boolean(),
  /** The key's last 4 characters, null when no key is set. */
  keyHint: z.string().max(4).nullable(),
  /** 0 = never changed. */
  updatedAt: Millis,
});
export type AiServiceConfig = z.infer<typeof AiServiceConfig>;

const needsBaseUrl = (v: { provider: AiProvider; baseUrl?: string | null }, ctx: z.RefinementCtx) => {
  if (v.provider === "openai-compatible" && !v.baseUrl) ctx.addIssue({ code: "custom", path: ["baseUrl"], message: "openai-compatible needs baseUrl" });
};

/**
 * `PUT /v1/settings/ai` (owner): replaces the config. Without `apiKey` the saved key stays, which
 * is allowed only while the provider stays the same. Response: `AiServiceConfig`.
 */
export const UpdateAiServiceRequest = z
  .object({
    provider: AiProvider,
    model: AiModel,
    baseUrl: AiBaseUrl.nullable().optional(),
    apiKey: AiApiKey.optional(),
  })
  .superRefine(needsBaseUrl);
export type UpdateAiServiceRequest = z.infer<typeof UpdateAiServiceRequest>;

/**
 * `POST /v1/settings/ai/test` (owner): one tiny call with these values, not saved. Without
 * `apiKey` the saved key is used when the provider is the saved one. Response: `AiServiceTestResult`.
 */
export const TestAiServiceRequest = UpdateAiServiceRequest;
export type TestAiServiceRequest = UpdateAiServiceRequest;

export const AiServiceTestResult = z.object({
  ok: z.boolean(),
  /** The provider's error (status and message), when not ok. Plain text, may be English. */
  error: z.string().max(500).optional(),
  latencyMs: z.int().min(0),
});
export type AiServiceTestResult = z.infer<typeof AiServiceTestResult>;

/** Model suggested for each provider when the owner picks it (cheap and good at short titles). */
export const DEFAULT_AI_MODELS: Record<AiProvider, string> = {
  anthropic: "claude-haiku-5-5",
  deepseek: "deepseek-chat",
  "openai-compatible": "",
};

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
