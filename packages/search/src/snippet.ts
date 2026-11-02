// A display snippet from the original message text. FTS5 snippet() cannot help, because the
// FTS column holds tokens, not the text.

import { highlightTerms } from "./tokenize.ts"

export interface Snippet {
  /** The excerpt, with "…" where text was cut. Whitespace runs become single spaces. */
  text: string
  /** Matched ranges in `text` as [start, end) UTF-16 offsets, sorted, not overlapping. */
  highlights: [number, number][]
}

export interface SnippetOptions {
  /** Excerpt length in UTF-16 units, without the ellipses. Default 120. */
  maxLength?: number
  /** Characters to keep before the first match. Default 30. */
  before?: number
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

/** Moves an offset off the low half of a surrogate pair. */
function safeOffset(text: string, i: number): number {
  if (i > 0 && i < text.length) {
    const code = text.charCodeAt(i)
    if (code >= 0xdc00 && code <= 0xdfff) return i - 1
  }
  return i
}

/**
 * The part of `text` that matches `userQuery`, with highlight ranges. When nothing matches
 * literally (for example after Unicode normalization), it returns the start of the text.
 */
export function makeSnippet(text: string, userQuery: string, options: SnippetOptions = {}): Snippet {
  const maxLength = Math.max(10, options.maxLength ?? 120)
  const before = Math.max(0, Math.min(options.before ?? 30, maxLength - 10))
  const flat = text.normalize("NFKC").replace(/\s+/gu, " ").trim()

  const matches: [number, number][] = []
  for (const term of highlightTerms(userQuery)) {
    const re = new RegExp(escapeRegExp(term), "giu")
    for (const m of flat.matchAll(re)) matches.push([m.index, m.index + m[0].length])
  }
  matches.sort((a, b) => a[0] - b[0] || b[1] - a[1])

  const first = matches[0]
  let start = first ? Math.max(0, first[0] - before) : 0
  let end = Math.min(flat.length, start + maxLength)
  if (end - start < maxLength) start = Math.max(0, end - maxLength)
  start = safeOffset(flat, start)
  end = safeOffset(flat, end)

  const lead = start > 0 ? "…" : ""
  const tail = end < flat.length ? "…" : ""
  const highlights: [number, number][] = []
  for (const [s, e] of matches) {
    if (e <= start || s >= end) continue
    const a = Math.max(s, start) - start + lead.length
    const b = Math.min(e, end) - start + lead.length
    const last = highlights[highlights.length - 1]
    if (last && a <= last[1]) last[1] = Math.max(last[1], b)
    else highlights.push([a, b])
  }
  return { text: lead + flat.slice(start, end) + tail, highlights }
}
