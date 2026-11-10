// Diagnostics uploads, app releases (the DMG and the Windows installer), update checks and the
// public download.
import { MAX_DIAGNOSTICS_BYTES, MAX_FILE_BYTES, ReleasePlatform, ReleaseVersion, type UpdateInfo } from "@chat/protocol";
import { z } from "zod";
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
const releaseKey = (version: string, file: string) => `releases/${version}/${file}`;

/**
 * One file per release and platform. The DMG rows are the `releases` table, as before the
 * Windows client; every other platform is a row of `release_assets`. A request that names no
 * platform means the DMG, so clients and scripts from before keep working unchanged.
 */
const PLATFORMS: Record<ReleasePlatform, { file: (version: string) => string; mime: string; table: string; where: string }> = {
  macos: {
    file: (version) => `${product.slug}-${version}.dmg`,
    mime: "application/x-apple-diskimage",
    table: "releases",
    where: "",
  },
  windows: {
    file: (version) => `${product.slug}-${version}-setup.exe`,
    mime: "application/vnd.microsoft.portable-executable",
    table: "release_assets",
    where: "platform = 'windows' AND ",
  },
};

/** The `platform` query parameter; missing = the DMG. */
function platformOf(raw: string | null): ReleasePlatform {
  if (raw === null) return "macos";
  const p = ReleasePlatform.safeParse(raw);
  if (!p.success) throw new ApiError("invalid", "unknown platform");
  return p.data;
}

type ReleaseRow = {
  version: string;
  file_name: string;
  size: number;
  sha256: string;
  notes: string;
  status: "uploading" | "ready";
  created_at: number;
  published_at: number | null;
  withdrawn_at: number | null;
};

function toUpdateInfo(r: ReleaseRow, origin: string, platform: ReleasePlatform = "macos"): UpdateInfo {
  return {
    version: r.version,
    notes: r.notes,
    size: r.size,
    sha256: r.sha256,
    publishedAt: r.published_at ?? r.created_at,
    // The DMG's URL is the one old clients know; other platforms name themselves.
    downloadUrl: `${origin}/v1/updates/${r.version}/download${platform === "macos" ? "" : `?platform=${platform}`}`,
    platform,
  };
}

async function latestRelease(db: D1Database, platform: ReleasePlatform = "macos"): Promise<ReleaseRow | null> {
  const p = PLATFORMS[platform];
  return db
    .prepare(
      `SELECT * FROM ${p.table} WHERE ${p.where}status = 'ready' AND withdrawn_at IS NULL ORDER BY major DESC, minor DESC, patch DESC LIMIT 1`,
    )
    .first<ReleaseRow>();
}

