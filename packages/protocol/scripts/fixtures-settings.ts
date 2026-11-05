// Settings fixtures: notification settings, digest, the approval policy, devices. Called by gen-fixtures.ts.
type Obj = Record<string, unknown>;
export type SettingsCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  T0: number;
  ID: Record<string, string>;
  base: Obj;
};

export function addSettingsFixtures({ ok, bad, T0, ID, base }: SettingsCtx) {
  const notify = {
    events: { waiting: true, mention: true, result: true, blocked: true },
    digestAt: "09:00",
    quietHours: { enabled: true, start: "22:00", end: "08:00" },
    timeZone: "Asia/Shanghai",
    updatedAt: T0,
  };
  ok("http/notify-settings.json", "NotifySettings", notify);
  ok("http/notify-settings-defaults.json", "NotifySettings", { ...notify, digestAt: null, quietHours: { ...notify.quietHours, enabled: false }, updatedAt: 0 }, "never changed, no digest");
  bad("http/notify-settings-bad-time.json", "NotifySettings", { ...notify, digestAt: "24:00" }, "digest time out of range");
  ok("http/update-notify-settings.json", "UpdateNotifySettingsRequest", { events: { mention: false }, quietHours: { enabled: true }, digestAt: null });
  ok("http/update-notify-settings-empty.json", "UpdateNotifySettingsRequest", {}, "no change");
  bad("http/update-notify-settings-bad-zone.json", "UpdateNotifySettingsRequest", { timeZone: "Asia/Shang hai" }, "space in time zone");
  bad("http/update-notify-settings-bad-start.json", "UpdateNotifySettingsRequest", { quietHours: { start: "9:00" } }, "start needs two hour digits");

  const msgEvent = {
    ...base,
    type: "topic.updated",
    data: { topicId: ID.topic, status: "blocked" },
  };
  ok("server/event-notify-event-blocked.json", "ServerFrame", { t: "event", event: msgEvent, notify: "message", notifyEvent: "blocked" }, "a blocked topic: old clients see a plain notification");

  const digest = {
    type: "digest",
    at: T0 + 86_400_000,
    since: T0,
    counts: { waiting: 2, mention: 1, result: 3, blocked: 0 },
    items: [
      { groupId: ID.general, topicId: ID.topic, event: "waiting" },
      { groupId: ID.general, topicId: ID.topic, event: "result" },
    ],
  };
  ok("transient/digest.json", "TransientEvent", digest);
  ok("transient/digest-empty.json", "TransientEvent", { ...digest, counts: { waiting: 0, mention: 0, result: 0, blocked: 0 }, items: [] }, "nothing happened");
  bad("transient/digest-missing-counts.json", "TransientEvent", { type: "digest", at: T0, since: T0, items: [] }, "missing counts");

  const policy = {
    askBefore: { mergeMain: true, deployProd: true, prodConfig: true, deleteData: true, externalPublish: false },
    maxSpendCny: 50,
    approver: "owners",
    updatedAt: T0,
  };
  ok("http/approval-policy.json", "ApprovalPolicy", policy);
  ok("http/approval-policy-unlimited.json", "ApprovalPolicy", { ...policy, maxSpendCny: null, approver: "requester" }, "no spend limit");
  bad("http/approval-policy-zero-limit.json", "ApprovalPolicy", { ...policy, maxSpendCny: 0 }, "limit below 1");
  ok("http/update-approval-policy-unlimited.json", "UpdateApprovalPolicyRequest", { askBefore: { mergeMain: false }, maxSpendCny: null }, "no spend limit");
  ok("http/update-approval-policy-empty.json", "UpdateApprovalPolicyRequest", {}, "no change");
  ok("http/update-approval-policy.json", "UpdateApprovalPolicyRequest", { maxSpendCny: 100, approver: "requester" });
  bad("http/update-approval-policy-fraction.json", "UpdateApprovalPolicyRequest", { maxSpendCny: 20.5 }, "whole yuan only");

  const device = { id: "01JBX0TOKEN0000000000000AH", name: "Joye 的 MacBook Pro", createdAt: T0, lastUsedAt: T0 + 60_000, current: true };
  ok("http/device.json", "Device", device);
  ok("http/devices-response.json", "DevicesResponse", {
    devices: [device, { id: "01JBX0TOKEN0000000000000AJ", name: "", createdAt: T0 - 1000, lastUsedAt: null, current: false }],
  });
  bad("http/device-missing-current.json", "Device", { id: device.id, name: "x", createdAt: T0, lastUsedAt: null }, "missing current");

  ok("http/email-code-request.json", "EmailCodeRequest", { email: "joye@example.com" });
  bad("http/email-code-request-bad.json", "EmailCodeRequest", { email: "joye at example.com" }, "not an email address");
  ok("http/email-code-response.json", "EmailCodeResponse", { sent: true, expiresAt: T0 + 600_000 });
  ok("http/email-sign-in-request.json", "EmailSignInRequest", { email: "joye@example.com", code: "042817", deviceName: "Joye 的 MacBook Pro" });
  bad("http/email-sign-in-request-short-code.json", "EmailSignInRequest", { email: "joye@example.com", code: "4281", deviceName: "Mac" }, "code is six digits");
  ok("http/verify-email-request.json", "VerifyEmailRequest", { email: "joye@example.com", code: "042817" });
  ok("http/my-email-response.json", "MyEmailResponse", { email: "joye@example.com" });
  ok("http/my-email-response-none.json", "MyEmailResponse", { email: null }, "no sign-in email");
}
