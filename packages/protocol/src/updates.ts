// V1 app updates and DMG downloads. See docs/protocol.md, "Updates and downloads".
import { z } from "zod";
import { Count, Millis, Sha256 } from "./common.ts";

/** Release versions are `MAJOR.MINOR.PATCH` without leading zeros. */
export const ReleaseVersion = z.string().regex(/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/);

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
});
export type UpdateInfo = z.infer<typeof UpdateInfo>;
