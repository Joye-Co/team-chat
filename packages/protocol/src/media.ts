// 1.1 media metadata of 产出 versions and message files (docs/protocol.md, "Media"). The
// sender's client computes it; the server checks sizes and limits and never decodes media.
import { z } from "zod";
import { Count, eventBase, Id, Mime } from "./common.ts";

export const MediaKind = z.enum(["html", "markdown", "image", "imageSet", "video", "audio", "pdf", "code", "table", "file"]);
export type MediaKind = z.infer<typeof MediaKind>;
export const MediaPreview = z.enum(["ready", "pending", "failed"]);
export type MediaPreview = z.infer<typeof MediaPreview>;

export const MAX_MEDIA_FRAMES = 12;
export const MAX_MEDIA_CHAPTERS = 20;
export const MAX_MEDIA_IMAGES = 30;
/** Size limits (bytes, seconds, pixels, pages), checked by `mediaProblem`. */
export const MEDIA_LIMITS = {
  anyBytes: 2_000_000_000,
  durationSeconds: 4 * 3600,
  imageBytes: 50_000_000,
  imagePixels: 100_000_000,
  pdfBytes: 500_000_000,
  pdfPages: 2000,
  htmlBytes: 20_000_000,
  textBytes: 5_000_000,
  /** CSV / table (kind `table`). */
  tableBytes: 200_000_000,
  /** Poster (video poster, HTML snapshot, PDF page 1, image thumbnail): a JPEG of at most 1280 px. */
  posterBytes: 200_000,
  posterPixels: 1280,
  /** One video frame: a JPEG of at most 320 px. */
  frameBytes: 200_000,
  framePixels: 320,
  /** Transcript JSON. */
  transcriptBytes: 2_000_000,
} as const;

const Px = z.int().positive();
const Seconds = z.number().nonnegative();

/** What a 产出 version or a message file is, for inline previews. */
export const MediaInfo = z.object({
  kind: MediaKind,
  /** Sniffed by the sender, checked again by the receiver. */
  mime: Mime,
  bytes: Count,
  width: Px.optional(),
  height: Px.optional(),
  /** Seconds (video, audio). */
  duration: Seconds.optional(),
  pages: Count.optional(),
  rows: Count.optional(),
  cols: Count.optional(),
  lines: Count.optional(),
  language: z.string().min(1).max(32).optional(),
  /** Video poster, HTML snapshot, PDF page 1 or image thumbnail. */
  posterFileId: Id.optional(),
  frames: z
    .array(z.object({ t: Seconds, fileId: Id, label: z.string().min(1).max(80).optional() }))
    .max(MAX_MEDIA_FRAMES)
    .optional(),
  chapters: z.array(z.object({ t: Seconds, title: z.string().min(1).max(120) })).max(MAX_MEDIA_CHAPTERS).optional(),
  /** Audio waveform peaks (Float16 array). */
  peaksFileId: Id.optional(),
  /** JSON `[{t, end, speaker, text}]`. */
  transcriptFileId: Id.optional(),
  images: z.array(z.object({ fileId: Id, width: Px, height: Px, thumbFileId: Id })).max(MAX_MEDIA_IMAGES).optional(),
  /** Code: the lines shown inline. */
  focus: z.object({ fromLine: z.int().positive(), toLine: z.int().positive() }).optional(),
  preview: MediaPreview,
});
export type MediaInfo = z.infer<typeof MediaInfo>;

/** The first size limit `m` breaks, or undefined. */
export function mediaProblem(m: MediaInfo): string | undefined {
  const L = MEDIA_LIMITS;
  if (m.bytes > L.anyBytes) return "file exceeds 2 GB";
  if (m.duration !== undefined && m.duration > L.durationSeconds) return "longer than 4 hours";
  if (m.kind === "image") {
    if (m.bytes > L.imageBytes) return "image exceeds 50 MB";
    if (m.width !== undefined && m.height !== undefined && m.width * m.height > L.imagePixels) return "image exceeds 100 MP";
  }
  if (m.kind === "pdf" && (m.bytes > L.pdfBytes || (m.pages ?? 0) > L.pdfPages)) return "pdf exceeds 500 MB or 2000 pages";
  if (m.kind === "html" && m.bytes > L.htmlBytes) return "html exceeds 20 MB";
  if ((m.kind === "markdown" || m.kind === "code") && m.bytes > L.textBytes) return "text exceeds 5 MB";
  if (m.kind === "table" && m.bytes > L.tableBytes) return "table exceeds 200 MB";
  if (m.focus && m.focus.fromLine > m.focus.toLine) return "focus.fromLine is after toLine";
  return undefined;
}

/** What a writer knows of a file that media names: its size and, for images, its pixels. */
export type MediaFileFacts = { size: number; width?: number | null; height?: number | null };

/**
 * The first limit a file that `m` names breaks (poster and frames 200 KB and 1280 / 320 px,
 * transcript 2 MB), or undefined. `facts` returns undefined for an unknown file; the caller
 * checks that every named file exists.
 */
export function mediaFileProblem(m: MediaInfo, facts: (fileId: string) => MediaFileFacts | undefined): string | undefined {
  const L = MEDIA_LIMITS;
  const big = (f: MediaFileFacts | undefined, px: number) => f !== undefined && Math.max(f.width ?? 0, f.height ?? 0) > px;
  if (m.posterFileId) {
    const f = facts(m.posterFileId);
    if (f && f.size > L.posterBytes) return "poster exceeds 200 KB";
    if (big(f, L.posterPixels)) return "poster exceeds 1280 px";
  }
  for (const fr of m.frames ?? []) {
    const f = facts(fr.fileId);
    if (f && f.size > L.frameBytes) return "frame exceeds 200 KB";
    if (big(f, L.framePixels)) return "frame exceeds 320 px";
  }
  if (m.transcriptFileId) {
    const f = facts(m.transcriptFileId);
    if (f && f.size > L.transcriptBytes) return "transcript exceeds 2 MB";
  }
  return undefined;
}

/** `MediaInfo` on a write: shapes and `mediaProblem`. */
export const MediaInfoInput = MediaInfo.superRefine((m, ctx) => {
  const p = mediaProblem(m);
  if (p) ctx.addIssue({ code: "custom", message: p });
});

/** Stored event (1.1): a sender attached the preview of a 产出 version after the upload. */
export const ArtifactPreviewReadyEvent = z.object({
  ...eventBase,
  type: z.literal("artifact.previewReady"),
  data: z.object({ artifactId: Id, version: z.int().positive(), media: MediaInfo }),
});
