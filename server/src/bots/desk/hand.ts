// BotDesk: a person's commands while taking over a run (design-takeover 3.4, 3.6, 3.7; T3).
// The person holding a take-over runs one command at a time in the run's work folder. Commands
// are sent only while the machine is connected (never queued), and their output (`hand.output`)
// is shown only to the person who ran it and admins.
import { HandCommandId, MAX_HAND_COMMAND_CHARS, RunHandback } from "@chat/protocol";
import { ApiError, type Result } from "../../core/errors";
import { ulid } from "../../core/ids";
import { linkStub } from "../env";
import { type RunRow, result } from "./core";
import { HAND_COMMAND_MAX_MS, type HoldActor } from "./hold";
import { holdKindOf } from "./phase";
import { DeskTakeover } from "./takeover";

/** Who reads a run's events: the person, and whether they are an admin. */
export interface RunViewer {
  id: string;
  admin: boolean;
}

/** A run log frame that is a person's command output (`hand.output`). */
function handOutput(entry: unknown): string | null {
  const f = entry as { t?: unknown; event?: { type?: unknown; commandId?: unknown } } | null;
  return f?.t === "run.event" && f.event?.type === "hand.output" && typeof f.event.commandId === "string" ? f.event.commandId : null;
}

export abstract class DeskHand extends DeskTakeover {
  /** The run taken over by `actor`, with no other check; throws for anything else. */
  private heldBy(r: RunRow | undefined, actor: HoldActor): RunRow {
    if (!r) throw new ApiError("not_found", "run not found");
    if (r.ended_at !== null) throw new ApiError("conflict", "the run ended", "run.ended");
    if (!r.paused_by || holdKindOf(r) !== "takeover") {
      if (actor.kind !== "human") throw new ApiError("forbidden", "only the person holding the run runs commands");
      throw new ApiError("conflict", "the run is not taken over", "run.not_held");
    }
    if (r.paused_by !== actor.id) throw new ApiError("forbidden", "only the person holding the run runs commands");
    if (!r.machine_id) throw new ApiError("conflict", "the run has no machine", "run.no_machine");
    return r;
  }

  /** The command that runs now, or null; one without a result for too long no longer counts. */
  private runningCommand(r: RunRow, now: number): string | null {
    if (!r.hand_command) return null;
    if (r.hand_command_at !== null && now - r.hand_command_at > HAND_COMMAND_MAX_MS) return null;
    return r.hand_command;
  }

  /** `POST /v1/runs/:id/commands`: runs `command` in the run's work folder now. */
  async runCommand(botId: string, runId: string, actor: HoldActor, command: string): Promise<Result<{ commandId: string }>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.heldBy(this.run(runId), actor);
        if (command.length === 0 || command.length > MAX_HAND_COMMAND_CHARS) throw new ApiError("invalid", "command is 1..4000 characters");
        const now = Date.now();
        if (this.runningCommand(r, now)) throw new ApiError("conflict", "a command is running", "hand.busy");
        const commandId = ulid();
        const sent = await linkStub(this.env, r.machine_id as string).toRun({ t: "run.command", runId, commandId, command, by: actor.id } as never, { noQueue: true });
        if (!sent.ok) throw new ApiError("conflict", "the run is not open on its machine", "run.no_machine");
        // Never queued: a command must not run hours later when the machine is back.
        if (!sent.sent) throw new ApiError("conflict", "the run's machine is offline", "hand.offline");
        this.sql.exec("UPDATE runs SET hand_command = ?, hand_command_at = ? WHERE id = ?", commandId, now, runId);
        r.hand_command = commandId;
        r.hand_command_at = now;
        this.log.info("hand command", { runId, commandId, actorId: actor.id });
        return { commandId };
      });
    });
  }

  /** `POST /v1/runs/:id/commands/:commandId/cancel`: stops the person's running command. */
  async cancelCommand(botId: string, runId: string, actor: HoldActor, commandId: string): Promise<Result<{ ok: true }>> {
    return result(() => {
      this.bind(botId);
      return this.serial(runId, async () => {
        const r = this.heldBy(this.run(runId), actor);
        if (!HandCommandId.safeParse(commandId).success) throw new ApiError("not_found", "command not found");
        // A command that already finished has nothing to stop.
        if (r.hand_command !== commandId) return { ok: true as const };
        const sent = await linkStub(this.env, r.machine_id as string).toRun({ t: "run.command.cancel", runId, commandId } as never, { noQueue: true });
        if (sent.ok && !sent.sent) throw new ApiError("conflict", "the run's machine is offline", "hand.offline");
        return { ok: true as const };
      });
    });
  }

  /**
   * The daemon's `hand.*` events (events.ts sends them here, apart from the AI member's
   * progress): `hand.result` frees the command slot, `hand.summary` keeps the hand-back.
   */
  protected async handEvents(r: RunRow, events: (Record<string, unknown> & { type: string })[]): Promise<void> {
    let changed = false;
    for (const ev of events) {
      if (ev.type === "hand.result" && typeof ev.commandId === "string" && ev.commandId === r.hand_command) {
        this.sql.exec("UPDATE runs SET hand_command = NULL, hand_command_at = NULL WHERE id = ?", r.id);
        r.hand_command = null;
        r.hand_command_at = null;
      } else if (ev.type === "hand.summary") {
        const { type: _t, ...rest } = ev;
        const parsed = RunHandback.safeParse(rest);
        if (!parsed.success) {
          this.log.warn("hand.summary does not parse", { runId: r.id });
          continue;
        }
        r.last_handback = JSON.stringify(parsed.data);
        this.sql.exec("UPDATE runs SET last_handback = ? WHERE id = ?", r.last_handback, r.id);
        changed = true;
      }
    }
    if (changed) await this.setStatus(r, r.status);
  }

  /**
   * The run log as `viewer` may read it: a person's command output (`hand.output`) only for the
   * person who ran that command and admins (design-takeover 3.6).
   */
  protected visibleEntries(runId: string, entries: unknown[], viewer?: RunViewer): unknown[] {
    if (!viewer || viewer.admin || !entries.some((e) => handOutput(e) !== null)) return entries;
    const mine = new Set<string>();
    const rows = this.sql
      .exec<{ frame: string }>(`SELECT frame FROM run_log WHERE run_id = ? AND frame LIKE '%"type":"hand.command"%'`, runId)
      .toArray();
    for (const x of rows) {
      try {
        const ev = (JSON.parse(x.frame) as { event?: { type?: unknown; commandId?: unknown; by?: unknown } }).event;
        if (ev?.type === "hand.command" && ev.by === viewer.id && typeof ev.commandId === "string") mine.add(ev.commandId);
      } catch {
        // A frame that does not parse gives nobody its output.
      }
    }
    return entries.filter((e) => {
      const id = handOutput(e);
      return id === null || mine.has(id);
    });
  }
}
