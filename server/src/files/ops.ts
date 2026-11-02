// Diagnostics uploads, app releases, update checks and the public DMG download.
import { MAX_DIAGNOSTICS_BYTES, MAX_FILE_BYTES, ReleaseVersion, type UpdateInfo } from "@chat/protocol";
import product from "../../../product.json";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import {
  INVITE_CODE_LENGTH,
  LEGACY_INVITE_CODE_LENGTH,
  normalizeInviteCode,
  sha256Hex,
  timingSafeEqualString,
} from "../core/crypto";
import type { Env } from "../core/env";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { registerRoute } from "../core/registry";
import { contentDisposition, putVerified, rateHit, readBytes, SAFE_HEADERS, serveObject } from "./store";

export const DIAGNOSTICS_PER_HOUR = 10;
/** Public download attempts per IP per window. */
export const DOWNLOAD_RATE = { windowMs: 10 * 60_000, max: 20 };
/** An `uploading` release row older than this may be replaced by a new upload. */
const STALE_UPLOAD_MS = 30 * 60_000;
const MAX_NOTES_CHARS = 20_000;

const diagnosticsKey = (memberId: string, id: string) => `diagnostics/${memberId}/${id}.gz`;
const releaseFile = (version: string) => `${product.slug}-${version}.dmg`;
const releaseKey = (version: string, file: string) => `releases/${version}/${file}`;

type ReleaseRow = {
  version: string;
  file_name: string;
  size: number;
  sha256: string;
  notes: string;
  status: "uploading" | "ready";
  created_at: number;
  published_at: number | null;
};

function toUpdateInfo(r: ReleaseRow, origin: string): UpdateInfo {
  return {
    version: r.version,
    notes: r.notes,
    size: r.size,
    sha256: r.sha256,
    publishedAt: r.published_at ?? r.created_at,
    downloadUrl: `${origin}/v1/updates/${r.version}/download`,
  };
}

async function latestRelease(db: D1Database): Promise<ReleaseRow | null> {
  return db
    .prepare(
      "SELECT * FROM releases WHERE status = 'ready' ORDER BY major DESC, minor DESC, patch DESC LIMIT 1",
    )
    .first<ReleaseRow>();
}

function serveRelease(request: Request, env: Env, r: ReleaseRow): Promise<Response> {
  return serveObject(request, env.FILES, releaseKey(r.version, r.file_name), {
    size: r.size,
    mime: "application/x-apple-diskimage",
    disposition: contentDisposition("attachment", r.file_name),
    headers: { "cache-control": "private, max-age=3600" },
  });
}

function plain(status: number, text: string): Response {
  return new Response(text, { status, headers: { "content-type": "text/plain; charset=utf-8", ...SAFE_HEADERS } });
}

function decodeNotes(header: string | null): string {
  if (!header) return "";
  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(atob(header.trim()), (c) => c.charCodeAt(0));
  } catch {
    throw new ApiError("invalid", "X-Release-Notes is not base64");
  }
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(bytes);
  } catch {
    throw new ApiError("invalid", "X-Release-Notes is not UTF-8");
  }
  if (text.length > MAX_NOTES_CHARS) throw new ApiError("invalid", "release notes too long");
  return text;
}

