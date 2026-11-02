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
//   bot --name <n> [--role <r>]        make a bot member, save its token
//   revoke --member <id>               revoke every token of a member
//   revoke --self                      revoke the saved owner token
//   members                            list members (no secrets)
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

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
  const product = JSON.parse(readFileSync(join(ROOT, "product.json"), "utf8")) as {
    servers?: Record<string, string>;
  };
  const url = product.servers?.[env];
  if (!url) fail(`no server URL for ${env} in product.json; pass --url`);
  return url.replace(/\/+$/, "");
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
    console.log(readFileSync(new URL(import.meta.url).pathname, "utf8").split("\n").slice(1, 15).join("\n"));
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
      console.log(`invite code: ${path}`);
      return;
    }
    case "bot": {
      const name = str(flags, "name", true) as string;
      const r = await call(base, "POST", "/v1/bots", {
        token: ownerToken(),
        body: { name, role: str(flags, "role") },
      });
      const path = join(dir, `bot-${slug(name)}-${r.member.id}.token`);
      writeSecret(path, r.token);
      console.log(`bot ${r.member.id} (${r.member.name})`);
      console.log(`bot token: ${path}`);
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
        console.log([m.id, m.kind, m.level, m.deactivated ? "deactivated" : "active", m.name, m.role ?? ""].join("\t"));
      }
      return;
    }
    default:
      fail(`unknown command: ${command}`);
  }
}

await main();
