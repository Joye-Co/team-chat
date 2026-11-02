// Monitor configuration from Worker vars and secrets.

export interface Target {
  /** Environment name, e.g. "production". Also the alert-sink order: the TARGETS order. */
  name: string;
  baseUrl: string;
  /** Ops group id or exact group name, as the monitor bot sees it. */
  opsGroup: string;
  /** Name of the Worker secret with this target's monitor bot token. */
  tokenSecret: string;
  /**
   * Service binding to the target's API Worker. A Worker cannot reach another Worker on the
   * same workers.dev subdomain with a global fetch (Cloudflare error 1042), so deployed
   * targets go through a binding; `baseUrl` is then only the URL host in requests.
   */
  service?: string;
  /** Group of the round-trip messages (id or exact name). Default: `opsGroup`. A group people
   *  do not watch, so the stored round trip every 5 minutes is not noise next to the alerts. */
  probeGroup?: string;
  /** Member ids (of this env) mentioned on alert cards posted here, so the people on call are
   *  notified (a group's default notify level is "mentions"). Default: every human member of
   *  the ops group. */
  oncall?: string[];
}

export interface Env {
  MONITOR_STATE: DurableObjectNamespace<import("./state").MonitorState>;
  TARGETS: string;
  FAIL_THRESHOLD?: string;
  SUMMARY_HOUR_UTC?: string;
  /** Bearer secret of POST /test-alert (Worker secret). Unset: the route is off. */
  MONITOR_ADMIN_SECRET?: string;
  [secret: string]: unknown;
}

export interface Config {
  targets: Target[];
  failThreshold: number;
  summaryHourUtc: number;
}

export function readConfig(env: Env): Config {
  let raw: unknown;
  try {
    raw = JSON.parse(env.TARGETS);
  } catch {
    throw new Error("TARGETS is not valid JSON");
  }
  if (!Array.isArray(raw) || raw.length === 0) throw new Error("TARGETS must be a non-empty array");
  const targets = raw.map((t, i) => {
    const o = t as Record<string, unknown>;
    for (const k of ["name", "baseUrl", "opsGroup", "tokenSecret"]) {
      if (typeof o[k] !== "string" || o[k] === "") throw new Error(`TARGETS[${i}].${k} is required`);
    }
    for (const k of ["service", "probeGroup"]) {
      if (o[k] !== undefined && (typeof o[k] !== "string" || o[k] === "")) throw new Error(`TARGETS[${i}].${k} must be a string`);
    }
    if (o.oncall !== undefined && (!Array.isArray(o.oncall) || o.oncall.some((m) => typeof m !== "string" || m === ""))) {
      throw new Error(`TARGETS[${i}].oncall must be a list of member ids`);
    }
    return { ...(o as unknown as Target), baseUrl: (o.baseUrl as string).replace(/\/+$/, "") };
  });
  const failThreshold = Math.max(1, Number(env.FAIL_THRESHOLD ?? 2) || 2);
  const summaryHourUtc = Math.min(23, Math.max(0, Number(env.SUMMARY_HOUR_UTC ?? 1) || 0));
  return { targets, failThreshold, summaryHourUtc };
}

export function tokenFor(env: Env, t: Target): string | null {
  const v = env[t.tokenSecret];
  return typeof v === "string" && v !== "" ? v : null;
}

/** The fetch to reach a target: its service binding when it has one, else `fallback`. */
export function fetcherFor(env: Env, t: Target, fallback: typeof fetch): typeof fetch {
  if (!t.service) return fallback;
  const binding = env[t.service] as Fetcher | undefined;
  if (!binding || typeof binding.fetch !== "function") throw new Error(`service binding ${t.service} is missing`);
  return ((input: RequestInfo | URL, init?: RequestInit) => binding.fetch(input as RequestInfo, init)) as typeof fetch;
}
