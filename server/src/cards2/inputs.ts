// Declarative checks of answer inputs and live event values against the block definitions
// (CARD21 §4, INTERACT §3.2). Only shapes, closed id sets, required, min/max and lengths: a
// value is data a person typed, never interpreted.
import type { CardV2Block } from "@chat/protocol";

type Block = CardV2Block;
type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => !!v && typeof v === "object" && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const MAX_OTHER = 60;

/** Option ids of a choice-like block. */
function choiceIds(b: Block): string[] | undefined {
  switch (b.type) {
    case "decision":
    case "imageChoice":
    case "poll":
      return b.options.map((o) => o.id);
    case "slots":
      return b.slots.map((s) => s.id);
    default:
      return undefined;
  }
}

/** Row keys of a table: `rowKeys`, else the 0-based row index as text. */
export function rowKeys(b: Extract<Block, { type: "table" }>): string[] {
  return b.rowKeys ?? b.rows.map((_, i) => String(i));
}

function choiceProblem(b: Block, ids: string[], v: unknown): string | undefined {
  const multiple = (b.type === "decision" || b.type === "imageChoice" || b.type === "slots") && b.multiple === true;
  // 2.0 decision answers: an option id, or an array of option ids.
  let values: unknown[];
  if (isStr(v)) values = [v];
  else if (Array.isArray(v)) values = v;
  else if (isObj(v) && Array.isArray(v.values)) values = v.values;
  else if (isObj(v) && isStr(v.value)) {
    if (v.value === "other" && !ids.includes("other")) {
      if (b.type !== "decision" || !b.allowOther) return "other needs allowOther";
      if (!isStr(v.other) || v.other.length < 1 || [...v.other].length > MAX_OTHER) return `other is 1..${MAX_OTHER} characters`;
      return multiple ? "a multiple choice takes values" : undefined;
    }
    values = [v.value];
  } else return "a choice is {value} or {values}";
  if (!values.every(isStr)) return "choice values are option ids";
  if (new Set(values).size !== values.length) return "choice values are unique";
  for (const x of values) if (!ids.includes(x as string)) return `no option ${String(x)}`;
  if (!multiple && values.length !== 1) return "a single choice takes one value";
  if (multiple && b.type === "decision") {
    const min = b.minSelect ?? 1;
    const max = b.maxSelect ?? ids.length;
    if (values.length < min || values.length > max) return `pick ${min}..${max}`;
  }
  return undefined;
}

const unwrap = (v: unknown): unknown => (isObj(v) && "value" in v ? v.value : v);

/** Why `v` is not a valid value of input block `b`, or undefined. */
export function inputProblem(b: Block, v: unknown): string | undefined {
  const ids = choiceIds(b);
  if (ids) return choiceProblem(b, ids, v);
  switch (b.type) {
    case "input": {
      switch (b.kind) {
        case "text":
        case "textarea":
        case "date": {
          const x = unwrap(v);
          if (!isStr(x)) return "a text input takes text";
          if (b.maxLength !== undefined && [...x].length > b.maxLength) return `at most ${b.maxLength} characters`;
          return undefined;
        }
        case "number":
        case "slider": {
          const x = unwrap(v);
          if (typeof x !== "number" || !Number.isFinite(x)) return "a number input takes a number";
          if ((b.min !== undefined && x < b.min) || (b.max !== undefined && x > b.max)) return "number out of range";
          return undefined;
        }
        case "switch":
          return typeof unwrap(v) === "boolean" ? undefined : "a switch takes true or false";
        case "select": {
          const x = unwrap(v);
          return isStr(x) && (b.options ?? []).some((o) => o.id === x) ? undefined : "no such option";
        }
        case "chips": {
          const xs = isObj(v) && Array.isArray(v.values) ? v.values : v;
          if (!Array.isArray(xs) || !xs.every(isStr)) return "chips take values";
          const known = new Set((b.options ?? []).map((o) => o.id));
          return known.size === 0 || xs.every((x) => known.has(x)) ? undefined : "no such option";
        }
        default:
          return undefined;
      }
    }
    case "rank": {
      if (!isObj(v) || !Array.isArray(v.order) || !v.order.every(isStr)) return "a rank takes {order}";
      const all = b.items.map((i) => i.id);
      const order = v.order as string[];
      if (new Set(order).size !== order.length || order.some((x) => !all.includes(x))) return "order names each item once";
      const want = b.rankTop ?? all.length;
      return order.length === want || order.length === all.length ? undefined : `order has ${want} items`;
    }
    case "rating": {
      const x = unwrap(v);
      return Number.isInteger(x) && (x as number) >= 1 && (x as number) <= 5 ? undefined : "a rating is 1..5";
    }
    case "scale": {
      const x = unwrap(v);
      return Number.isInteger(x) && (x as number) >= b.min && (x as number) <= b.max ? undefined : `a scale is ${b.min}..${b.max}`;
    }
    case "confirm":
      return typeof unwrap(v) === "boolean" ? undefined : "a confirm takes true or false";
    case "matrix": {
      if (!isObj(v) || !isObj(v.rows)) return "a matrix takes {rows}";
      const rows = v.rows;
      const cols = new Set(b.columns.map((c) => c.id));
      for (const [r, c] of Object.entries(rows)) {
        if (!b.rows.some((x) => x.id === r)) return `no row ${r}`;
        if (!isStr(c) || !cols.has(c)) return `no column ${String(c)}`;
      }
      for (const r of b.rows) if ((r.required || b.required) && !(r.id in rows)) return `row ${r.id} is required`;
      return undefined;
    }
    case "compare": {
      const x = unwrap(v);
      if (!isStr(x) || !b.options.some((o) => o.id === x)) return "no such option";
      if (isObj(v) && v.note !== undefined) {
        if (!b.note) return "this compare takes no note";
        if (!isStr(v.note) || [...v.note].length > (b.note.maxLength ?? 1000)) return "note is too long";
      }
      return undefined;
    }
    case "table": {
      if (!b.select) return "the table is not an input";
      const keys = rowKeys(b);
      const xs = isObj(v) && Array.isArray(v.values) ? v.values : isObj(v) && "value" in v ? [v.value] : [v];
      if (!xs.every((x) => isStr(x) && keys.includes(x))) return "no such row";
      if (b.select === "single" && xs.length !== 1) return "a single select takes one row";
      return undefined;
    }
    case "draft":
      return isStr(v) || isObj(v) ? undefined : "a draft takes text or {fields}";
    default:
      return "this block takes no input";
  }
}

