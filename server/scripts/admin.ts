#!/usr/bin/env bun
// Admin helper for one environment. Secrets live in ~/.config/team-chat/<env>/ (mode 600):
//   admin-secret   ADMIN_SECRET of the Worker (you put it there; `local-secret` makes one)
//   owner-token    the owner's token (written by `bootstrap`)
// New tokens and invite codes are written to files there; this script prints only paths.
//
// Usage: bun scripts/admin.ts <command> --env <local|e2e|staging|production> [--url <base>] [options]
//   local-secret                       make a local ADMIN_SECRET and write server/.dev.vars
//   bootstrap --name <ownerName>       make the first owner, save its token
//   invite --name <n> [--kind human|bot] [--level member|owner] [--role <r>]
//   invite --self | --member <id>      sign-in code for the saved owner or an existing member
//                                      (signs in as that member, adds a device; single use)
//   bot --name <n> [--role <r>] [--character <id>]
//                                      make a bot member, save its token; <id> is a character id
//   set-member --member <id> [--character <id>|none] [--role <r>|none] [--level member|owner]
//                                      change a member's character, 岗位 or level (owner token)
//   revoke --member <id>               revoke every token of a member
//   revoke --self                      revoke the saved owner token
//   members                            list members (no secrets)
//   machine --name <n> [--labels a,b]  add a machine for the daemon, save its token (mode 600)
//   machines                           list machines and their health (no secrets)
//   machine-revoke --machine <id>      revoke a machine token and close its link
//   runner --bot <id> --runtime <r> [--labels a,b] [--repo-url <u> | --repo-path <p>]
//          [--base-branch <b>] [--model <m>] [--env-allow A,B] [--disable]
//                                      make a bot runner-backed (runs on a matching machine)
//   withdraw-release --version <v> [--restore]
//                                      stop offering a bad release (or offer it again); admin secret
//   revocations-save [--out <file>]    save revoked tokens/machines, deleted hooks, deactivated members
//                                      (ids and times, no secrets) before a D1 restore; admin secret
//   revocations-apply --file <file>    apply a saved list again after a D1 restore; admin secret
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { deployedServer } from "../../scripts/servers";

const ENVS = ["local", "e2e", "staging", "production"] as const;
type EnvName = (typeof ENVS)[number];

const LOCAL_URL = "http://127.0.0.1:8789";
const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../..");

function parseArgs(argv: string[]) {
  const [command, ...rest] = argv;
  const flags: Record<string, string | true> = {};
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    if (!a.startsWith("--")) fail(`unexpected argument: ${a}`);
    const key = a.slice(2);
    const next = rest[i + 1];
    if (next === undefined || next.startsWith("--")) flags[key] = true;
    else {
      flags[key] = next;
      i++;
    }
  }
  return { command, flags };
}

function fail(msg: string): never {
  console.error(`error: ${msg}`);
  process.exit(1);
}

function str(flags: Record<string, string | true>, key: string, required = false): string | undefined {
  const v = flags[key];
  if (v === undefined) {
    if (required) fail(`--${key} is required`);
    return undefined;
  }
  if (v === true) fail(`--${key} needs a value`);
  return v;
}

function configDir(env: EnvName): string {
  const dir = join(homedir(), ".config", "team-chat", env);
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  return dir;
}

