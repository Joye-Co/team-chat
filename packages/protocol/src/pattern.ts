// TcSchema `pattern` without backtracking (SEC-4). The one rule of the server (files/bridge.ts,
// files/room.ts) and the bot SDK (a bot learns of a refused pattern before it publishes). A bot declares the pattern and a viewer's page
// sends the string, so a JavaScript RegExp here would let a pattern such as `^(a+)+$` stall the
// group room. The pattern is compiled to a Thompson NFA and run over the string's code points in
// O(length × states). Supported: the `u`-flag syntax without backreferences and lookaround
// (`\1`, `\k<…>`, `(?=`, `(?!`, `(?<=`, `(?<!`). A version that declares another pattern is
// refused (`invalid`, reason `artifact.bad_schema`). Single characters (`.`, classes, escapes)
// are tested with a one-character RegExp, which cannot backtrack.

/** NFA states at most, after `{n,m}` is expanded. */
const MAX_STATES = 1000;
/** A string checked by a pattern has at most this many code points (else it does not match). */
export const MAX_PATTERN_INPUT = 4096;
const CACHE_SIZE = 256;

type Test = (cp: number) => boolean;
type Node =
  | { k: "char"; test: Test }
  | { k: "seq"; items: Node[] }
  | { k: "alt"; alts: Node[] }
  | { k: "rep"; child: Node; min: number; max: number }
  | { k: "assert"; at: "bol" | "eol" | "word" | "notword" };

type State =
  | { t: "char"; test: Test; out: number }
  | { t: "split"; a: number; b: number }
  | { t: "assert"; at: "bol" | "eol" | "word" | "notword"; out: number }
  | { t: "match" };

export class UnsupportedPattern extends Error {}

/** The pattern compiled, or an UnsupportedPattern / SyntaxError. */
type Compiled = { states: State[]; start: number };
const cache = new Map<string, Compiled | Error>();

/** A test of one code point: a literal, or a one-character RegExp (`.`, a class, an escape), memoized. */
function singleChar(src: string, literal = false): Test {
  if (literal) {
    const cp = src.codePointAt(0) as number;
    return (c) => c === cp;
  }
  const re = new RegExp(`^(?:${src})$`, "u");
  const memo = new Map<number, boolean>();
  return (c) => {
    let v = memo.get(c);
    if (v === undefined) {
      v = re.test(String.fromCodePoint(c));
      if (memo.size < 4096) memo.set(c, v);
    }
    return v;
  };
}

