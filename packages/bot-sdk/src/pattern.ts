// The server's TcSchema `pattern` rule (server/src/files/pattern.ts), so that a bot learns of a
// pattern the server refuses before it publishes a version. The server checks a pattern in linear
// time with a Thompson NFA, so it refuses a backreference (`\1`, `\k<…>`), lookaround (`(?=`,
// `(?!`, `(?<=`, `(?<!`) and a pattern of more than 1000 states after `{n,m}` is expanded. A string
// checked by a pattern has at most MAX_PATTERN_INPUT code points (a longer one does not match).
// Keep this file in step with the server's parser.

import { type ArtifactEventDecl, ARTIFACT_EVENT_NAME_RE, tcSchemaProblem } from "@chat/protocol";
import { ChatError } from "./errors.ts";

/** NFA states at most, after `{n,m}` is expanded. */
const MAX_STATES = 1000;
/** A string checked by a pattern has at most this many code points (else it does not match). */
export const MAX_PATTERN_INPUT = 4096;

type Node =
  | { k: "char" }
  | { k: "seq"; items: Node[] }
  | { k: "alt"; alts: Node[] }
  | { k: "rep"; child: Node; min: number; max: number }
  | { k: "assert" };

class UnsupportedPattern extends Error {}

const CHAR: Node = { k: "char" };
const ASSERT: Node = { k: "assert" };

class Parser {
  i = 0;
  readonly p: string;
  constructor(p: string) {
    this.p = p;
  }

  peek(n = 0): string {
    return this.p[this.i + n] ?? "";
  }

  disjunction(): Node {
    const alts = [this.alternative()];
    while (this.peek() === "|") {
      this.i++;
      alts.push(this.alternative());
    }
    return alts.length === 1 ? (alts[0] as Node) : { k: "alt", alts };
  }

  alternative(): Node {
    const items: Node[] = [];
    while (this.i < this.p.length && this.peek() !== "|" && this.peek() !== ")") items.push(this.quantified(this.term()));
    return { k: "seq", items };
  }

  term(): Node {
    const c = this.peek();
    if (c === "^" || c === "$") {
      this.i++;
      return ASSERT;
    }
    if (c === "(") return this.group();
    if (c === "[") return this.charClass();
    if (c === "\\") return this.escape();
    this.i += String.fromCodePoint(this.p.codePointAt(this.i) as number).length;
    return CHAR;
  }

  group(): Node {
    this.i++;
    if (this.peek() === "?") {
      const n = this.peek(1);
      if (n === ":") this.i += 2;
      else if (n === "<" && this.peek(2) !== "=" && this.peek(2) !== "!") this.i = this.p.indexOf(">", this.i) + 1;
      else throw new UnsupportedPattern("lookaround is not supported");
    }
    const inner = this.disjunction();
    if (this.peek() !== ")") throw new SyntaxError("unclosed group");
    this.i++;
    return inner;
  }

  charClass(): Node {
    this.i++;
    if (this.peek() === "^") this.i++;
    while (this.i < this.p.length && this.peek() !== "]") this.i += this.peek() === "\\" ? 2 : 1;
    if (this.peek() !== "]") throw new SyntaxError("unclosed class");
    this.i++;
    return CHAR;
  }

  escape(): Node {
    const from = this.i;
    const c = this.peek(1);
    if (/[1-9]/.test(c) || c === "k") throw new UnsupportedPattern("backreferences are not supported");
    this.i += 2;
    if (c === "b" || c === "B") return ASSERT;
    if ((c === "p" || c === "P") && this.peek() === "{") this.i = this.p.indexOf("}", this.i) + 1;
    else if (c === "u" && this.peek() === "{") this.i = this.p.indexOf("}", this.i) + 1;
    else if (c === "u") this.i += 4;
    else if (c === "x") this.i += 2;
    else if (c === "c") this.i += 1;
    else if (c === "") throw new SyntaxError("trailing backslash");
    else this.i = from + 1 + String.fromCodePoint(this.p.codePointAt(from + 1) as number).length;
    return CHAR;
  }

