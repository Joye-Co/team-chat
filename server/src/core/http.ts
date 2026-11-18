import type { z } from "zod";
import { ApiError, STATUS, toErrorBody } from "./errors";
import { logger } from "./logger";

export function json(data: unknown, status = 200, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...headers },
  });
}

export function errorResponse(err: unknown): Response {
  const body = toErrorBody(err);
  if (!(err instanceof ApiError)) logger.error("unhandled error", { err });
  // `Retry-After` is whole seconds; the exact wait is `retryAfterMs` in the body.
  const headers: HeadersInit = body.retryAfterMs !== undefined ? { "retry-after": String(Math.ceil(body.retryAfterMs / 1000)) } : {};
  return json({ error: body }, STATUS[body.code], headers);
}

/** Reads the request body as text, at most `limit` bytes. */
export async function readText(request: Request, limit: number): Promise<string> {
  const declared = request.headers.get("content-length");
  if (declared && Number(declared) > limit) throw new ApiError("too_large", "request body too large");
  if (!request.body) return "";
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      throw new ApiError("too_large", "request body too large");
    }
    chunks.push(value);
  }
  const all = new Uint8Array(size);
  let off = 0;
  for (const c of chunks) {
    all.set(c, off);
    off += c.byteLength;
  }
  return new TextDecoder().decode(all);
}

/** Reads and validates a JSON body with a zod schema. */
export async function readJson<S extends z.ZodType>(
  request: Request,
  schema: S,
  limit: number,
): Promise<z.infer<S>> {
  const text = await readText(request, limit);
  let raw: unknown;
  try {
    raw = text === "" ? {} : JSON.parse(text);
  } catch {
    throw new ApiError("invalid", "body is not valid JSON");
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw new ApiError("invalid", `invalid body: ${issueText(parsed.error)}`);
  }
  return parsed.data;
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

export interface CompiledRoute<H> {
  method: Method;
  path: string;
  regex: RegExp;
  keys: string[];
  handler: H;
}

export function compilePath(path: string): { regex: RegExp; keys: string[] } {
  const keys: string[] = [];
  const pattern = path
    .split("/")
    .map((seg) => {
      if (seg.startsWith(":")) {
        keys.push(seg.slice(1));
        return "([^/]+)";
      }
      // `*name` as the last segment matches the rest of the path, slashes included.
      if (seg.startsWith("*")) {
        keys.push(seg.slice(1));
        return "(.+)";
      }
      return seg.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    })
    .join("/");
  return { regex: new RegExp(`^${pattern}$`), keys };
}

export function matchRoute<H>(
  routes: CompiledRoute<H>[],
  method: string,
  pathname: string,
): { route: CompiledRoute<H>; params: Record<string, string> } | { route: null; pathMatched: boolean; allow: Method[] } {
  // The methods of the routes this path has, for the 405 answer to a wrong method.
  const allow: Method[] = [];
  for (const route of routes) {
    const m = route.regex.exec(pathname);
    if (!m) continue;
    if (route.method !== method) {
      if (!allow.includes(route.method)) allow.push(route.method);
      continue;
    }
    const params: Record<string, string> = {};
    try {
      route.keys.forEach((k, i) => {
        params[k] = decodeURIComponent(m[i + 1]);
      });
    } catch {
      throw new ApiError("invalid", "bad path encoding");
    }
    return { route, params };
  }
  return { route: null, pathMatched: allow.length > 0, allow };
}

/** 405 for a path that exists under other methods: code `invalid`, the methods in `Allow`. */
export function methodNotAllowed(allow: readonly string[]): Response {
  const list = allow.join(", ");
  return json({ error: { code: "invalid", message: `method not allowed: use ${list}` } }, 405, { allow: list });
}

/** Validates the query string with a zod schema; a failure names the parameter like `readJson` names a field. */
export function readQuery<S extends z.ZodType>(url: URL, schema: S): z.infer<S> {
  const parsed = schema.safeParse(Object.fromEntries(url.searchParams));
  if (!parsed.success) throw new ApiError("invalid", `invalid query: ${issueText(parsed.error)}`);
  return parsed.data;
}

/** `<path> <message>` of the first issue: which field broke which rule, never a value. */
export function issueText(error: z.ZodError): string {
  const first = error.issues[0];
  return `${first?.path.join(".") || "(root)"} ${first?.message ?? ""}`.trim();
}
