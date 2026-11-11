// Push fixtures: device registration, the badge, push data and answering from a notification.
// Called by gen-fixtures.ts.
export type PushCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  T0: number;
  ID: Record<string, string>;
};

export function addPushFixtures({ ok, bad, T0, ID }: PushCtx) {
  const token = "a1".repeat(32);
  const register = { platform: "apns", token, appId: "dev.example.chat", environment: "production" };
  ok("http/register-push-device-request.json", "RegisterPushDeviceRequest", register);
  ok("http/register-push-device-request-sandbox.json", "RegisterPushDeviceRequest", { ...register, environment: "sandbox" }, "development build");
  bad("http/register-push-device-request-upper-token.json", "RegisterPushDeviceRequest", { ...register, token: token.toUpperCase() }, "token must be lower-case hex");
  bad("http/register-push-device-request-short-token.json", "RegisterPushDeviceRequest", { ...register, token: "abcd" }, "token too short");
  bad("http/register-push-device-request-bad-platform.json", "RegisterPushDeviceRequest", { ...register, platform: "sms" }, "unknown platform");
  bad("http/register-push-device-request-bad-app.json", "RegisterPushDeviceRequest", { ...register, appId: "chat" }, "app id needs a dot");

  const device = { id: "01JBX0PUSH00000000000000AA", platform: "apns", appId: "dev.example.chat", environment: "production", createdAt: T0, updatedAt: T0 + 1000, current: true };
  ok("http/push-device.json", "PushDevice", device);
  ok("http/push-devices-response.json", "PushDevicesResponse", { devices: [device, { ...device, id: "01JBX0PUSH00000000000000AB", environment: "sandbox", current: false }] });
  ok("http/push-devices-response-empty.json", "PushDevicesResponse", { devices: [] });
  bad("http/push-device-missing-current.json", "PushDevice", { ...device, current: undefined }, "current is required");
  ok("http/remove-push-device-response.json", "RemovePushDeviceResponse", { removed: 1 });
  ok("http/badge-response.json", "BadgeResponse", { waiting: 3 });
  bad("http/badge-response-negative.json", "BadgeResponse", { waiting: -1 }, "count is not negative");

  const ask = { messageId: ID.msg, rev: 2, actions: [{ id: "ok", label: "可以" }, { id: "no", label: "先不" }] };
  ok("http/push-data-ask.json", "PushData", { v: 1, reason: "waiting", notifyEvent: "waiting", groupId: ID.general, topicId: ID.topic, messageId: ID.msg, seq: 42, ask });
  ok("http/push-data-mention.json", "PushData", { v: 1, reason: "mention", notifyEvent: "mention", groupId: ID.general, topicId: ID.topic, messageId: ID.msg, seq: 7 });
  ok("http/push-data-minimal.json", "PushData", { v: 1, reason: "result", groupId: ID.general, seq: 1 }, "an event without topic or message");
  bad("http/push-data-bad-version.json", "PushData", { v: 2, reason: "message", groupId: ID.general, seq: 1 }, "unknown version");
  bad("http/push-data-too-many-actions.json", "PushData", { v: 1, reason: "waiting", groupId: ID.general, seq: 1, ask: { ...ask, actions: [...ask.actions, { id: "a", label: "A" }, { id: "b", label: "B" }] } }, "at most 3 actions");
  bad("http/push-data-no-actions.json", "PushData", { v: 1, reason: "waiting", groupId: ID.general, seq: 1, ask: { ...ask, actions: [] } }, "an ask needs an action");

  ok("http/answer-card-request.json", "AnswerCardRequest", { rev: 2, actionId: "ok", clientEventId: "push-01JBX0" });
  ok("http/answer-card-request-minimal.json", "AnswerCardRequest", { rev: 1, actionId: "no" });
  bad("http/answer-card-request-zero-rev.json", "AnswerCardRequest", { rev: 0, actionId: "ok" }, "rev starts at 1");
  bad("http/answer-card-request-bad-action.json", "AnswerCardRequest", { rev: 1, actionId: "可以" }, "action ids are ASCII");
}