  quantified(atom: Node): Node {
    const c = this.peek();
    let min: number;
    let max: number;
    if (c === "*") [min, max] = [0, Number.POSITIVE_INFINITY];
    else if (c === "+") [min, max] = [1, Number.POSITIVE_INFINITY];
    else if (c === "?") [min, max] = [0, 1];
    else if (c === "{") {
      const m = /^\{(\d+)(,(\d*))?\}/.exec(this.p.slice(this.i));
      if (!m) return atom;
      min = Number(m[1]);
      max = m[2] === undefined ? min : m[3] === "" ? Number.POSITIVE_INFINITY : Number(m[3]);
      this.i += m[0].length - 1;
    } else return atom;
    this.i++;
    if (this.peek() === "?") this.i++;
    if (min > MAX_STATES || (max !== Number.POSITIVE_INFINITY && max > MAX_STATES)) throw new UnsupportedPattern("the pattern is too large");
    return { k: "rep", child: atom, min, max };
  }
}

/** Counts the NFA states the server builds for a node, as server/src/files/pattern.ts `build`. */
function countStates(root: Node): void {
  let states = 1; // match
  const push = () => {
    if (states >= MAX_STATES) throw new UnsupportedPattern("the pattern is too large");
    states++;
  };
  const compile = (n: Node): void => {
    switch (n.k) {
      case "char":
      case "assert":
        return push();
      case "seq":
        for (const item of n.items) compile(item);
        return;
      case "alt":
        for (const a of n.alts) compile(a);
        for (let i = 0; i < n.alts.length - 1; i++) push();
        return;
      case "rep":
        if (n.max === Number.POSITIVE_INFINITY) {
          push();
          compile(n.child);
        } else {
          for (let i = 0; i < n.max - n.min; i++) {
            compile(n.child);
            push();
          }
        }
        for (let i = 0; i < n.min; i++) compile(n.child);
    }
  };
  compile(root);
}

/** Why the server cannot check `pattern` (the server's message), or undefined when it can. */
export function patternProblem(pattern: string): string | undefined {
  try {
    // The RegExp parser checks the syntax; it is never run.
    new RegExp(pattern, "u");
    const p = new Parser(pattern);
    const root = p.disjunction();
    if (p.i !== pattern.length) throw new SyntaxError("unbalanced parenthesis");
    countStates(root);
    return undefined;
  } catch (err) {
    if (err instanceof UnsupportedPattern) return `pattern: ${err.message}`;
    return "pattern is not a valid regular expression";
  }
}

type SchemaPatterns = { pattern?: string; properties?: Record<string, SchemaPatterns>; items?: SchemaPatterns };

/** The first `pattern` in a TcSchema that the server cannot check, or undefined. */
export function schemaPatternProblem(schema: unknown): string | undefined {
  if (!schema || typeof schema !== "object") return undefined;
  const s = schema as SchemaPatterns;
  if (typeof s.pattern === "string") {
    const p = patternProblem(s.pattern);
    if (p) return p;
  }
  for (const sub of [...Object.values(s.properties ?? {}), s.items]) {
    const p = schemaPatternProblem(sub);
    if (p) return p;
  }
  return undefined;
}

/**
 * Checks the events a 产出 version declares as the server does when the version is made: the
 * name, the TcSchema subset and each `pattern`. Throws `ChatError` `invalid` before anything is
 * sent; for a `pattern` the server cannot check, with reason `artifact.bad_schema` as the server.
 */
export function checkArtifactEvents(events: readonly ArtifactEventDecl[] | undefined): void {
  const names = new Set<string>();
  for (const e of events ?? []) {
    if (!ARTIFACT_EVENT_NAME_RE.test(e.name)) throw new ChatError("invalid", `event ${e.name}: names are 1..64 of [a-z][A-Za-z0-9_.-]`);
    if (names.has(e.name)) throw new ChatError("invalid", `event ${e.name}: event names are unique`);
    names.add(e.name);
    if (e.schema === undefined) continue;
    const shape = tcSchemaProblem(e.schema);
    if (shape) throw new ChatError("invalid", `event ${e.name}: ${shape}`);
    const problem = schemaPatternProblem(e.schema);
    if (problem) throw new ChatError("invalid", `event ${e.name}: ${problem}`, undefined, "artifact.bad_schema");
  }
}
