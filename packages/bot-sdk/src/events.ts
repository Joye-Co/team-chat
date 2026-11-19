// Live events of Card v2.1 blocks: which events a block can send, the check `composeCard` runs
// on a block's `events`, and checked readers of an event's `value` (untrusted data from a
// person's client). The table is the server's own rule (`eventsOf`, server/src/cards2/inputs.ts);
// test/unit.test.ts compares the two.
import type { CardV2Block } from "@chat/protocol";
import { ChatError } from "./errors.ts";

/** The live events a block can send. A name outside this list in `events` never fires. */
export function blockEvents(b: CardV2Block): string[] {
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

/**
 * Throws `ChatError` `invalid` when a block declares a live event its type cannot send: the
 * server would refuse every such event (`card.undeclared_event`), so the mistake shows when the
 * card is built, not as a control that does nothing. An `embed` is not checked: its `events` are
 * names its 产出 version declares.
 */
export function checkBlockEvents(list: readonly CardV2Block[]): void {
  for (const b of list) {
    if (b.type === "embed" || !b.events || b.events.length === 0) continue;
    const can = blockEvents(b);
    const kind = b.type === "input" ? `input ${b.kind}` : b.type;
    for (const name of b.events) {
      if (can.includes(name)) continue;
      const hint = can.length > 0 ? `it can send: ${can.join(", ")}` : "it sends no live events";
      throw new ChatError("invalid", `card: block "${b.id}" (${kind}) cannot send ${name}; ${hint}`);
    }
  }
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter(isStr) : []);

/** The chart point of a `chart.select`. */
export interface EventPoint {
  /** The series id (its name when it has no id). */
  series: string;
  x?: string | number;
  y?: number;
  /** The 0-based index of the point in the series. */
  index?: number;
}

/** Checked readers of a live event's `value`: each returns undefined (or []) on another shape. */
export interface EventValueReaders {
  /** `choice.change`, `choice.submit`, `compare.pick`: the chosen option id (the first of several). */
  choice(): string | undefined;
  /** `choice.change`, `choice.submit` of a multiple choice: the chosen option ids; [] when none. */
  choices(): string[];
  /** `button.press`: the id of the pressed button of a `buttons` block. */
  button(): string | undefined;
  /** `table.rowSelect`: the selected row keys (`rowKeys`, else the row index as text); [] when none. */
  rows(): string[];
  /** `tab.open`: the id of the opened tab. */
  tab(): string | undefined;
  /** `chart.select`: the series and the point or bar. */
  point(): EventPoint | undefined;
  /** `table.sort`: the column and the direction. */
  sort(): { column: string; dir: "asc" | "desc" } | undefined;
}

/** Readers of one event value. 2.0 clients send bare values (`"o2"`, `["o1", "o2"]`); 2.1 clients objects. */
export function eventValueReaders(value: unknown): EventValueReaders {
  const choices = (): string[] => {
    if (isStr(value)) return [value];
    if (Array.isArray(value)) return strings(value);
    if (!isObj(value)) return [];
    if (Array.isArray(value.values)) return strings(value.values);
    return isStr(value.value) ? [value.value] : [];
  };
  return {
    choices,
    choice: () => choices()[0],
    button: () => (isObj(value) && isStr(value.buttonId) ? value.buttonId : undefined),
    rows: () => (isObj(value) ? strings(value.values) : []),
    tab: () => (isObj(value) && isStr(value.tabId) ? value.tabId : undefined),
    point: () => {
      if (!isObj(value) || !isStr(value.series)) return undefined;
      const p: EventPoint = { series: value.series };
      if (isStr(value.x) || (typeof value.x === "number" && Number.isFinite(value.x))) p.x = value.x;
      if (typeof value.y === "number" && Number.isFinite(value.y)) p.y = value.y;
      if (typeof value.index === "number" && Number.isInteger(value.index) && value.index >= 0) p.index = value.index;
      return p;
    },
    sort: () => (isObj(value) && isStr(value.column) && (value.dir === "asc" || value.dir === "desc") ? { column: value.column, dir: value.dir } : undefined),
  };
}
