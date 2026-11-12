// Sign-in fixtures: password sign-in and QR sign-in between two devices. Called by
// gen-fixtures.ts. The passwords, codes, secrets and tokens are made up.
export type SignInCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string, reason?: string) => void;
  ID: Record<string, string>;
};

export function addSignInFixtures({ ok, bad, ID }: SignInCtx) {
  const at = 1_762_800_000_000;
  const expiresAt = at + 300_000;

  // Password
  ok("http/my-password-response.json", "MyPasswordResponse", { set: true, updatedAt: at });
  ok("http/my-password-response-none.json", "MyPasswordResponse", { set: false, updatedAt: null });
  ok("http/password-sign-in-request.json", "PasswordSignInRequest", { email: "joye@example.com", password: "hunter2hunter2", deviceName: "Joye 的 iPhone" });
  bad("http/password-sign-in-request-empty.json", "PasswordSignInRequest", { email: "joye@example.com", password: "", deviceName: "iPhone" }, "password is not empty");
  ok("http/set-password-request.json", "SetPasswordRequest", { password: "correct horse 电池", currentPassword: "old-pass-1" });
  ok("http/set-password-request-first.json", "SetPasswordRequest", { password: "hunter2hunter2" });
  bad("http/set-password-request-short.json", "SetPasswordRequest", { password: "1234567" }, "password is at least 8 characters");
  bad("http/set-password-request-spaces.json", "SetPasswordRequest", { password: "          " }, "password is not only spaces");
  ok("http/remove-password-request.json", "RemovePasswordRequest", { currentPassword: "hunter2hunter2" });
  ok("http/remove-password-request-fresh.json", "RemovePasswordRequest", {});

  // QR: a signed-in device offers a code and a new device claims it ...
  ok("http/qr-offer-response.json", "QrOfferResponse", { id: "01JQROFFER000000000000000", code: "7K2M-9QPX", expiresAt });
  ok("http/qr-offer-status.json", "QrOfferStatus", { state: "used", deviceName: "Joye 的 iPhone" });
  ok("http/qr-offer-status-waiting.json", "QrOfferStatus", { state: "waiting" });
  ok("http/qr-claim-request.json", "QrClaimRequest", { code: "7k2m 9qpx", deviceName: "Joye 的 iPhone" });
  bad("http/qr-claim-request-no-device.json", "QrClaimRequest", { code: "7K2M-9QPX", deviceName: "" }, "deviceName is not empty");

  // ... or a new device requests one and a signed-in device approves it.
  const requestId = "01JQRREQ00000000000000000";
  const secret = "Zm9vYmFyYmF6cXV4cXV1eHF1dXhxdXV4cXV1eHF1dXg";
  ok("http/qr-request-request.json", "QrRequestRequest", { deviceName: "Joye 的 MacBook Pro" });
  ok("http/qr-request-response.json", "QrRequestResponse", { id: requestId, code: "H3VD-2WQ8", secret, expiresAt });
  ok("http/qr-code-request.json", "QrCodeRequest", { code: "H3VD-2WQ8" });
  ok("http/qr-peek-response.json", "QrPeekResponse", { deviceName: "Joye 的 MacBook Pro", expiresAt });
  ok("http/qr-poll-request.json", "QrPollRequest", { id: requestId, secret });
  ok("http/qr-poll-response-waiting.json", "QrPollResponse", { state: "waiting" });
  ok("http/qr-poll-response-approved.json", "QrPollResponse", {
    state: "approved",
    token: `tc_${"a".repeat(43)}`,
    member: { id: ID.alice, kind: "human", name: "Alice", level: "member" },
  });
}