function serveRelease(request: Request, env: Env, r: ReleaseRow, platform: ReleasePlatform = "macos"): Promise<Response> {
  return serveObject(request, env.FILES, releaseKey(r.version, r.file_name), {
    size: r.size,
    mime: PLATFORMS[platform].mime,
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
    handler: ({ env, request, url, params }) => publishRelease(env, request, url, params.version, "macos"),
  });

  // The file of a release for one platform. `macos` is the same upload as the route above.
  registerRoute({
    method: "PUT",
    path: "/v1/admin/releases/:version/assets/:platform",
    auth: "none",
    handler: ({ env, request, url, params }) => publishRelease(env, request, url, params.version, platformOf(params.platform)),
  });

  // Withdraw a bad release: it is no longer offered as latest (the previous ready release is)
  // and cannot be downloaded. `{ "restore": true }` offers it again. See docs/RUNBOOK.md.
  registerRoute({
    method: "POST",
    path: "/v1/admin/releases/:version/withdraw",
    auth: "none",
    async handler({ env, request, url, params, body }) {
      const given = request.headers.get("x-admin-secret") ?? "";
      const expected = env.ADMIN_SECRET ?? "";
      const ok = await timingSafeEqualString(given, expected);
      if (!expected || !ok) throw new ApiError("unauthorized", "invalid admin secret");
      const b = await body(z.object({ restore: z.boolean().optional() }));
      // A version is withdrawn as a whole: its DMG and the files of its other platforms.
      const row =
        (await env.DB.prepare("SELECT * FROM releases WHERE version = ? AND status = 'ready'")
          .bind(params.version)
          .first<ReleaseRow>()) ??
        (await env.DB.prepare("SELECT * FROM release_assets WHERE version = ? AND status = 'ready' LIMIT 1")
          .bind(params.version)
          .first<ReleaseRow>());
      if (!row) throw new ApiError("not_found", "no such release");
      const now = Date.now();
      await env.DB.batch([
        env.DB.prepare("UPDATE releases SET withdrawn_at = ? WHERE version = ?").bind(b.restore ? null : now, row.version),
        env.DB.prepare("UPDATE release_assets SET withdrawn_at = ? WHERE version = ?").bind(b.restore ? null : now, row.version),
        auditStatement(env.DB, null, { action: b.restore ? "release.restore" : "release.withdraw", target: row.version }, now),
      ]);
      const latest = await latestRelease(env.DB);
      return { version: row.version, withdrawn: !b.restore, latest: latest ? toUpdateInfo(latest, url.origin) : null };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/updates/latest",
    auth: "member",
    async handler({ env, url }) {
      const platform = platformOf(url.searchParams.get("platform"));
      const r = await latestRelease(env.DB, platform);
      if (!r) throw new ApiError("not_found", "no release");
      return toUpdateInfo(r, url.origin, platform);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/updates/:version/download",
    auth: "member",
    async handler({ env, request, url, params }) {
      const platform = platformOf(url.searchParams.get("platform"));
      const p = PLATFORMS[platform];
      const r = await env.DB.prepare(
        `SELECT * FROM ${p.table} WHERE ${p.where}version = ? AND status = 'ready' AND withdrawn_at IS NULL`,
      )
        .bind(params.version)
        .first<ReleaseRow>();
      if (!r) throw new ApiError("not_found", "no such release");
      return serveRelease(request, env, r, platform);
    },
  });

  // Public: anyone with an invite code that has not expired (used or not). Invite links carry
  // the code in the URL fragment (`/download#code=...`), which browsers never send, so no request
  // log (ours or the Workers invocation log) sees it: GET serves a small page that POSTs the code.
  // Workers invocation logs (which store the full URL) are also off in server/wrangler.jsonc.
  registerRoute({
    method: "GET",
    path: "/download",
    auth: "none",
    async handler({ env, request, url }) {
      // Links made before the fragment form keep working.
      if (url.searchParams.has("code")) {
        return downloadFor(env, request, url.searchParams.get("code") ?? "", url.searchParams.get("platform"));
      }
      return new Response(DOWNLOAD_PAGE, {
        headers: {
          "content-type": "text/html; charset=utf-8",
          "cache-control": "no-store",
          "referrer-policy": "no-referrer",
          "x-content-type-options": "nosniff",
          "content-security-policy": `default-src 'none'; script-src '${await downloadScriptHash()}'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'`,
        },
      });
    },
  });

  registerRoute({
    method: "POST",
    path: "/download",
    auth: "none",
    async handler({ env, request }) {
      const text = new TextDecoder().decode(await readBytes(request, 1024));
      const form = new URLSearchParams(text);
      return downloadFor(env, request, form.get("code") ?? "", form.get("platform"));
    },
  });
}

/** Stores the file of `version` for `platform` (admin secret) and answers with its `UpdateInfo`. */
async function publishRelease(env: Env, request: Request, url: URL, version: string, platform: ReleasePlatform): Promise<UpdateInfo> {
  const given = request.headers.get("x-admin-secret") ?? "";
  const expected = env.ADMIN_SECRET ?? "";
  const ok = await timingSafeEqualString(given, expected);
  if (!expected || !ok) throw new ApiError("unauthorized", "invalid admin secret");
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
  const p = PLATFORMS[platform];
  const file = p.file(version);
  const now = Date.now();
  // `releases` has no platform column: its rows are the DMGs.
  const [platformColumn, platformValue] = platform === "macos" ? ["", ""] : ["platform, ", `'${platform}', `];
  const conflict = platform === "macos" ? "version" : "version, platform";

  // Claim the version first, so two uploads of one version never overwrite each other.
  const claim = await env.DB.prepare(
    `INSERT INTO ${p.table} (${platformColumn}version, major, minor, patch, file_name, size, sha256, notes, status, created_at)
     VALUES (${platformValue}?, ?, ?, ?, ?, ?, ?, ?, 'uploading', ?)
     ON CONFLICT (${conflict}) DO UPDATE SET size = excluded.size, sha256 = excluded.sha256,
       notes = excluded.notes, created_at = excluded.created_at
     WHERE ${p.table}.status = 'uploading' AND ${p.table}.created_at < ?`,
  )
    .bind(version, major, minor, patch, file, size, sha256, notes, now, now - STALE_UPLOAD_MS)
    .run();
  if (claim.meta.changes !== 1) throw new ApiError("conflict", "release exists");

  try {
    await putVerified(env.FILES, releaseKey(version, file), request.body, size, sha256, { contentType: p.mime });
  } catch (err) {
    await env.DB.prepare(`DELETE FROM ${p.table} WHERE ${p.where}version = ? AND status = 'uploading' AND created_at = ?`)
      .bind(version, now)
      .run();
    throw err;
  }
  const published = Date.now();
  // The DMG's audit entry keeps its shape; another platform names itself.
  const details = platform === "macos" ? { size, sha256 } : { size, sha256, platform };
  await env.DB.batch([
    env.DB.prepare(
      `UPDATE ${p.table} SET status = 'ready', published_at = ? WHERE ${p.where}version = ? AND created_at = ?`,
    ).bind(published, version, now),
    auditStatement(env.DB, null, { action: "release.publish", target: version, details }, published),
  ]);
  const row = await env.DB.prepare(`SELECT * FROM ${p.table} WHERE ${p.where}version = ?`).bind(version).first<ReleaseRow>();
  return toUpdateInfo(row as ReleaseRow, url.origin, platform);
}

/** Rate limited per IP; the code is never logged (this function logs nothing). */
async function downloadFor(env: Env, request: Request, raw: string, rawPlatform: string | null = null): Promise<Response> {
  // An unknown platform gets the DMG, as every link did before platforms.
  const platform = ReleasePlatform.safeParse(rawPlatform).data ?? "macos";
  const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
  const key = `download:${await sha256Hex(ip)}`;
  if (!(await rateHit(env.DB, key, DOWNLOAD_RATE.windowMs, DOWNLOAD_RATE.max))) {
    return plain(429, "Too many requests. Try again later.\n");
  }
  const code = normalizeInviteCode(raw.slice(0, 64));
  if (code.length !== INVITE_CODE_LENGTH && code.length !== LEGACY_INVITE_CODE_LENGTH) {
    return plain(404, "Invite code not found or expired.\n");
  }
  const invite = await env.DB.prepare("SELECT id FROM invites WHERE code_hash = ? AND expires_at > ?")
    .bind(await sha256Hex(code), Date.now())
    .first<{ id: string }>();
  if (!invite) return plain(404, "Invite code not found or expired.\n");
  const r = await latestRelease(env.DB, platform);
  if (!r) return plain(404, platform === "macos" ? "No release yet.\n" : "No release for this system yet.\n");
  return serveRelease(request, env, r, platform);
}

/**
 * Reads the code from the fragment (or an old query link), drops it from the address bar, posts
 * it. A browser on Windows also posts `platform=windows` and so gets the installer, not the DMG.
 */
const DOWNLOAD_SCRIPT = `(function(){var h=new URLSearchParams(location.hash.slice(1)),q=new URLSearchParams(location.search),c=h.get("code")||q.get("code")||"";history.replaceState(null,"",location.pathname);if(!c){document.getElementById("m").textContent="Invite code not found or expired.";return}var f=document.createElement("form");f.method="POST";f.action="/download";var i=document.createElement("input");i.type="hidden";i.name="code";i.value=c;f.appendChild(i);if(/Windows/.test(navigator.userAgent)){var p=document.createElement("input");p.type="hidden";p.name="platform";p.value="windows";f.appendChild(p)}document.body.appendChild(f);f.submit()})();`;

const DOWNLOAD_PAGE =
  '<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Download</title>' +
  `<p id="m">Downloading…</p><script>${DOWNLOAD_SCRIPT}</script>`;

let scriptHash: Promise<string> | null = null;
/** CSP source of the page script (only this script may run). */
function downloadScriptHash(): Promise<string> {
  scriptHash ??= crypto.subtle
    .digest("SHA-256", new TextEncoder().encode(DOWNLOAD_SCRIPT))
    .then((d) => `sha256-${btoa(String.fromCharCode(...new Uint8Array(d)))}`);
  return scriptHash;
}
