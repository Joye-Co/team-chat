// Usable supervision fixtures: what the desk did to a run (BotRun.supervision), the daemon's
// "the run's processes are working" flag on a heartbeat (RunCursor.active), and the team's run
// time limit (设置). Called by gen-fixtures.ts.
export type SuperviseCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string, reason?: string) => void;
  T0: number;
  ID: Record<string, string>;
};

export function addSuperviseFixtures({ ok, bad, T0, ID }: SuperviseCtx) {
  const runId = "01JBX0RUN000000000000000AU";
  const run = {
    id: runId,
    botId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    machineId: "01JBX0MACHINE00000000000AV",
    status: "running",
    requestedBy: ID.owner,
    createdAt: T0,
    startedAt: T0 + 1000,
    updatedAt: T0 + 9000,
  };
  ok("entities/bot-run-supervision.json", "BotRun", {
    ...run,
    supervision: [
      { at: T0 + 300_000, action: "nudge", reason: "silent", minutes: 5 },
      { at: T0 + 7_200_000, action: "limit", reason: "time_limit", minutes: 120 },
      { at: T0 + 7_300_000, action: "lucky_guess", reason: "the_moon" },
    ],
  }, "usable: what the desk did by itself; unknown actions and reasons are open strings");
  bad("entities/bot-run-supervision-no-reason.json", "BotRun", { ...run, supervision: [{ at: T0, action: "nudge" }] }, "every act has a reason");
  bad("entities/bot-run-supervision-negative.json", "BotRun", { ...run, supervision: [{ at: T0, action: "nudge", reason: "silent", minutes: -1 }] }, "minutes are not negative");

  const health = { diskFreeBytes: 120000000000, diskTotalBytes: 494000000000, load1: 3.5, load5: 3.1, load15: 2.4, cpuCount: 10, activeRuns: 1 };
  ok("machine/up-heartbeat-active.json", "MachineUpFrame", {
    t: "heartbeat",
    at: 1793627580000,
    health,
    runs: [{ runId, lastSeq: 42, done: false, active: true }],
  }, "usable (盯 1.4): the processes the run's CLI started used CPU since the last heartbeat");
  bad("machine/up-heartbeat-active-string.json", "MachineUpFrame", { t: "heartbeat", at: 1793627580000, health, runs: [{ runId, lastSeq: 42, done: false, active: "yes" }] }, "active is a boolean");

  ok("http/run-settings.json", "RunSettings", { limitMinutes: 120, updatedAt: 0 }, "the default: two hours");
  ok("http/update-run-settings-request.json", "UpdateRunSettingsRequest", { limitMinutes: 240 });
  ok("http/update-run-settings-request-empty.json", "UpdateRunSettingsRequest", {}, "nothing changes");
  bad("http/update-run-settings-request-short.json", "UpdateRunSettingsRequest", { limitMinutes: 10 }, "the limit is 30..1440 minutes");
  bad("http/update-run-settings-request-long.json", "UpdateRunSettingsRequest", { limitMinutes: 1441 }, "the limit is 30..1440 minutes");
  bad("http/update-run-settings-request-fraction.json", "UpdateRunSettingsRequest", { limitMinutes: 90.5 }, "whole minutes");
}
