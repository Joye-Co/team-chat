// Tokenizing for SQLite FTS5 with the unicode61 tokenizer.
//
// FTS5 does not split Chinese, and the trigram tokenizer needs 3 characters. We split in our
// code and store space-separated tokens, which unicode61 (with tokenchars '_') keeps as is.
//
// Index layout: a "main stream" of tokens in text order, then "extra" tokens at the end.
// - Latin, numbers, code identifiers: one lowercased token per word in the main stream.
//   camelCase and snake_case parts go to the extra tokens, so phrases over whole words
//   (for example "foo.bar" or "3.14") stay adjacent in the main stream.
// - CJK runs: overlapping bigrams in the main stream; a run of one character is a unigram.
//   The last character of each longer run goes to the extra tokens as a unigram, so a
//   prefix query "c"* finds every occurrence of a single character c.
// - Punctuation, symbols, emoji and whitespace are dropped.

/** Bump when the index format changes, so callers can rebuild their index. */
export const TOKENIZER_VERSION = 1

const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}々〆ー]/u
const WORD = /[\p{L}\p{M}\p{N}_]/u
const HAS_ALNUM = /[\p{L}\p{N}]/u
const CAMEL_PART = /\p{Lu}+\p{N}*(?!\p{Ll})|\p{Lu}?[\p{Ll}\p{M}]+\p{N}*|\p{N}+|[\p{Lo}\p{Lt}\p{Lm}\p{M}]+/gu

/** A piece of text: a run of word characters, or a run of CJK characters. */
export type Piece =
  | { kind: "word"; text: string; start: number; end: number; group: number }
  | { kind: "cjk"; text: string; start: number; end: number; group: number }

/**
 * Splits text into word and CJK pieces. Word pieces that touch through punctuation only
 * (no whitespace, no CJK, no other piece) share a `group` number, so "foo.bar" is one group.
 * Offsets refer to the normalized string returned in `normalized`.
 */
export function splitPieces(input: string): { normalized: string; pieces: Piece[] } {
  const text = input.normalize("NFKC")
  const pieces: Piece[] = []
  const chars = Array.from(text)
  let group = 0
  let lastWordGroupOpen = false
  let pos = 0
  let i = 0
  while (i < chars.length) {
    const ch = chars[i] as string
    if (CJK.test(ch)) {
      const start = pos
      let run = ""
      while (i < chars.length && CJK.test(chars[i] as string)) {
        run += chars[i]
        pos += (chars[i] as string).length
        i++
      }
      pieces.push({ kind: "cjk", text: run, start, end: pos, group: ++group })
      lastWordGroupOpen = false
      continue
    }
    if (WORD.test(ch)) {
      const start = pos
      let word = ""
      while (i < chars.length) {
        const c = chars[i] as string
        if (!WORD.test(c) || CJK.test(c)) break
        word += c
        pos += c.length
        i++
      }
      if (HAS_ALNUM.test(word)) {
        if (!lastWordGroupOpen) group++
        pieces.push({ kind: "word", text: word, start, end: pos, group })
        lastWordGroupOpen = true
      }
      continue
    }
    // Separator. Whitespace closes the current word group; other punctuation keeps it open.
    if (/\s/u.test(ch)) lastWordGroupOpen = false
    pos += ch.length
    i++
  }
  return { normalized: text, pieces }
}

/** camelCase and snake_case parts of an identifier, lowercased, without the whole word. */
export function identifierParts(word: string): string[] {
  const whole = word.toLowerCase()
  const parts = new Set<string>()
  for (const segment of word.split("_")) {
    if (!segment) continue
    const sub = segment.match(CAMEL_PART) ?? [segment]
    if (segment.toLowerCase() !== whole) parts.add(segment.toLowerCase())
    if (sub.length > 1) for (const p of sub) parts.add(p.toLowerCase())
  }
  parts.delete(whole)
  return [...parts].filter((p) => HAS_ALNUM.test(p))
}

function bigrams(run: string): string[] {
  const chars = Array.from(run)
  if (chars.length === 1) return [run]
  const out: string[] = []
  for (let i = 0; i + 1 < chars.length; i++) out.push((chars[i] as string) + (chars[i + 1] as string))
  return out
}

/**
 * Tokens for the FTS5 `tokens` column: a space-separated string. Empty when the text has
 * nothing searchable (for example only emoji); do not index such a message.
 */
export function tokenizeForIndex(text: string): string {
  const { pieces } = splitPieces(text)
  const main: string[] = []
  const extra: string[] = []
  for (const piece of pieces) {
    if (piece.kind === "word") {
      main.push(piece.text.toLowerCase())
      extra.push(...identifierParts(piece.text))
    } else {
      main.push(...bigrams(piece.text))
      const chars = Array.from(piece.text)
      if (chars.length > 1) extra.push(chars[chars.length - 1] as string)
    }
  }
  return main.concat(extra).join(" ")
}

export interface MatchQueryOptions {
  /** Match the last Latin word as a prefix (search as you type). Default false. */
  prefixLast?: boolean
  /** At most this many terms; the rest of the query is ignored. Default 32. */
  maxTerms?: number
}

/** One FTS5 string literal. Tokens never contain quotes, but escape anyway. */
function quote(token: string): string {
  return `"${token.replaceAll('"', '""')}"`
}

/**
 * An FTS5 MATCH expression for what the user typed, or null when nothing is searchable
 * (then do not run the query). Every token is a quoted string, so user input cannot use
 * FTS5 syntax (AND, OR, NOT, NEAR, column filters, parentheses, ^, *).
 *
 * - A CJK run of 2+ characters becomes a phrase of its bigrams: 部署上线 → "部署 署上 上线".
 * - A single CJK character becomes a prefix query "部"*, which finds it anywhere.
 * - Words joined by punctuation become a phrase: foo.bar → "foo bar".
 * - Terms are combined with AND.
 */
export function buildMatchQuery(userQuery: string, options: MatchQueryOptions = {}): string | null {
  const maxTerms = options.maxTerms ?? 32
  const { pieces } = splitPieces(userQuery)
  const terms: string[] = []
  let lastWordTerm = -1
  for (let i = 0; i < pieces.length && terms.length < maxTerms; i++) {
    const piece = pieces[i] as Piece
    if (piece.kind === "cjk") {
      const grams = bigrams(piece.text)
      if (Array.from(piece.text).length === 1) terms.push(`${quote(piece.text)}*`)
      else terms.push(quote(grams.join(" ")))
      lastWordTerm = -1
      continue
    }
    const words = [piece.text.toLowerCase()]
    while (i + 1 < pieces.length) {
      const next = pieces[i + 1] as Piece
      if (next.kind !== "word" || next.group !== piece.group) break
      words.push(next.text.toLowerCase())
      i++
    }
    terms.push(quote(words.join(" ")))
    lastWordTerm = terms.length - 1
  }
  if (terms.length === 0) return null
  if (options.prefixLast && lastWordTerm === terms.length - 1) terms[lastWordTerm] += "*"
  return terms.join(" AND ")
}

/** The strings to look for in the original text when building a snippet. */
export function highlightTerms(userQuery: string): string[] {
  const { pieces } = splitPieces(userQuery)
  const out = new Set<string>()
  for (const piece of pieces) out.add(piece.text.toLowerCase())
  return [...out].sort((a, b) => b.length - a.length)
}
