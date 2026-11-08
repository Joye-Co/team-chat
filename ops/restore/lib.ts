// Shared helpers for the restore tools. Secrets are read from files and never printed.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { deployedServer } from "../../scripts/servers";

export const ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../..");
export const ENVS = ["local", "e2e", "staging", "production"] as const;
export type EnvName = (typeof ENVS)[number];
export const LOCAL_URL = "http://127.0.0.1:8789";

export function fail(msg: string): never {
  console.error(`error: ${msg}`);
  process.exit(1);
}

export function parseArgs(argv: string[]): { command: string | undefined; flags: Record<string, string | true> } {
  const [command, ...rest] = argv;
  const flags: Record<string, string | true> = {};
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    if (!a.startsWith("--")) fail(`unexpected argument: ${a}`);
    const next = rest[i + 1];
    if (next === undefined || next.startsWith("--")) flags[a.slice(2)] = true;
    else {
      flags[a.slice(2)] = next;
      i++;
    }
  }
  return { command, flags };
}

export function flag(flags: Record<string, string | true>, key: string): string | undefined {
  const v = flags[key];
  if (v === true) fail(`--${key} needs a value`);
  return v;
}

export function configDir(env: string): string {
  return join(homedir(), ".config", "team-chat", env);
}

export function readSecretFile(path: string, what: string): string {
  if (!existsSync(path)) fail(`${what} not found: ${path}`);
  const mode = statSync(path).mode & 0o077;
  if (mode !== 0) fail(`${what} must be mode 600: ${path}`);
  return readFileSync(path, "utf8").trim();
}

export function serverUrl(env: EnvName, override?: string): string {
  if (override) return override.replace(/\/+$/, "");
  if (env === "local") return LOCAL_URL;
  const url = deployedServer(env);
  if (!url) fail(`no server URL for ${env} (TC_OPS_DIR deploy/servers.json); pass the URL flag`);
  return url;
}

/** One environment's admin API: base URL plus ADMIN_SECRET from <config>/<env>/admin-secret. */
export class AdminApi {
  constructor(
    readonly base: string,
    private readonly secret: string,
  ) {}

  static fromFlags(flags: Record<string, string | true>, prefix: "from" | "to"): AdminApi {
    const env = (flag(flags, prefix) ?? fail(`--${prefix} <env> is required`)) as EnvName;
    if (!ENVS.includes(env)) fail(`--${prefix} must be one of ${ENVS.join(", ")}`);
    const base = serverUrl(env, flag(flags, `${prefix}-url`));
    const secretFile = flag(flags, `${prefix}-secret-file`) ?? join(configDir(env), "admin-secret");
    return new AdminApi(base, readSecretFile(secretFile, "admin secret"));
  }

  async request(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    headers.set("x-admin-secret", this.secret);
    for (let attempt = 0; ; attempt++) {
      try {
        const res = await fetch(`${this.base}${path}`, { ...init, headers });
        if (res.status >= 500 && attempt < 3) {
          await Bun.sleep(500 * 2 ** attempt);
          continue;
        }
        return res;
      } catch (err) {
        if (attempt >= 3) throw err;
        await Bun.sleep(500 * 2 ** attempt);
      }
    }
  }

  async json<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await this.request(path, init);
    const text = await res.text();
    if (!res.ok) throw new Error(`${init.method ?? "GET"} ${path}: ${res.status} ${text.slice(0, 300)}`);
    return JSON.parse(text) as T;
  }
}

/** Where archive objects come from: an env's admin API, or a local folder with the same layout. */
export interface ArchiveSource {
  describe: string;
  list(prefix: string): Promise<string[]>;
  get(key: string): Promise<Uint8Array>;
}

export function httpSource(api: AdminApi): ArchiveSource {
  return {
    describe: api.base,
    async list(prefix) {
      const keys: string[] = [];
      let cursor: string | null = null;
      do {
        const q = new URLSearchParams({ prefix });
        if (cursor) q.set("cursor", cursor);
        const page: { objects: { key: string }[]; cursor: string | null } = await api.json(`/v1/admin/archive?${q}`);
        keys.push(...page.objects.map((o) => o.key));
        cursor = page.cursor;
      } while (cursor);
      return keys.sort();
    },
    async get(key) {
      const res = await api.request(`/v1/admin/archive/object?key=${encodeURIComponent(key)}`);
      if (!res.ok) throw new Error(`archive object ${key}: ${res.status}`);
      return new Uint8Array(await res.arrayBuffer());
    },
  };
}

/** A folder that holds `archive/...` (for example downloaded from the R2 dashboard). */
export function dirSource(dir: string): ArchiveSource {
  const root = resolve(dir);
  const walk = (d: string): string[] =>
    existsSync(d)
      ? readdirSync(d, { withFileTypes: true }).flatMap((e) =>
          e.isDirectory() ? walk(join(d, e.name)) : [relative(root, join(d, e.name))],
        )
      : [];
  return {
    describe: root,
    async list(prefix) {
      return walk(root)
        .map((k) => k.split("\\").join("/"))
        .filter((k) => k.startsWith(prefix))
        .sort();
    },
    async get(key) {
      if (key.includes("..")) fail(`bad key ${key}`);
      return new Uint8Array(readFileSync(join(root, key)));
    },
  };
}

export function gunzipText(bytes: Uint8Array): string {
  return new TextDecoder().decode(Bun.gunzipSync(bytes));
}

export function parseGroupFileKey(key: string): { groupId: string; from: number; to: number } | null {
  const m = /^archive\/groups\/([^/]+)\/(\d+)-(\d+)\.ndjson\.gz$/.exec(key);
  return m ? { groupId: m[1], from: Number(m[2]), to: Number(m[3]) } : null;
}

export function parseStateKey(key: string): { groupId: string; date: string } | null {
  const m = /^archive\/groups\/([^/]+)\/state\/(\d{4}-\d{2}-\d{2})\.json\.gz$/.exec(key);
  return m ? { groupId: m[1], date: m[2] } : null;
}

export function parseMemberStateKey(key: string): { memberId: string; date: string } | null {
  const m = /^archive\/members\/([^/]+)\/state\/(\d{4}-\d{2}-\d{2})\.json\.gz$/.exec(key);
  return m ? { memberId: m[1], date: m[2] } : null;
}

export function parseD1Key(key: string): string | null {
  return /^archive\/d1\/(\d{4}-\d{2}-\d{2})\.json\.gz$/.exec(key)?.[1] ?? null;
}

export interface D1Dump {
  version: 1;
  environment: string;
  createdAt: number;
  tables: { name: string; columns: string[]; rows: unknown[][] }[];
}
