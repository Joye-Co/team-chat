// R2 helpers: verified streaming uploads, bounded reads, Range downloads and safe headers.
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import type { Actor } from "../core/registry";

const HEX = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, "0"));

export function toHex(buf: ArrayBuffer | Uint8Array): string {
  const b = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  let s = "";
  for (const x of b) s += HEX[x];
  return s;
}

export async function sha256Bytes(data: Uint8Array): Promise<string> {
  return toHex(await crypto.subtle.digest("SHA-256", data));
}

export function actorOf(auth: AuthContext): Actor {
  return { id: auth.memberId, level: auth.member.level, kind: auth.member.kind, tokenId: auth.tokenId };
}

/** Throws `invalid` unless the request's Content-Length (when sent) equals `size`. */
export function checkDeclaredLength(request: Request, size: number): void {
  const declared = request.headers.get("content-length");
  if (declared !== null && Number(declared) !== size) {
    throw new ApiError("invalid", "body length does not match size");
  }
}

/**
 * Streams `body` into R2 at `key` without buffering it. The body must be exactly `size` bytes
 * with SHA-256 `sha256` (lowercase hex). On any mismatch `invalid` is thrown and this attempt
 * leaves no object behind; other storage failures throw `internal`.
 */
export async function putVerified(
  bucket: R2Bucket,
  key: string,
  body: ReadableStream<Uint8Array> | null,
  size: number,
  sha256: string,
  httpMetadata: R2HTTPMetadata,
): Promise<void> {
  const digest = new crypto.DigestStream("SHA-256");
  const writer = digest.getWriter();
  digest.digest.catch(() => {}); // rejects when aborted below
  let count = 0;
  let tooLong = false;
  let flushed = false;
  const source = body ?? new ReadableStream<Uint8Array>({ start: (c) => c.close() });
  const counted = source.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      async transform(chunk, ctl) {
        count += chunk.byteLength;
        if (count > size) {
          tooLong = true;
          throw new ApiError("invalid", "body longer than size");
        }
        await writer.write(chunk);
        ctl.enqueue(chunk);
      },
      async flush() {
        await writer.close();
        flushed = true;
      },
    }),
  );
  const fixed = new FixedLengthStream(size);
  const piping = counted.pipeTo(fixed.writable).catch(() => {});
  let stored: R2Object | null = null;
  try {
    stored = await bucket.put(key, fixed.readable, { sha256, httpMetadata });
  } catch {
    stored = null;
  }
  await piping;
  let hex: string | null = null;
  if (flushed && count === size) {
    hex = await digest.digest.then(toHex, () => null);
  } else {
    writer.abort().catch(() => {});
  }
  if (stored && hex === sha256) return;
  // Remove only our own object: a failed attempt must never delete the good object of another
  // upload of the same key (a client retry that finished first). R2 refused a put whose bytes
  // do not match `sha256`, so usually there is nothing of ours to remove.
  if (stored) {
    const current = await bucket.head(key).catch(() => null);
    if (current?.version === stored.version) await bucket.delete(key).catch(() => {});
  }
  if (tooLong || count !== size) throw new ApiError("invalid", "body length does not match size");
  if (hex !== sha256) throw new ApiError("invalid", "sha256 does not match the body");
  throw new ApiError("internal", "storage write failed");
}

/** Reads the whole body, at most `limit` bytes (`too_large` otherwise). */
export async function readBytes(request: Request, limit: number): Promise<Uint8Array> {
  const declared = request.headers.get("content-length");
  if (declared && Number(declared) > limit) throw new ApiError("too_large", "request body too large");
  if (!request.body) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel().catch(() => {});
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
  return all;
}

export type ByteRange = { offset: number; length: number };

/**
 * Parses a single-range `Range: bytes=...` header. Returns null to serve the whole body
 * (no header, other units or several ranges) and "unsatisfiable" for a range outside the body.
 */
