// V1 diagnostics upload. See docs/protocol.md, "Diagnostics".
import { z } from "zod";
import { Id } from "./common.ts";

/** Max gzip body of `POST /v1/diagnostics`. */
export const MAX_DIAGNOSTICS_BYTES = 5_242_880;

export const DiagnosticsResponse = z.object({ id: Id });
export type DiagnosticsResponse = z.infer<typeof DiagnosticsResponse>;
