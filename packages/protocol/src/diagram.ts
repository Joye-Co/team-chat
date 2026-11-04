// Static checks of a Mermaid diagram source (docs/protocol.md, "Diagrams"). The server runs
// them on a Card v2 `diagram` block; the SDK and the desktop run the same rules. No
// rendering here. Mirrored in crates/chat-protocol/src/diagram.rs; both read
// protocol/checks/diagram-checks.json in their tests.

/** Max UTF-8 bytes of a diagram source. */
export const MAX_DIAGRAM_SOURCE_BYTES = 16_384;

/** First keywords of the diagram types 1.1.0 draws, and the type each one names. */
export const DIAGRAM_KEYWORDS: Readonly<Record<string, string>> = {
  flowchart: "flowchart",
  graph: "flowchart",
  sequenceDiagram: "sequenceDiagram",
  stateDiagram: "stateDiagram",
  "stateDiagram-v2": "stateDiagram",
  classDiagram: "classDiagram",
  erDiagram: "erDiagram",
  gantt: "gantt",
  timeline: "timeline",
};

/** Text that a source must not have (compared without case): directives, links, script and HTML. */
export const DIAGRAM_FORBIDDEN = ["%%{", "click ", "href", "callback", "call ", "javascript:", "<script", "<iframe", "<foreignobject"] as const;

export type DiagramCheck = { ok: true; type: string } | { ok: false; reason: string };

/**
 * Checks a diagram source: size, no front matter, none of `DIAGRAM_FORBIDDEN`, and a first
 * non-comment line that starts with a keyword of `DIAGRAM_KEYWORDS`. Reasons: `empty`,
 * `too_large`, `front_matter`, `forbidden:<text>`, `unsupported:<keyword>`.
 */
export function checkDiagramSource(source: string): DiagramCheck {
  if (new TextEncoder().encode(source).byteLength > MAX_DIAGRAM_SOURCE_BYTES) return { ok: false, reason: "too_large" };
  const lines = source.split(/\r?\n/);
  const first = lines.findIndex((l) => l.trim() !== "");
  if (first < 0) return { ok: false, reason: "empty" };
  if (lines[first]?.trim() === "---") return { ok: false, reason: "front_matter" };
  const lower = source.toLowerCase();
  for (const f of DIAGRAM_FORBIDDEN) if (lower.includes(f)) return { ok: false, reason: `forbidden:${f.trim()}` };
  const head = lines.find((l) => l.trim() !== "" && !l.trim().startsWith("%%"));
  if (head === undefined) return { ok: false, reason: "empty" };
  const keyword = head.trim().split(/[\s:;]/)[0] ?? "";
  const type = Object.hasOwn(DIAGRAM_KEYWORDS, keyword) ? DIAGRAM_KEYWORDS[keyword] : undefined;
  return type ? { ok: true, type } : { ok: false, reason: `unsupported:${keyword}` };
}