export function parseRange(header: string | null, size: number): ByteRange | null | "unsatisfiable" {
  if (!header) return null;
  const m = /^bytes=\s*(\d*)\s*-\s*(\d*)\s*$/i.exec(header.trim());
  if (!m) return null;
  const [, a, b] = m;
  if (a === "" && b === "") return null;
  if (a === "") {
    const n = Number(b);
    if (n === 0 || size === 0) return "unsatisfiable";
    const length = Math.min(n, size);
    return { offset: size - length, length };
  }
  const start = Number(a);
  if (start >= size) return "unsatisfiable";
  const end = b === "" ? size - 1 : Math.min(Number(b), size - 1);
  if (end < start) return "unsatisfiable";
  return { offset: start, length: end - start + 1 };
}

/** `Content-Disposition` with an ASCII fallback and the UTF-8 name (RFC 6266 / 5987). */
export function contentDisposition(type: "inline" | "attachment", name: string): string {
  const ascii = name.replace(/[^\x20-\x7e]|["\\%]/g, "_");
  const encoded = encodeURIComponent(name).replace(
    /['()*]/g,
    (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`,
  );
  return `${type}; filename="${ascii}"; filename*=UTF-8''${encoded}`;
}

/** Image types the API may show inline; everything else downloads as an attachment. */
export const INLINE_MIMES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

/** Locks down a response from the API origin: no sniffing, no scripts, no framing. */
export const SAFE_HEADERS: Record<string, string> = {
  "x-content-type-options": "nosniff",
  "content-security-policy": "sandbox; default-src 'none'",
  "cross-origin-resource-policy": "same-origin",
};

export interface ServeOptions {
  size: number;
  mime: string;
  /** Sent as Content-Disposition when set. */
  disposition?: string;
  headers?: Record<string, string>;
}

/** Streams an R2 object with `Range` support (206 / 416). */
export async function serveObject(
  request: Request,
  bucket: R2Bucket,
  key: string,
  opts: ServeOptions,
): Promise<Response> {
  const range = parseRange(request.headers.get("range"), opts.size);
  const headers = new Headers({
    "content-type": opts.mime,
    "accept-ranges": "bytes",
    "cache-control": "private, max-age=3600",
    ...SAFE_HEADERS,
    ...(opts.headers ?? {}),
  });
  if (opts.disposition) headers.set("content-disposition", opts.disposition);
  if (range === "unsatisfiable") {
    headers.set("content-range", `bytes */${opts.size}`);
    return new Response(null, { status: 416, headers });
  }
  const obj = await bucket.get(key, range ? { range } : undefined);
  if (!obj) throw new ApiError("not_found", "content not found");
  headers.set("etag", obj.httpEtag);
  if (range) {
    headers.set("content-range", `bytes ${range.offset}-${range.offset + range.length - 1}/${opts.size}`);
    headers.set("content-length", String(range.length));
    return new Response(obj.body, { status: 206, headers });
  }
  headers.set("content-length", String(opts.size));
  return new Response(obj.body, { status: 200, headers });
}

/**
 * Fixed-window counter in D1. Adds `amount` and returns false when the window's total is over
 * `max`. Old windows are removed now and then.
 */
export async function rateHit(db: D1Database, key: string, windowMs: number, max: number, amount = 1): Promise<boolean> {
  const now = Date.now();
  const window = Math.floor(now / windowMs) * windowMs;
  const row = await db
    .prepare(
      `INSERT INTO files_rate_hits (key, window, count) VALUES (?, ?, ?)
       ON CONFLICT (key, window) DO UPDATE SET count = count + excluded.count RETURNING count`,
    )
    .bind(key, window, amount)
    .first<{ count: number }>();
  if (Math.random() < 0.02) {
    await db
      .prepare("DELETE FROM files_rate_hits WHERE window < ?")
      .bind(now - 24 * 3600_000)
      .run()
      .catch(() => {});
  }
  return (row?.count ?? amount) <= max;
}
