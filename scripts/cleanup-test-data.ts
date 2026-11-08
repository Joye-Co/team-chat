#!/usr/bin/env bun
// Lists, and with --apply cleans up, the test data that e2e and self-test runs leave in the e2e
// and staging envs. The API has no group or member delete, so a clean-up:
//   - revokes every token of each test member (owner route POST /v1/tokens/revoke), so a leaked
//     test token stops working;
//   - removes each test member from the groups that are not test groups (全员, 开发 ...), so the
//     team's groups list only the team;
//   - makes the owner leave each test group, so it no longer shows in the owner's sidebar.
// The rows stay in D1 and the room logs (the archive and restore keep working).
//
// Test members: names that start with "e2e-<run id>-", "dev-e2e-" or "selftest-".
// Test groups: names that start with "自测", "final-", "daemon e2e-", "e2e-" or equal "echo check".
// --keep <name,...> keeps members by name (for example the bot that staging-smoke uses).
// Production is refused. Without --apply nothing changes. Do not --apply while an e2e run uses
// the env: it revokes that run's tokens too.
//
// Usage: bun scripts/cleanup-test-data.ts --env <e2e|staging> [--apply] [--keep a,b]
// Report (names and counts, no tokens): .tmp/cleanup-test-data/<env>.json
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { deployedServer } from "./servers";

const ROOT = join(import.meta.dir, "..");
const args = process.argv.slice(2);
const opt = (name: string): string | undefined => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const ENV = opt("env");
if (ENV !== "e2e" && ENV !== "staging") {
  console.error("usage: bun scripts/cleanup-test-data.ts --env <e2e|staging> [--apply] [--keep a,b]");
  process.exit(2);
}
const APPLY = args.includes("--apply");
const KEEP = new Set((opt("keep") ?? "").split(",").filter(Boolean));
const BASE = (opt("base") ?? deployedServer(ENV) ?? "").replace(/\/$/, "");
if (!BASE) throw new Error(`no base URL for env ${ENV} (TC_OPS_DIR deploy/servers.json); pass --base`);
const CONF = join(process.env.TEAM_CHAT_CONFIG_DIR ?? join(homedir(), ".config/team-chat"), ENV);
const TOKEN = readFileSync(join(CONF, "owner-token"), "utf8").trim();

export const TEST_MEMBER = /^(e2e-[a-z0-9]{6}-|dev-e2e-|selftest-)/;
export const TEST_GROUP = /^(自测|final-|daemon e2e-|e2e-)|^echo check$/;

async function api(method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: { authorization: `Bearer ${TOKEN}`, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text.slice(0, 200)}`);
  return text ? JSON.parse(text) : null;
}

const me = await api("GET", "/v1/me");
if (me.level !== "owner") throw new Error("the owner token is not an owner's");
const members = (await api("GET", "/v1/members")) as { id: string; name: string; kind: string; deactivated?: boolean }[];
const groups = (await api("GET", "/v1/groups")) as { group: { id: string; name: string; kind: string; memberIds: string[] } }[];

const testMembers = members.filter((m) => TEST_MEMBER.test(m.name) && !KEEP.has(m.name) && m.id !== me.id);
const testIds = new Set(testMembers.map((m) => m.id));
const testGroups = groups.filter((g) => g.group.kind === "group" && TEST_GROUP.test(g.group.name));
const teamGroups = groups.filter((g) => g.group.kind === "group" && !TEST_GROUP.test(g.group.name));
const removals = teamGroups.flatMap((g) => g.group.memberIds.filter((id) => testIds.has(id)).map((id) => ({ groupId: g.group.id, group: g.group.name, memberId: id })));

const done = { revoked: 0, removed: 0, left: 0, errors: [] as string[] };
if (APPLY) {
  for (const m of testMembers) {
    try {
      done.revoked += (await api("POST", "/v1/tokens/revoke", { memberId: m.id, all: true })).revoked ?? 0;
    } catch (e) {
      done.errors.push(String(e));
    }
  }
  for (const r of removals) {
    try {
      await api("DELETE", `/v1/groups/${r.groupId}/members/${r.memberId}`);
      done.removed++;
    } catch (e) {
      done.errors.push(String(e));
    }
  }
  for (const g of testGroups) {
    try {
      await api("DELETE", `/v1/groups/${g.group.id}/members/${me.id}`);
      done.left++;
    } catch (e) {
      done.errors.push(String(e));
    }
  }
}

const report = {
  env: ENV,
  at: new Date().toISOString(),
  apply: APPLY,
  testMembers: testMembers.map((m) => m.name).sort(),
  testGroups: testGroups.map((g) => g.group.name).sort(),
  removals: removals.map((r) => `${r.group}: ${members.find((m) => m.id === r.memberId)?.name}`),
  kept: [...KEEP],
  done: APPLY ? done : null,
};
mkdirSync(join(ROOT, ".tmp/cleanup-test-data"), { recursive: true });
writeFileSync(join(ROOT, `.tmp/cleanup-test-data/${ENV}.json`), `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ ...report, testMembers: report.testMembers.length, removals: report.removals.length }, null, 2));
process.exit(done.errors.length ? 1 : 0);