function writeSecret(path: string, value: string): void {
  writeFileSync(path, `${value}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
}

function readSecret(path: string, what: string): string {
  if (!existsSync(path)) fail(`${what} not found: ${path}`);
  return readFileSync(path, "utf8").trim();
}

function baseUrl(env: EnvName, flag?: string): string {
  if (flag) return flag.replace(/\/+$/, "");
  if (env === "local") return LOCAL_URL;
  const url = deployedServer(env);
  if (!url) fail(`no server URL for ${env} (TC_OPS_DIR deploy/servers.json); pass --url`);
  return url;
}

function slug(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "member";
}

async function call(
  base: string,
  method: string,
  path: string,
  opts: { token?: string; adminSecret?: string; body?: unknown } = {},
): Promise<any> {
  const headers: Record<string, string> = {};
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  if (opts.adminSecret) headers["x-admin-secret"] = opts.adminSecret;
  if (opts.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${base}${path}`, {
    method,
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
  const text = await res.text();
  let data: any;
  try {
    data = JSON.parse(text);
  } catch {
    data = null;
  }
  if (!res.ok) {
    // Error bodies hold only a code and a message, never secrets.
    fail(`${method} ${path} -> ${res.status} ${data?.error?.code ?? ""} ${data?.error?.message ?? ""}`.trim());
  }
  return data;
}

async function main() {
  const { command, flags } = parseArgs(process.argv.slice(2));
  if (!command || command === "help" || flags.help) {
    console.log(readFileSync(new URL(import.meta.url).pathname, "utf8").split("\n").slice(1, 30).join("\n"));
    return;
  }
  const envName = (str(flags, "env") ?? "local") as EnvName;
  if (!ENVS.includes(envName)) fail(`--env must be one of ${ENVS.join(", ")}`);
  const dir = configDir(envName);
  const ownerTokenPath = join(dir, "owner-token");
  const adminSecretPath = join(dir, "admin-secret");
  const base = command === "local-secret" ? "" : baseUrl(envName, str(flags, "url"));
  const ownerToken = () => readSecret(ownerTokenPath, "owner token");

  switch (command) {
    case "local-secret": {
      if (envName !== "local") fail("local-secret is for --env local only");
      const secret = existsSync(adminSecretPath)
        ? readSecret(adminSecretPath, "admin secret")
        : Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url");
      writeSecret(adminSecretPath, secret);
      const devVars = join(ROOT, "server", ".dev.vars");
      writeSecret(devVars, `ADMIN_SECRET=${secret}`);
      console.log(`admin secret: ${adminSecretPath}`);
      console.log(`wrangler dev vars: ${devVars}`);
      return;
    }
    case "bootstrap": {
      const name = str(flags, "name", true) as string;
      if (existsSync(ownerTokenPath) && !flags.force) fail(`owner token already exists: ${ownerTokenPath}`);
      const r = await call(base, "POST", "/v1/admin/bootstrap", {
        adminSecret: readSecret(adminSecretPath, "admin secret"),
        body: { ownerName: name },
      });
      writeSecret(ownerTokenPath, r.token);
      console.log(`owner ${r.member.id} (${r.member.name}); group ${r.group.id}`);
      console.log(`owner token: ${ownerTokenPath}`);
      return;
    }
    case "invite": {
      const memberId = flags.self ? ((await call(base, "GET", "/v1/me", { token: ownerToken() })) as any).id : str(flags, "member");
      if (memberId) {
        // The server signs the code in as that member: name, kind and level come from it.
        const r = await call(base, "POST", "/v1/invites", {
          token: ownerToken(),
          body: { name: "sign-in", kind: "human", level: "member", memberId },
        });
        const path = join(dir, `signin-${memberId}-${Date.now()}.txt`);
        writeSecret(path, r.code);
        console.log(`sign-in code for member ${memberId}, expires ${new Date(r.expiresAt).toISOString()}`);
        console.log(`sign-in code (XXXX-XXXX, single use): ${path}`);
        return;
      }
      const name = str(flags, "name", true) as string;
      const body = {
        name,
        kind: str(flags, "kind") ?? "human",
        level: str(flags, "level") ?? "member",
        role: str(flags, "role"),
      };
      const r = await call(base, "POST", "/v1/invites", { token: ownerToken(), body });
      const path = join(dir, `invite-${slug(name)}-${Date.now()}.txt`);
      writeSecret(path, r.code);
      console.log(`invite for ${name}, expires ${new Date(r.expiresAt).toISOString()}`);
      // The code (XXXX-XXXX, single use) is a secret: only its file path is printed.
      console.log(`invite code (XXXX-XXXX, single use): ${path}`);
      return;
    }
    case "bot": {
      const name = str(flags, "name", true) as string;
      const r = await call(base, "POST", "/v1/bots", {
        token: ownerToken(),
        body: { name, role: str(flags, "role"), character: str(flags, "character") },
      });
      const path = join(dir, `bot-${slug(name)}-${r.member.id}.token`);
      writeSecret(path, r.token);
      console.log(`bot ${r.member.id} (${r.member.name})`);
      console.log(`bot token: ${path}`);
      return;
    }
    case "set-member": {
      const member = str(flags, "member", true) as string;
      // "none" clears a field; an absent flag leaves it unchanged.
      const nullable = (k: string) => {
        const v = str(flags, k);
        return v === undefined ? undefined : v === "none" ? null : v;
      };
      const body = { character: nullable("character"), role: nullable("role"), level: str(flags, "level") };
      if (Object.values(body).every((v) => v === undefined)) fail("pass --character, --role or --level");
      const r = await call(base, "PATCH", `/v1/members/${member}`, { token: ownerToken(), body });
      console.log([r.id, r.kind, r.level, r.name, r.role ?? "", r.character ?? ""].join("\t"));
      return;
    }
    case "revoke": {
      const member = str(flags, "member");
      if (!member && !flags.self) fail("pass --member <id> or --self");
      const body = member ? { memberId: member, all: true } : {};
      const r = await call(base, "POST", "/v1/tokens/revoke", { token: ownerToken(), body });
      console.log(`revoked ${r.revoked} token(s)`);
      return;
    }
    case "members": {
      const r = (await call(base, "GET", "/v1/members", { token: ownerToken() })) as any[];
      for (const m of r) {
        console.log([m.id, m.kind, m.level, m.deactivated ? "deactivated" : "active", m.name, m.role ?? "", m.character ?? ""].join("\t"));
      }
      return;
    }
    case "machine": {
      const name = str(flags, "name", true) as string;
      const labels = (str(flags, "labels") ?? "").split(",").map((l) => l.trim()).filter(Boolean);
      const r = await call(base, "POST", "/v1/machines", { token: ownerToken(), body: { name, labels } });
      const path = join(dir, `machine-${slug(name)}-${r.machine.id}.token`);
      writeSecret(path, r.token);
      console.log(`machine ${r.machine.id} (${r.machine.name})`);
      console.log(`machine token: ${path}`);
      return;
    }
    case "machines": {
      const r = (await call(base, "GET", "/v1/machines", { token: ownerToken() })) as any[];
      for (const m of r) {
        const free = m.health ? `${Math.round(m.health.diskFreeBytes / 1e9)}GB free` : "";
        const state = m.revoked ? "revoked" : m.healthy ? "healthy" : m.connected ? "connected" : "offline";
        console.log([m.id, state, m.name, m.labels.join(","), m.daemonVersion ?? "", free, `${m.runningRuns.length} runs`].join("\t"));
      }
      return;
    }
    case "machine-revoke": {
      const id = str(flags, "machine", true) as string;
      await call(base, "DELETE", `/v1/machines/${id}`, { token: ownerToken() });
      console.log(`revoked machine ${id}`);
      return;
    }
    case "runner": {
      const bot = str(flags, "bot", true) as string;
      const list = (k: string) => (str(flags, k) ?? "").split(",").map((l) => l.trim()).filter(Boolean);
      const repoUrl = str(flags, "repo-url");
      const repoPath = str(flags, "repo-path");
      const body = {
        runtime: str(flags, "runtime", true),
        labels: list("labels"),
        repo: repoUrl ? { url: repoUrl } : repoPath ? { path: repoPath } : undefined,
        baseBranch: str(flags, "base-branch"),
        model: str(flags, "model"),
        envAllowlist: list("env-allow"),
        enabled: !flags.disable,
      };
      const r = await call(base, "PUT", `/v1/bots/${bot}/runner`, { token: ownerToken(), body });
      console.log(`bot ${r.botId}: runtime ${r.runtime}, labels [${r.labels.join(",")}], ${r.enabled ? "enabled" : "disabled"}`);
      return;
    }
    case "withdraw-release": {
      const version = str(flags, "version", true) as string;
      const r = await call(base, "POST", `/v1/admin/releases/${encodeURIComponent(version)}/withdraw`, {
        adminSecret: readSecret(adminSecretPath, "admin secret"),
        body: { restore: flags.restore === true },
      });
      console.log(`${r.version}: ${r.withdrawn ? "withdrawn" : "offered again"}; latest is now ${r.latest?.version ?? "(none)"}`);
      return;
    }
    case "revocations-save": {
      const r = await call(base, "GET", "/v1/admin/revocations", { adminSecret: readSecret(adminSecretPath, "admin secret") });
      const out = str(flags, "out") ?? join(dir, `revocations-${new Date(r.at).toISOString().replace(/[:.]/g, "-")}.json`);
      writeSecret(out, JSON.stringify(r));
      console.log(`saved ${r.tokens.length} tokens, ${r.machines.length} machines, ${r.hooks.length} hooks, ${r.members.length} members`);
      console.log(`revocations: ${out}`);
      return;
    }
    case "revocations-apply": {
      const file = str(flags, "file", true) as string;
      const body = JSON.parse(readSecret(file, "revocations file"));
      const r = await call(base, "POST", "/v1/admin/revocations", { adminSecret: readSecret(adminSecretPath, "admin secret"), body });
      console.log(`applied ${r.applied} revocation(s) again`);
      return;
    }
    default:
      fail(`unknown command: ${command}`);
  }
}

await main();
