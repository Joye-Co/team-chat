// Fixtures for a bot pinned to one machine and for AI members' home folders on a VM host.
// Called by gen-fixtures.ts (and fixtures-bots.ts for the monitor's home folders).
export type HomesCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  ID: Record<string, string>;
};

const machineId = "01JBX0MACH00000000000000AA";
const bot2 = "01JBX0BOT000000000000000AD";
const homeSize = 21_474_836_480;

/** The home folders the machine monitor shows (http/machine-monitor.json). */
export const monitorHomes = (ID: Record<string, string>) => [
  { memberId: ID.bot, usedBytes: 3_221_225_472, sizeBytes: homeSize },
  { memberId: bot2, usedBytes: 1_048_576 },
];

export function addHomesFixtures({ ok, ID }: HomesCtx) {
  // A bot that runs on one machine
  ok("http/set-bot-runner-request-machine.json", "SetBotRunnerRequest", { runtime: "claude", machineId });
  ok("http/set-bot-runner-request-machine-clear.json", "SetBotRunnerRequest", { runtime: "claude", machineId: null });
  ok("http/bot-runner-machine.json", "BotRunner", {
    botId: ID.bot,
    runtime: "codex",
    labels: ["mac"],
    repo: { path: "/Users/me/code/team-chat" },
    baseBranch: "main",
    envAllowlist: [],
    enabled: true,
    updatedAt: 1_762_128_000_000,
    machineId,
  });

  // Home folders
  ok("machine/down-home.clear.json", "MachineDownFrame", { t: "home.clear", memberId: ID.bot }, "usable, feature homes: empty one AI member's home folder");
  ok(
    "machine/up-heartbeat-homes.json",
    "MachineUpFrame",
    {
      t: "heartbeat",
      at: 1_793_627_430_000,
      health: { diskFreeBytes: 412_000_000_000, diskTotalBytes: 996_000_000_000, load1: 1.2, load5: 1.1, load15: 0.9, cpuCount: 12, activeRuns: 0 },
      runs: [],
      metrics: {
        at: 1_793_627_429_900,
        vmSlots: 4,
        runs: [],
        homes: [
          { memberId: ID.bot, usedBytes: 3_221_225_472, sizeBytes: homeSize },
          { memberId: bot2, usedBytes: 0, sizeBytes: homeSize },
        ],
      },
    },
    "usable, feature homes: a VM host reports each AI member's home folder",
  );
}
