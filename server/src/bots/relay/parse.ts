// Usable (design-relay R3): a relay (按顺序做) is written by a person as the first line of a
// message in a topic: `@A note → @B note → @C note`, separators `→`, `->` or `=>`. With a second
// line, the lines from the second on are the goal; with one line, the text after the last @ is
// the goal and the texts after the others are their steps' notes. The order is the order of the
// @ names in the body (a message's `mentions` are deduplicated and cannot give it), each matched
// against the members the message mentions. Pure: the room checks who can take the work.
import { MAX_RELAY_STEPS, MIN_RELAY_STEPS } from "@chat/protocol";

/** Longest note of one step, and of the goal (`TopicRelay`). */
export const MAX_RELAY_NOTE = 100;
export const MAX_RELAY_GOAL = 1000;

const ARROW = /\s*(?:→|->|=>)\s*/;

export interface RelayDraft {
  steps: { memberId: string; note?: string }[];
  goal: string;
}

/** A first line that starts like an arrow line but is not a valid one (`relay.bad_steps`). */
export class RelayParseError extends Error {
  readonly reason = "relay.bad_steps";
}

function cut(s: string, n: number): string {
  const cs = [...s];
  return cs.length <= n ? s : cs.slice(0, n).join("");
}

/** True when the first line is an arrow line (checked before any member is looked up). */
export function looksLikeRelay(body: string): boolean {
  const first = (body.split("\n")[0] ?? "").trim();
  if (!first.startsWith("@") || !ARROW.test(first)) return false;
  const parts = first.split(ARROW);
  return (parts[parts.length - 1] ?? "").trim().startsWith("@");
}

/**
 * The relay a message asks for, or null when its first line is no arrow line: it does not start
 * with an @, has no separator, or its last part is not an @ (as in `@A compare x -> y`). Throws
 * `RelayParseError` for a broken one: an empty part, a part that names no mentioned member, fewer
 * than 2 or more than 6 steps, the same member twice in a row, or no goal.
 */
export function parseRelay(body: string, members: { id: string; name: string }[]): RelayDraft | null {
  if (!looksLikeRelay(body)) return null;
  const lines = body.split("\n");
  const parts = (lines[0] ?? "").trim().split(ARROW).map((p) => p.trim());
  // Longest name first, so `@小码2` is not read as `@小码` with the note `2`.
  const byName = [...members].filter((m) => m.name !== "").sort((a, b) => [...b.name].length - [...a.name].length);
  const steps: { memberId: string; text: string }[] = [];
  for (const p of parts) {
    if (!p.startsWith("@")) throw new RelayParseError("every part of an arrow line starts with an @ member");
    const rest = p.slice(1);
    const hit = byName.find((m) => rest.startsWith(m.name));
    if (!hit) throw new RelayParseError("an @ in the arrow line names no member the message mentions");
    steps.push({ memberId: hit.id, text: rest.slice(hit.name.length).trim() });
  }
  if (steps.length < MIN_RELAY_STEPS || steps.length > MAX_RELAY_STEPS) {
    throw new RelayParseError(`a relay has ${MIN_RELAY_STEPS} to ${MAX_RELAY_STEPS} steps`);
  }
  for (let i = 1; i < steps.length; i++) {
    if (steps[i].memberId === steps[i - 1].memberId) throw new RelayParseError("the same member cannot do two steps in a row");
  }
  const rest = lines.slice(1).join("\n").trim();
  const twoLines = rest !== "";
  const goal = twoLines ? rest : steps[steps.length - 1].text;
  if (goal === "") throw new RelayParseError("a relay needs a goal: after the last @, or on the next line");
  return {
    goal: cut(goal, MAX_RELAY_GOAL),
    steps: steps.map((s, i) => {
      const note = twoLines || i < steps.length - 1 ? cut(s.text, MAX_RELAY_NOTE) : "";
      return note !== "" ? { memberId: s.memberId, note } : { memberId: s.memberId };
    }),
  };
}
