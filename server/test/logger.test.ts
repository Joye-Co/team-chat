import { describe, expect, it } from "vitest";
import { Logger, redact } from "../src/core/logger";

describe("logger", () => {
  it("never writes tokens, secrets, invite codes or message bodies", () => {
    const lines: string[] = [];
    const log = new Logger({ svc: "test" }, (line) => lines.push(line));
    const token = `tc_${"A".repeat(43)}`;
    log.info(`login with ${token}`, {
      token,
      authorization: `Bearer ${token}`,
      headers: { "x-admin-secret": "s3cret", Authorization: "Bearer abc" },
      inviteCode: "ABCD-EFGH-JKMN",
      note: "code is ABCD-EFGH-JKMN and token tc_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      frame: { t: "send", body: "private text", append: "more private" },
      err: new Error(`bad token ${token}`),
      groupId: "01GROUP",
      code: "XYZW-1234-5678",
    });
    expect(lines).toHaveLength(1);
    const out = lines[0];
    for (const s of [token, "s3cret", "ABCD-EFGH-JKMN", "private text", "more private", "XYZW-1234-5678", "tc_bbbb", "abc"]) {
      expect(out).not.toContain(s);
    }
    const parsed = JSON.parse(out);
    expect(parsed).toMatchObject({ level: "info", svc: "test", groupId: "01GROUP" });
    expect(parsed.frame.t).toBe("send");
  });

  it("keeps ordinary values", () => {
    expect(redact({ status: 200, path: "/v1/me", error: { code: "forbidden" } })).toEqual({
      status: 200,
      path: "/v1/me",
      error: { code: "forbidden" },
    });
  });
});
