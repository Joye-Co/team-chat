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
  return json({ error: body }, STATUS[body.code]);
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
    const first = parsed.error.issues[0];
    throw new ApiError("invalid", `invalid body: ${first?.path.join(".") || "(root)"} ${first?.message ?? ""}`.trim());
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
      return seg.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    })
    .join("/");
  return { regex: new RegExp(`^${pattern}$`), keys };
}

export function matchRoute<H>(
  routes: CompiledRoute<H>[],
  method: string,
  pathname: string,
): { route: CompiledRoute<H>; params: Record<string, string> } | { route: null; pathMatched: boolean } {
  let pathMatched = false;
  for (const route of routes) {
    const m = route.regex.exec(pathname);
    if (!m) continue;
    pathMatched = true;
    if (route.method !== method) continue;
    const params: Record<string, string> = {};
    route.keys.forEach((k, i) => {
      params[k] = decodeURIComponent(m[i + 1]);
    });
    return { route, params };
  }
  return { route: null, pathMatched };
}
