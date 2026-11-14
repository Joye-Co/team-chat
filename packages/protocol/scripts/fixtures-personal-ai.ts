// Fixtures for a member's own AI member and the VM size an admin sets per machine
// (docs/plans/usable/design-personal-ai.zh.md). Called by gen-fixtures.ts.
export type PersonalAiCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string, reason?: string) => void;
  T0: number;
  ID: Record<string, string>;
};

const machineId = "01JBX0MACH00000000000000AA";
const runId = "01JBX0RUN0000000000000VM01";

export function addPersonalAiFixtures({ ok, bad, T0, ID }: PersonalAiCtx) {
  ok("http/create-bot-request-owner.json", "CreateBotRequest", { name: "小助手", ownerId: ID.alice }, "usable: an admin makes an AI member that belongs to Alice");
  ok("entities/member-bot-owned.json", "Member", { id: ID.bot, kind: "bot", name: "小助手", level: "member", ownerId: ID.alice }, "usable: an AI member of Alice's own");
  ok("http/create-machine-request-owner.json", "CreateMachineRequest", { name: "实习生电脑", ownerId: ID.alice }, "usable: an admin adds a machine that belongs to Alice");
  ok("http/update-machine-request-vm.json", "UpdateMachineRequest", { vm: { vcpus: 2, memMib: 4096 } });
  ok("http/update-machine-request-vm-clear.json", "UpdateMachineRequest", { vm: null }, "back to the daemon's own [vm] size");
  bad("http/update-machine-request-vm-too-big.json", "UpdateMachineRequest", { vm: { vcpus: 64, memMib: 4096 } }, "vcpus is 1 to 16");
  bad("http/update-machine-request-vm-too-small.json", "UpdateMachineRequest", { vm: { vcpus: 2, memMib: 256 } }, "memMib is at least 1024");
  ok("http/machine-vm.json", "Machine", {
    id: machineId,
    name: "实习生电脑",
    labels: ["vm"],
    createdBy: ID.owner,
    createdAt: T0,
    connected: true,
    healthy: true,
    runningRuns: [],
    ownerId: ID.alice,
    vm: { vcpus: 2, memMib: 4096 },
  });
  ok("machine/down-run.assign-vm.json", "MachineDownFrame", {
    t: "run.assign",
    runId,
    botMemberId: ID.bot,
    groupId: ID.general,
    topicId: ID.topic,
    runtime: "claude",
    prompt: "Topic: 修好登录\n\nAlice: @小助手 修一下",
    envAllowlist: [],
    leaseMs: 600000,
    vm: { vcpus: 2, memMib: 4096 },
  }, "usable: the VM size an admin set for this machine");
}
