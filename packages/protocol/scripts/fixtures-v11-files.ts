// 1.1 fixtures of the server-files gaps (GAPS.md SF-*): media file limits, multipart uploads,
// stored page events, desk notes, aside origins, PR merge state, group purpose and 产出
// previews. Called by gen-fixtures.ts.
import type { InteractCtx } from "./fixtures-v11-interact.ts";

export function addFilesFixtures({ ok, bad, T0, ID, base, message }: InteractCtx) {
  const FILE = "01JC0FIL0000000000000INDEX";
  void ok;
  void base;
  void message;

  // SF-1: files over 100 MB upload in parts.
  const SHA = "b".repeat(64);
  const BIG = "01JC0FIL00000000000000BIG1";
  const bigMeta = { id: BIG, groupId: ID.general, topicId: null, name: "录屏.mov", size: 150_000_000, mime: "video/quicktime", sha256: SHA, status: "pending", createdBy: ID.alice, createdAt: T0 };
  ok("http/create-file-request-multipart.json", "CreateFileRequest", { groupId: ID.general, name: "录屏.mov", size: 150_000_000, mime: "video/quicktime", sha256: SHA }, "1.1: over 100 MB, up to 2 GB");
  ok("http/create-file-response-multipart.json", "CreateFileResponse", {
    file: bigMeta,
    uploadUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/content`,
    multipart: { partBytes: 52_428_800, parts: 3, partUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/parts`, completeUrl: `https://chat-api-e2e.example.workers.dev/v1/files/${BIG}/complete` },
  });
  ok("http/upload-part-response.json", "UploadPartResponse", { part: 2, size: 52_428_800 });

  // SF-2: the page log of a 产出.
  const ART = "01JC0ART00000000000000OCT3";
  ok("http/artifact-events-response.json", "ArtifactEventsResponse", {
    events: [
      { eventId: "01JC0EV0000000000000000002", artifactId: ART, version: 2, name: "pick", kind: "emit", payload: { channel: "bili" }, actorId: ID.owner, at: T0 + 60_000 },
      { eventId: "01JC0EV0000000000000000001", artifactId: ART, version: 1, name: "ask.total", kind: "ask", payload: { month: "2026-10" }, actorId: ID.alice, at: T0, replied: true },
      { eventId: "01JC0EV0000000000000000000", artifactId: ART, version: 1, name: "pick", kind: "emit", actorId: ID.alice, at: T0 - 1 },
    ],
    hasMore: true,
  }, "newest first; the last one is from before payloads were kept");
  ok("http/artifact-events-response-empty.json", "ArtifactEventsResponse", { events: [], hasMore: false });

  // SF-7: a table is at most 200 MB.
  const table = { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, preview: "ready" };
  const createCsv = { groupId: ID.general, topicId: ID.topic, title: "报名表", kind: "file", entry: "a.csv", files: [{ path: "a.csv", fileId: FILE }], media: table };
  ok("http/create-artifact-media-table.json", "CreateArtifactRequest", createCsv);
  bad("http/create-artifact-media-table-too-big.json", "CreateArtifactRequest", { ...createCsv, media: { ...table, bytes: 200_000_001 } }, "a table is at most 200 MB");
  ok("entities/media-table.json", "MediaInfo", { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, posterFileId: FILE, preview: "ready" });
}