const isWord = (cp: number | undefined) =>
  cp !== undefined && ((cp >= 48 && cp <= 57) || (cp >= 65 && cp <= 90) || (cp >= 97 && cp <= 122) || cp === 95);

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
    return alts.length === 1 ? alts[0] : { k: "alt", alts };
  }

  alternative(): Node {
    const items: Node[] = [];
    while (this.i < this.p.length && this.peek() !== "|" && this.peek() !== ")") {
      const atom = this.term();
      items.push(this.quantified(atom));
    }
    return { k: "seq", items };
  }

  term(): Node {
    const c = this.peek();
    if (c === "^" || c === "$") {
      this.i++;
      return { k: "assert", at: c === "^" ? "bol" : "eol" };
    }
    if (c === "(") return this.group();
    if (c === "[") return this.charClass();
    if (c === "\\") return this.escape();
    // A literal or `.`: one code point.
    const cp = this.p.codePointAt(this.i) as number;
    const ch = String.fromCodePoint(cp);
    this.i += ch.length;
    return { k: "char", test: ch === "." ? singleChar(".") : singleChar(ch, true) };
  }

  group(): Node {
    this.i++;
    if (this.peek() === "?") {
      const n = this.peek(1);
      if (n === ":") this.i += 2;
      else if (n === "<" && this.peek(2) !== "=" && this.peek(2) !== "!") {
        const end = this.p.indexOf(">", this.i);
        this.i = end + 1;
      } else throw new UnsupportedPattern("lookaround is not supported");
    }
    const inner = this.disjunction();
    if (this.peek() !== ")") throw new SyntaxError("unclosed group");
    this.i++;
    return inner;
  }

  charClass(): Node {
    const from = this.i;
    this.i++;
    if (this.peek() === "^") this.i++;
    while (this.i < this.p.length && this.peek() !== "]") this.i += this.peek() === "\\" ? 2 : 1;
    if (this.peek() !== "]") throw new SyntaxError("unclosed class");
    this.i++;
    return { k: "char", test: singleChar(this.p.slice(from, this.i)) };
  }

  escape(): Node {
    const from = this.i;
    const c = this.peek(1);
    if (/[1-9]/.test(c) || c === "k") throw new UnsupportedPattern("backreferences are not supported");
    if (c === "b" || c === "B") {
      this.i += 2;
      return { k: "assert", at: c === "b" ? "word" : "notword" };
    }
    this.i += 2;
    if ((c === "p" || c === "P") && this.peek() === "{") this.i = this.p.indexOf("}", this.i) + 1;
    else if (c === "u" && this.peek() === "{") this.i = this.p.indexOf("}", this.i) + 1;
    else if (c === "u") this.i += 4;
    else if (c === "x") this.i += 2;
    else if (c === "c") this.i += 1;
    else if (c === "") throw new SyntaxError("trailing backslash");
    else {
      // A surrogate pair escaped as one character.
      const cp = this.p.codePointAt(from + 1) as number;
      this.i = from + 1 + String.fromCodePoint(cp).length;
    }
    return { k: "char", test: singleChar(this.p.slice(from, this.i)) };
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

function build(root: Node): Compiled {
  const states: State[] = [{ t: "match" }];
  const push = (s: State): number => {
    if (states.length >= MAX_STATES) throw new UnsupportedPattern("the pattern is too large");
    states.push(s);
    return states.length - 1;
  };
  const compile = (n: Node, next: number): number => {
    switch (n.k) {
      case "char":
        return push({ t: "char", test: n.test, out: next });
      case "assert":
        return push({ t: "assert", at: n.at, out: next });
      case "seq": {
        let at = next;
        for (let i = n.items.length - 1; i >= 0; i--) at = compile(n.items[i], at);
        return at;
      }
      case "alt": {
        const starts = n.alts.map((a) => compile(a, next));
        let at = starts[starts.length - 1];
        for (let i = starts.length - 2; i >= 0; i--) at = push({ t: "split", a: starts[i], b: at });
        return at;
      }
      case "rep": {
        let tail = next;
        if (n.max === Number.POSITIVE_INFINITY) {
          const loop = push({ t: "split", a: -1, b: next });
          (states[loop] as { a: number }).a = compile(n.child, loop);
          tail = loop;
        } else {
          for (let i = 0; i < n.max - n.min; i++) tail = push({ t: "split", a: compile(n.child, tail), b: next });
        }
        for (let i = 0; i < n.min; i++) tail = compile(n.child, tail);
        return tail;
      }
    }
  };
  const start = compile(root, 0);
  return { states, start };
}

function compiled(pattern: string): Compiled {
  let c = cache.get(pattern);
  if (!c) {
    try {
      // The RegExp parser checks the syntax; it is never run.
      new RegExp(pattern, "u");
      const p = new Parser(pattern);
      const root = p.disjunction();
      if (p.i !== pattern.length) throw new SyntaxError("unbalanced parenthesis");
      c = build(root);
    } catch (err) {
      c = err instanceof Error ? err : new SyntaxError(String(err));
    }
    if (cache.size >= CACHE_SIZE) cache.delete(cache.keys().next().value as string);
    cache.set(pattern, c);
  }
  if (c instanceof Error) throw c;
  return c;
}

/** Why the server cannot check `pattern`, or undefined when it can. */
export function patternProblem(pattern: string): string | undefined {
  try {
    compiled(pattern);
    return undefined;
  } catch (err) {
    if (err instanceof UnsupportedPattern) return `pattern: ${err.message}`;
    return "pattern is not a valid regular expression";
  }
}

/** `new RegExp(pattern, "u").test(value)`, in linear time. Throws for a pattern patternProblem refuses. */
export function patternTest(pattern: string, value: string): boolean {
  const { states, start } = compiled(pattern);
  if (value.length > MAX_PATTERN_INPUT * 2 || [...value].length > MAX_PATTERN_INPUT) return false;
  const cps = Array.from(value, (ch) => ch.codePointAt(0) as number);
  const n = cps.length;
  const mark = new Int32Array(states.length).fill(-1);
  let gen = 0;
  let found = false;
  const add = (list: number[], s: number, pos: number): void => {
    const stack = [s];
    while (stack.length > 0) {
      const i = stack.pop() as number;
      if (mark[i] === gen) continue;
      mark[i] = gen;
      const st = states[i];
      if (st.t === "match") found = true;
      else if (st.t === "split") stack.push(st.b, st.a);
      else if (st.t === "assert") {
        const ok =
          st.at === "bol"
            ? pos === 0
            : st.at === "eol"
              ? pos === n
              : (isWord(cps[pos - 1]) !== isWord(cps[pos])) === (st.at === "word");
        if (ok) stack.push(st.out);
      } else list.push(i);
    }
  };
  let cur: number[] = [];
  for (let pos = 0; pos <= n; pos++) {
    // Search: a match may start at any position.
    add(cur, start, pos);
    if (found) return true;
    if (pos === n) break;
    gen++;
    const next: number[] = [];
    const cp = cps[pos];
    for (const i of cur) {
      const st = states[i] as { t: "char"; test: Test; out: number };
      if (st.test(cp)) add(next, st.out, pos + 1);
    }
    if (found) return true;
    cur = next;
  }
  return false;
}

type SchemaPatterns = { pattern?: string; properties?: Record<string, SchemaPatterns>; items?: SchemaPatterns };

/** The first `pattern` in a declared TcSchema that the server cannot check, or undefined. */
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
