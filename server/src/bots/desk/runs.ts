// BotDesk runs: starting a run from a mention (claim, ack, machine, prompt, assign) and the
// MachineLink answers to an assignment (accepted, rejected, lost).
import type { BotRunner, RunAssignFrame } from "@chat/protocol";
import { roomStub } from "../../core/env";
import { unwrap } from "../../core/errors";
import { ulid } from "../../core/ids";
import { LEASE_MS, MAX_RUN_MS, type MachineView, TEXT, buildPrompt, cut, pickMachine } from "../dispatch";
import { linkStub } from "../env";
import type { ClaimResult, TopicMessage } from "../room";
import { loadMachines, loadRunner } from "../store";
import { DeskCore, type RunRow } from "./core";

export abstract class DeskRuns extends DeskCore {
  // -------------------------------------------------------------------------
  // Starting a run
  // -------------------------------------------------------------------------

  protected async start(m: TopicMessage, followUpOf?: RunRow, followUps: { authorId: string; body: string }[] = []): Promise<void> {
    const botId = this.botId as string;
    const runner = await loadRunner(this.env.DB, botId);
    if (!runner?.enabled) return;
    const claim = unwrap(
      await roomStub(this.env, m.groupId).rpc(m.groupId, "bots.claim", this.actor(), { topicId: m.topicId }),
    ) as ClaimResult;
    if (claim.botId !== botId) return; // Another bot is responsible for this topic.
    if (this.activeRun(m.topicId)) return; // Raced with another message.

    const now = Date.now();
    const runId = ulid(now);
    this.sql.exec(
      `INSERT INTO runs (id, group_id, topic_id, status, requested_by, trigger_message_id, created_at, updated_at, last_event_at, follow_up_of)
       VALUES (?, ?, ?, 'created', ?, ?, ?, ?, ?, ?)`,
      runId,
      m.groupId,
      m.topicId,
      m.authorId,
      m.messageId,
      now,
      now,
      now,
      followUpOf?.id ?? null,
    );
    const r = this.run(runId) as RunRow;
    await this.serial(runId, async () => {
      try {
        await this.env.DB.prepare(
          "INSERT OR IGNORE INTO bot_runs (id, bot_id, group_id, topic_id, machine_id, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
        )
          .bind(runId, botId, m.groupId, m.topicId, now)
          .run();
        await this.post(r, `run:${runId}:ack`, TEXT.ack);
        await this.setStatus(r, "created");
        await this.topic(r, "in_progress");

        const machine = pickMachine(await loadMachines(this.env), runner, Date.now());
        if (!machine) return this.end(r, "failed", TEXT.noMachine, "blocked", TEXT.noMachine);

        const names = await this.names([...claim.messages.map((x) => x.authorId), m.authorId, botId]);
        const prompt = buildPrompt({
          botName: names.get(botId) ?? "bot",
          title: claim.title,
          messages: claim.messages.filter((x) => x.body !== "" && x.id !== m.messageId),
          names,
          request: { authorId: m.authorId, body: m.body },
          followUps,
        });
        await this.assign(r, runner, machine, prompt);
      } catch (err) {
        // A run that never reached a machine must not block the topic.
        this.log.error("run start failed", { runId, err });
        if (this.run(runId)?.ended_at === null && !this.run(runId)?.machine_id) {
          await this.end(r, "failed", TEXT.failed(""), "blocked", TEXT.failed(""));
        }
      }
      await this.scheduleStall();
    });
  }

  protected assignFrame(r: RunRow, runner: BotRunner, prompt: string): RunAssignFrame {
    const frame: RunAssignFrame = {
      t: "run.assign",
      runId: r.id,
      botMemberId: this.botId as string,
      groupId: r.group_id,
      topicId: r.topic_id,
      runtime: runner.runtime,
      prompt,
      envAllowlist: runner.envAllowlist,
      leaseMs: LEASE_MS,
      maxDurationMs: MAX_RUN_MS,
    };
    if (runner.model) frame.model = runner.model;
    if (runner.repo) frame.repo = { ...runner.repo, ...(runner.baseBranch ? { baseBranch: runner.baseBranch } : {}) };
    return frame;
  }

  protected async assign(r: RunRow, runner: BotRunner, machine: MachineView, prompt: string): Promise<void> {
    const frame = this.assignFrame(r, runner, prompt);
    this.sql.exec("UPDATE runs SET machine_id = ? WHERE id = ?", machine.id, r.id);
    r.machine_id = machine.id;
    await this.env.DB.prepare("UPDATE bot_runs SET machine_id = ? WHERE id = ?").bind(machine.id, r.id).run();
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `prompt:${r.id}`, prompt);
    await linkStub(this.env, machine.id).assign(machine.id, this.botId as string, frame);
  }

  // -------------------------------------------------------------------------
  // From MachineLink
  // -------------------------------------------------------------------------

  async runAccepted(runId: string, machineId: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId || r.status !== "created") return;
      await this.setStatus(r, "running", { started_at: Date.now(), last_event_at: Date.now() });
      this.sql.exec("UPDATE runs SET last_event_at = ? WHERE id = ?", r.last_event_at, runId);
      await this.scheduleStall();
    });
  }

  async runRejected(runId: string, machineId: string, reason: string, message: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId) return;
      this.log.info("run rejected", { runId, machineId, reason, message: cut(message, 200) });
      const excluded = [...(JSON.parse(r.excluded) as string[]), machineId];
      this.sql.exec("UPDATE runs SET excluded = ? WHERE id = ?", JSON.stringify(excluded), runId);
      const runner = await loadRunner(this.env.DB, this.botId as string);
      const machine = runner?.enabled ? pickMachine(await loadMachines(this.env), runner, Date.now(), excluded) : null;
      const prompt = this.meta(`prompt:${runId}`);
      if (!runner || !machine || prompt === null) return this.end(r, "failed", TEXT.noMachine, "blocked", TEXT.noMachine);
      await this.assign(r, runner, machine, prompt);
    });
  }

  async runLost(runId: string, machineId: string): Promise<void> {
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || r.ended_at !== null || r.machine_id !== machineId) return;
      await this.end(r, "blocked", TEXT.lost, "blocked", TEXT.lost);
    });
  }
}
