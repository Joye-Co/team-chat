// Checks of the page events a 产出 version declares, as the server checks them when the version
// is made. The `pattern` rule is the server's own (packages/protocol/src/pattern.ts): a Thompson
// NFA that refuses a backreference, lookaround and a pattern of more than 1000 states.
import { type ArtifactEventDecl, ARTIFACT_EVENT_NAME_RE, schemaPatternProblem, tcSchemaProblem } from "@chat/protocol";
import { ChatError } from "./errors.ts";

export { MAX_PATTERN_INPUT, patternProblem, schemaPatternProblem } from "@chat/protocol";

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
