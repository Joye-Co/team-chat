// V1 app updates and downloads of the DMG and the Windows installer. See docs/protocol.md, "Updates and downloads".
import { z } from "zod";
import { Count, Millis, Sha256 } from "./common.ts";

/** Release versions are `MAJOR.MINOR.PATCH` without leading zeros. */
export const ReleaseVersion = z.string().regex(/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/);

/**
 * What a release file runs on. `macos` is the DMG; `windows` is the installer (`-setup.exe`,
 * x64). A request or an `UpdateInfo` without a platform means `macos`.
 */
export const ReleasePlatform = z.enum(["macos", "windows"]);
export type ReleasePlatform = z.infer<typeof ReleasePlatform>;

/** `GET /v1/updates/latest` and the response of `PUT /v1/admin/releases/:version`. */
export const UpdateInfo = z.object({
  version: ReleaseVersion,
  /** Release notes, Markdown. */
  notes: z.string().max(20_000),
  size: Count,
  sha256: Sha256,
  publishedAt: Millis,
  /** Absolute URL of `GET /v1/updates/:version/download`. */
  downloadUrl: z.string().min(1),
  /** Present on every answer of a server that knows platforms; missing = `macos`. */
  platform: ReleasePlatform.optional(),
});
export type UpdateInfo = z.infer<typeof UpdateInfo>;
