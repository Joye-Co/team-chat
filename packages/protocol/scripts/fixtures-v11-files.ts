// 1.1 fixtures of the server-files gaps (GAPS.md SF-*): media file limits, multipart uploads,
// stored page events, desk notes, aside origins, PR merge state, group purpose and 产出
// previews. Called by gen-fixtures.ts.
import type { InteractCtx } from "./fixtures-v11-interact.ts";

export function addFilesFixtures({ ok, bad, T0, ID, base, message }: InteractCtx) {
  const FILE = "01JC0FIL0000000000000INDEX";
  void ok;
  void T0;
  void base;
  void message;

  // SF-7: a table is at most 200 MB.
  const table = { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, preview: "ready" };
  const createCsv = { groupId: ID.general, topicId: ID.topic, title: "报名表", kind: "file", entry: "a.csv", files: [{ path: "a.csv", fileId: FILE }], media: table };
  ok("http/create-artifact-media-table.json", "CreateArtifactRequest", createCsv);
  bad("http/create-artifact-media-table-too-big.json", "CreateArtifactRequest", { ...createCsv, media: { ...table, bytes: 200_000_001 } }, "a table is at most 200 MB");
  ok("entities/media-table.json", "MediaInfo", { kind: "table", mime: "text/csv", bytes: 120_000, rows: 124, cols: 6, posterFileId: FILE, preview: "ready" });
}
