/** Logger the SDK writes to. Fields never hold tokens or other secrets. */
export interface Logger {
  debug(msg: string, fields?: Record<string, unknown>): void;
  info(msg: string, fields?: Record<string, unknown>): void;
  warn(msg: string, fields?: Record<string, unknown>): void;
  error(msg: string, fields?: Record<string, unknown>): void;
}

export type LogLevel = "debug" | "info" | "warn" | "error" | "silent";

const ORDER: Record<LogLevel, number> = { debug: 10, info: 20, warn: 30, error: 40, silent: 100 };

/** Removes anything that looks like a token or a secret from a log line. */
export function redact(text: string): string {
  return text
    .replace(/\b(tc|whk)_[A-Za-z0-9_-]{20,}/g, "$1_[redacted]")
    .replace(/(bearer\s+)[^\s"']+/gi, "$1[redacted]");
}

function errorText(err: unknown): string {
  if (err instanceof Error) return err.stack ?? `${err.name}: ${err.message}`;
  return String(err);
}

/** One JSON line per entry on stderr. */
export function consoleLogger(level: LogLevel = "info"): Logger {
  const min = ORDER[level];
  const write = (lvl: LogLevel, msg: string, fields?: Record<string, unknown>) => {
    if (ORDER[lvl] < min) return;
    const entry: Record<string, unknown> = { at: new Date().toISOString(), level: lvl, msg };
    if (fields) {
      for (const [k, v] of Object.entries(fields)) entry[k] = v instanceof Error ? errorText(v) : v;
    }
    let line: string;
    try {
      line = JSON.stringify(entry);
    } catch {
      line = JSON.stringify({ at: entry.at, level: lvl, msg });
    }
    console.error(redact(line));
  };
  return {
    debug: (m, f) => write("debug", m, f),
    info: (m, f) => write("info", m, f),
    warn: (m, f) => write("warn", m, f),
    error: (m, f) => write("error", m, f),
  };
}
