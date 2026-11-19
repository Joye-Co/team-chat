import { applyD1Migrations, env } from "cloudflare:test";
import { afterAll, expect } from "vitest";
import { downViolations } from "./daemon";
import { protocolViolations, sockets } from "./helpers";

await applyD1Migrations(env.DB, env.TEST_MIGRATIONS);

// Runs at the end of every test file: each frame a test client or a fake daemon got must match
// the protocol schema. A new test file needs no code for this.
afterAll(async () => {
  // Let close handlers (presence fan-out) finish before the runtime shuts down.
  if (sockets.opened > 0) await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});
