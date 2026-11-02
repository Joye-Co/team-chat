// One structured JSON logger. It never writes tokens, secrets, invite codes or message bodies:
// sensitive keys are dropped and sensitive-looking string values are masked.

export type LogLevel = "debug" | "info" | "warn" | "error";
export type LogFields = Record<string, unknown>;

const REDACTED = "[redacted]";
const SENSITIVE_KEY =
  /token|secret|password|passwd|authorization|cookie|invite_?code|^code$|^body$|^append$|^text$|^content$|^title$|api_?key|signature/i;
const TOKEN_VALUE = /tc_[A-Za-z0-9_-]{20,}/g;
const BEARER_VALUE = /Bearer\s+\S+/gi;
const INVITE_VALUE = /\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b/gi;

export function redact(value: unknown, depth = 0): unknown {
  if (depth > 6) return "[depth]";
  if (typeof value === "string") {
    return value
      .replace(TOKEN_VALUE, REDACTED)
      .replace(BEARER_VALUE, `Bearer ${REDACTED}`)
      .replace(INVITE_VALUE, REDACTED);
  }
  if (value instanceof Error) {
    return { name: value.name, message: redact(value.message, depth + 1) };
  }
  if (Array.isArray(value)) return value.slice(0, 50).map((v) => redact(v, depth + 1));
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = SENSITIVE_KEY.test(k) ? REDACTED : redact(v, depth + 1);
    }
    return out;
  }
  return value;
}

export type LogSink = (line: string, level: LogLevel) => void;

const defaultSink: LogSink = (line, level) => {
  if (level === "error") console.error(line);
  else if (level === "warn") console.warn(line);
  else console.log(line);
};

export class Logger {
  constructor(
    private readonly base: LogFields = {},
    private readonly sink: LogSink = defaultSink,
  ) {}

  child(fields: LogFields): Logger {
    return new Logger({ ...this.base, ...fields }, this.sink);
  }

  log(level: LogLevel, msg: string, fields: LogFields = {}): void {
    const record = redact({ ...this.base, ...fields }) as LogFields;
    this.sink(JSON.stringify({ level, msg: redact(msg), at: Date.now(), ...record }), level);
  }
  debug(msg: string, fields?: LogFields) {
    this.log("debug", msg, fields);
  }
  info(msg: string, fields?: LogFields) {
    this.log("info", msg, fields);
  }
  warn(msg: string, fields?: LogFields) {
    this.log("warn", msg, fields);
  }
  error(msg: string, fields?: LogFields) {
    this.log("error", msg, fields);
  }
}

export const logger = new Logger({ svc: "chat-api" });