/** Why `value` is not valid for a live `event` of block `b`, or undefined. */
export function eventValueProblem(b: Block, event: string, value: unknown): string | undefined {
  switch (event) {
    case "choice.change":
    case "choice.submit":
    case "input.change":
    case "input.submit":
      return inputProblem(b, value);
    case "compare.pick":
      return b.type === "compare" ? inputProblem(b, value) : "not a compare";
    case "chart.select": {
      if (b.type !== "chart") return "not a chart";
      if (!isObj(value)) return "chart.select takes {series, x, y, index}";
      const s = b.series.find((x) => (x.id ?? x.name) === value.series);
      if (!s) return "no such series";
      if (value.index !== undefined && (!Number.isInteger(value.index) || (value.index as number) < 0 || (value.index as number) >= s.values.length)) {
        return "no such point";
      }
      return undefined;
    }
    case "table.rowSelect": {
      if (b.type !== "table") return "not a table";
      const keys = rowKeys(b);
      if (!isObj(value) || !Array.isArray(value.values)) return "table.rowSelect takes {values}";
      return value.values.every((x) => isStr(x) && keys.includes(x)) ? undefined : "no such row";
    }
    case "table.sort":
      if (b.type !== "table") return "not a table";
      return isObj(value) && isStr(value.column) && (value.dir === "asc" || value.dir === "desc") ? undefined : "table.sort takes {column, dir}";
    case "button.press":
      if (b.type !== "buttons") return "not buttons";
      return isObj(value) && b.buttons.some((x) => x.id === value.buttonId) ? undefined : "no such button";
    case "tab.open":
      if (b.type !== "tabs") return "not tabs";
      return isObj(value) && b.tabs.some((t) => t.id === value.tabId) ? undefined : "no such tab";
    case "draft.edit":
      if (b.type !== "draft") return "not a draft";
      return isObj(value) && isStr(value.fieldId) ? undefined : "draft.edit takes {fieldId, value}";
    default:
      return undefined;
  }
}

/** The events a block type can send. */
export function eventsOf(b: Block): string[] {
  switch (b.type) {
    case "decision":
    case "imageChoice":
    case "slots":
    case "poll":
    case "confirm":
      return ["choice.change", "choice.submit"];
    case "rank":
    case "matrix":
    case "rating":
    case "scale":
      return ["input.change"];
    case "input":
      return b.kind === "text" || b.kind === "number" ? ["input.change", "input.submit"] : ["input.change"];
    case "chart":
      return ["chart.select", "chart.clear"];
    case "metric":
      return ["metric.press"];
    case "table":
      return ["table.rowSelect", "table.sort"];
    case "buttons":
      return ["button.press"];
    case "draft":
      return ["draft.edit"];
    case "compare":
      return ["compare.pick"];
    case "tabs":
      return ["tab.open"];
    case "questionnaire":
      return ["action.press"];
    default:
      return [];
  }
}
