#!/usr/bin/env bun
// Prints the newest sign-in code the local sandbox "sent" to an address (default: the sample
// owner's). The local server keeps codes in an outbox instead of sending email; the code is
// single use and works only on this machine's sandbox.
//
// Usage: bun scripts/sandbox/email-code.ts [email]
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { OWNER_EMAIL } from "./seed.ts";

const dir = join(import.meta.dir, "../../.tmp/sandbox");
const url = readFileSync(join(dir, "server-url"), "utf8").trim();
const admin = readFileSync(join(dir, "admin-secret"), "utf8").trim();
const email = process.argv[2] ?? OWNER_EMAIL;

const res = await fetch(`${url}/v1/admin/email-outbox?email=${encodeURIComponent(email)}`, {
  headers: { "x-admin-secret": admin },
});
if (!res.ok) throw new Error(`email-outbox: ${res.status}`);
const { codes } = (await res.json()) as { codes: { purpose: string; code: string; at: number }[] };
const newest = codes.find((c) => c.purpose === "sign_in") ?? codes[0];
if (!newest) {
  console.error(`no code for ${email}: ask for one in the app first`);
  process.exit(1);
}
console.log(newest.code);