export function registerOpsRoutes(): void {
  // -------------------------------------------------------------------------
  // Diagnostics
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/diagnostics",
    auth: "member",
    async handler({ env, request, auth }) {
      const a = auth as AuthContext;
      const type = (request.headers.get("content-type") ?? "").split(";")[0].trim().toLowerCase();
      if (type !== "application/gzip") throw new ApiError("invalid", "Content-Type must be application/gzip");
      const since = Date.now() - 3600_000;
      const recent = await env.DB.prepare("SELECT count(*) AS n FROM diagnostics WHERE member_id = ? AND created_at > ?")
        .bind(a.memberId, since)
        .first<{ n: number }>();
      if ((recent?.n ?? 0) >= DIAGNOSTICS_PER_HOUR) throw new ApiError("rate_limited", "too many diagnostics uploads");
      const bytes = await readBytes(request, MAX_DIAGNOSTICS_BYTES);
      if (bytes.length < 18 || bytes[0] !== 0x1f || bytes[1] !== 0x8b) throw new ApiError("invalid", "body is not gzip");
      const now = Date.now();
      const id = ulid(now);
      await env.FILES.put(diagnosticsKey(a.memberId, id), bytes, { httpMetadata: { contentType: "application/gzip" } });
      await env.DB.batch([
        env.DB.prepare("INSERT INTO diagnostics (id, member_id, size, created_at) VALUES (?, ?, ?, ?)").bind(
          id,
          a.memberId,
          bytes.length,
          now,
        ),
        auditStatement(env.DB, a, { action: "diagnostics.upload", target: id, details: { size: bytes.length } }, now),
      ]);
      return { id };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/diagnostics",
    auth: "owner",
    async handler({ env, url }) {
      const limit = Math.min(Math.max(Number(url.searchParams.get("limit") ?? 50) || 50, 1), 200);
      const rows = await env.DB.prepare(
        "SELECT id, member_id, size, created_at FROM diagnostics ORDER BY created_at DESC, id DESC LIMIT ?",
      )
        .bind(limit)
        .all<{ id: string; member_id: string; size: number; created_at: number }>();
      return {
        entries: rows.results.map((r) => ({ id: r.id, memberId: r.member_id, size: r.size, createdAt: r.created_at })),
      };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/diagnostics/:id",
    auth: "owner",
    async handler({ env, params }) {
      const row = await env.DB.prepare("SELECT * FROM diagnostics WHERE id = ?")
        .bind(params.id)
        .first<{ id: string; member_id: string; size: number }>();
      if (!row) throw new ApiError("not_found", "diagnostics not found");
      const obj = await env.FILES.get(diagnosticsKey(row.member_id, row.id));
      if (!obj) throw new ApiError("not_found", "diagnostics not found");
      return new Response(obj.body, {
        headers: {
          "content-type": "application/gzip",
          "content-length": String(obj.size),
          "content-disposition": contentDisposition("attachment", `diagnostics-${row.id}.gz`),
          ...SAFE_HEADERS,
        },
      });
    },
  });

  // -------------------------------------------------------------------------
  // Releases and updates
  // -------------------------------------------------------------------------

  registerRoute({
    method: "PUT",
    path: "/v1/admin/releases/:version",
    auth: "none",
    async handler({ env, request, url, params }) {
      const given = request.headers.get("x-admin-secret") ?? "";
      const expected = env.ADMIN_SECRET ?? "";
      const ok = await timingSafeEqualString(given, expected);
      if (!expected || !ok) throw new ApiError("unauthorized", "invalid admin secret");
      const version = params.version;
      if (!ReleaseVersion.safeParse(version).success) throw new ApiError("invalid", "version must be MAJOR.MINOR.PATCH");
      const sha256 = (request.headers.get("x-sha256") ?? "").trim();
      if (!/^[0-9a-f]{64}$/.test(sha256)) throw new ApiError("invalid", "X-Sha256 must be 64 lowercase hex");
      const notes = decodeNotes(request.headers.get("x-release-notes"));
      const declared = request.headers.get("content-length");
      if (declared === null || !/^\d+$/.test(declared)) throw new ApiError("invalid", "Content-Length is required");
      const size = Number(declared);
      if (size > MAX_FILE_BYTES) throw new ApiError("too_large", "release too large");
      if (size === 0) throw new ApiError("invalid", "empty body");
      const [major, minor, patch] = version.split(".").map(Number);
      const file = releaseFile(version);
      const now = Date.now();

      // Claim the version first, so two uploads of one version never overwrite each other.
      const claim = await env.DB.prepare(
        `INSERT INTO releases (version, major, minor, patch, file_name, size, sha256, notes, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'uploading', ?)
         ON CONFLICT (version) DO UPDATE SET size = excluded.size, sha256 = excluded.sha256,
           notes = excluded.notes, created_at = excluded.created_at
         WHERE releases.status = 'uploading' AND releases.created_at < ?`,
      )
        .bind(version, major, minor, patch, file, size, sha256, notes, now, now - STALE_UPLOAD_MS)
        .run();
      if (claim.meta.changes !== 1) throw new ApiError("conflict", "release exists");

      try {
        await putVerified(env.FILES, releaseKey(version, file), request.body, size, sha256, {
          contentType: "application/x-apple-diskimage",
        });
      } catch (err) {
        await env.DB.prepare("DELETE FROM releases WHERE version = ? AND status = 'uploading' AND created_at = ?")
          .bind(version, now)
          .run();
        throw err;
      }
      const published = Date.now();
      await env.DB.batch([
        env.DB.prepare(
          "UPDATE releases SET status = 'ready', published_at = ? WHERE version = ? AND created_at = ?",
        ).bind(published, version, now),
        auditStatement(env.DB, null, { action: "release.publish", target: version, details: { size, sha256 } }, published),
      ]);
      const row = await env.DB.prepare("SELECT * FROM releases WHERE version = ?").bind(version).first<ReleaseRow>();
      return toUpdateInfo(row as ReleaseRow, url.origin);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/updates/latest",
    auth: "member",
    async handler({ env, url }) {
      const r = await latestRelease(env.DB);
      if (!r) throw new ApiError("not_found", "no release");
      return toUpdateInfo(r, url.origin);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/updates/:version/download",
    auth: "member",
    async handler({ env, request, params }) {
      const r = await env.DB.prepare("SELECT * FROM releases WHERE version = ? AND status = 'ready'")
        .bind(params.version)
        .first<ReleaseRow>();
      if (!r) throw new ApiError("not_found", "no such release");
      return serveRelease(request, env, r);
    },
  });

  // Public: anyone with an invite code that has not expired (used or not). The code is never
  // logged: the request log records only the path, and this handler logs nothing.
  registerRoute({
    method: "GET",
    path: "/download",
    auth: "none",
    async handler({ env, request, url }) {
      const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
      const key = `download:${await sha256Hex(ip)}`;
      if (!(await rateHit(env.DB, key, DOWNLOAD_RATE.windowMs, DOWNLOAD_RATE.max))) {
        return plain(429, "Too many requests. Try again later.\n");
      }
      const code = normalizeInviteCode(url.searchParams.get("code") ?? "");
      if (code.length !== INVITE_CODE_LENGTH && code.length !== LEGACY_INVITE_CODE_LENGTH) {
        return plain(404, "Invite code not found or expired.\n");
      }
      const invite = await env.DB.prepare("SELECT id FROM invites WHERE code_hash = ? AND expires_at > ?")
        .bind(await sha256Hex(code), Date.now())
        .first<{ id: string }>();
      if (!invite) return plain(404, "Invite code not found or expired.\n");
      const r = await latestRelease(env.DB);
      if (!r) return plain(404, "No release yet.\n");
      return serveRelease(request, env, r);
    },
  });
}
