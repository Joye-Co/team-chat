// Usable: one state a person sees per run (phase), pause versus take-over (接管), and a
// person's commands in the run's work folder. See docs/protocol.md, "Run phase and take-over",
// and docs/machine-protocol.md, "Take-over (feature `takeover`)"; design
// docs/plans/usable/design-takeover.zh.md. Readers take every value as an open string.
import { z } from "zod";
import { Count, Id, Millis } from "./common.ts";

/**
 * Known `BotRun.phase` values. A reader that gets another value (or none) works it out from
 * `status` and `pausedBy` as before.
 */
export const RUN_PHASES = [
  "working",
  "waiting_person",
  "needs_reply",
  "waiting_approval",
  "waiting_computer",
  "paused",
  "taken_over",
  "done",
  "failed",
  "stopped",
] as const;
/** Known `BotRun.waitingFor` values. */
export const RUN_WAITING_FOR = ["question", "stuck", "report", "queue", "offline", "booting"] as const;
/** Known `BotRun.holdKind` values and `run.pause.hold` values. */
export const RUN_HOLD_KINDS = ["pause", "takeover"] as const;
/** Known `BotRun.takeover` values: how a person can take over the run's machine. */
export const RUN_TAKEOVER = ["commands", "isolated", "none"] as const;
/** Known `hand.result.error` values. */
export const HAND_ERRORS = ["not_held", "busy", "unsupported"] as const;

export const MAX_HAND_COMMAND_CHARS = 4000;
/** Max UTF-8 bytes of one `hand.output` event. */
export const MAX_HAND_OUTPUT_BYTES = 16_384;
export const MAX_HANDBACK_FILES = 20;
/** A person's command in a run: `[A-Za-z0-9_-]{1,64}`, made by the server. */
export const HandCommandId = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/);

/** What a person did while holding a run (`BotRun.lastHandback`, run event `hand.summary`). */
export const RunHandback = z.object({
  /** The person who held it. */
  by: Id,
  heldMs: Count,
  /** Commands they ran. */
  commands: Count,
  /** Changed paths in the work folder, relative, at most 20. */
  files: z.array(z.string().min(1).max(1024)).max(MAX_HANDBACK_FILES),
  /** HEAD when the hold started and when it ended (no git: absent). */
  headFrom: z.string().min(1).max(64).optional(),
  headTo: z.string().min(1).max(64).optional(),
});
export type RunHandback = z.infer<typeof RunHandback>;

/** Fields added to `BotRun` (usable). All server-made; absent on older runs and servers. */
export const runPhaseFields = {
  /** The one state people see (`RUN_PHASES`); max 32. */
  phase: z.string().min(1).max(32).optional(),
  /** What a waiting phase waits for (`RUN_WAITING_FOR`). */
  waitingFor: z.string().min(1).max(32).optional(),
  /** With `pausedBy` only: "pause" | "takeover". */
  holdKind: z.string().min(1).max(32).optional(),
  /** With `pausedBy` only: when the hold started. */
  heldAt: Millis.optional(),
  /** How the run's machine can be taken over: "commands" | "isolated" | "none". */
  takeover: z.string().min(1).max(32).optional(),
  /** The latest hand-back. */
  lastHandback: RunHandback.optional(),
};

/** `POST /v1/runs/:id/commands` (the person holding the run). */
export const RunCommandRequest = z.object({ command: z.string().min(1).max(MAX_HAND_COMMAND_CHARS) });
export type RunCommandRequest = z.infer<typeof RunCommandRequest>;
export const RunCommandResponse = z.object({ commandId: HandCommandId });
export type RunCommandResponse = z.infer<typeof RunCommandResponse>;
