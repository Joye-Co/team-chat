// The prompt patch register (docs/design/agent-harness.zh.md 9): every piece of prompt text that
// teaches a model how to behave, why it is there, the failure it fixed and the model it was added
// with. When the model generation changes, replay with and without each patch
// (scripts/replay-frontdesk.ts) and remove the ones the plain model no longer needs.
export interface PromptPatch {
  id: string;
  /** Where the text lives. */
  where: string;
  why: string;
  /** The failure it fixed (a replay case id or a short description). */
  failure: string;
  addedWith: string;
  added: string;
}

export const PATCHES: PromptPatch[] = [
  {
    id: "P1-acceptance",
    where: "server/src/bots/dispatch.ts ACCEPTANCE_RULE",
    why: "The done card needs checkable acceptance criteria; CLIs did not report them unasked.",
    failure: "done cards without a checklist for code work",
    addedWith: "claude-opus-4-x / gpt-5.x",
    added: "1.1",
  },
  {
    id: "P2-context-tools",
    where: "server/src/bots/dispatch.ts CONTEXT_TOOLS_RULE",
    why: "The prompt carries only part of a long topic; runs guessed instead of reading older messages.",
    failure: "a member could not say what the first message of a 200-message topic asked for",
    addedWith: "claude-opus-5-5 / gpt-6",
    added: "M1",
  },
  {
    id: "P3-frontdesk-dispatch-when-unsure",
    where: "server/src/agent/frontdesk.ts systemPrompt (\"When unsure between reply and dispatch, dispatch\")",
    why: "A wrong reply claims knowledge the desk does not have; a wrong dispatch only costs a run.",
    failure: "the desk answered a repository question from general knowledge",
    addedWith: "claude-haiku-5-5 / deepseek-chat",
    added: "M2",
  },
  {
    id: "P4-supervise-nudge",
    where: "server/src/agent/supervise.ts NUDGE_TEXT, LOOP_TEXT, CONTINUE_TEXT",
    why: "Silent, looping or promise-only runs; the desk steps in before people have to.",
    failure: "runs stuck for hours; runs that ended with 我来看看",
    addedWith: "claude-opus-5-5 / gpt-6",
    added: "M3",
  },
];
