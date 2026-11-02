// Monitor configuration from Worker vars and secrets.

export interface Target {
  /** Environment name, e.g. "production". Also the alert-sink order: the TARGETS order. */
  name: string;
  baseUrl: string;
  /** Ops group id or exact group name, as the monitor bot sees it. */
  opsGroup: string;
  /** Name of the Worker secret with this target's monitor bot token. */
  tokenSecret: string;
}

export interface Env {
  MONITOR_STATE: DurableObjectNamespace<import("./state").MonitorState>;
  TARGETS: string;
  FAIL_THRESHOLD?: string;
  SUMMARY_HOUR_UTC?: string;
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
