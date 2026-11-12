// Fixtures for the computers that need an admin (usable, work line 8): which check failed
// (机 5.6), the daemon's last problem lines (机 5.4) and keys that ask before each use (机 4.3).
// Called by gen-fixtures.ts.
export type AlertsCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  T0: number;
  ID: Record<string, string>;
};

const machineId = "01JBX0MACHINE00000000000AV";
const health = { diskFreeBytes: 2_000_000_000, diskTotalBytes: 500_000_000_000, load1: 0.4, load5: 0.3, load15: 0.2, cpuCount: 8, activeRuns: 0 };

export function addAlertsFixtures({ ok, T0, ID }: AlertsCtx) {
  // 机 5.6: which self-check failed
  ok(
    "entities/machine-unhealthy-disk.json",
    "Machine",
    {
      id: machineId,
      name: "laptop",
      labels: [],
      createdBy: ID.owner,
      createdAt: T0,
      connected: true,
      healthy: false,
      unhealthy: "disk",
      lastHeartbeatAt: T0 + 30_000,
      health,
      runningRuns: [],
    },
    "usable: connected, not healthy because free disk is below the limit",
  );

  // 机 5.4: the daemon's last problem lines
  const problems = [
    { at: T0 - 120_000, text: "link down: read: connection reset" },
    { at: T0 - 60_000, text: "self-update: cannot reach the release server" },
  ];
  ok(
    "machine/up-heartbeat-problems.json",
    "MachineUpFrame",
    { t: "heartbeat", at: T0, health, runs: [], metrics: { at: T0 - 100, runs: [], problems } },
    "usable: the last warning and error lines of the daemon's log, redacted on the machine",
  );
  ok(
    "http/machine-monitor-problems.json",
    "MachineMonitor",
    {
      machine: { id: machineId, name: "laptop", connected: true, lastHeartbeatAt: T0 },
      series: { range: "15m", from: T0 - 900_000, stepMs: 30_000, cpuPct: [], memPct: [], diskPct: [], netRxBps: [], netTxBps: [] },
      capacity: { slots: 1, running: 0, canTake: 1 },
      queue: { count: 0 },
      today: { finished: 0, succeeded: 0, failed: 0, finishedYesterday: 0, tokens: 0 },
      clis: [],
      runs: [],
      members: [],
      problems,
    },
    "usable: 最近的问题 from the latest heartbeat",
  );

  // 机 4.3: a key that asks before each use
  ok("http/set-secret-request-ask.json", "SetSecretRequest", { sealed: "c2VhbGVkLWJveC1ieXRlcw==", members: [ID.bot], ask: true }, "usable: 每次用之前要批准");
  ok(
    "http/machine-secrets-ask.json",
    "MachineSecretsResponse",
    {
      publicKey: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
      secrets: [{ name: "CLOUDFLARE_API_TOKEN", last4: "k2Pw", members: [], updatedAt: T0, ask: true }],
    },
    "usable: a key runs get only after a person approves",
  );
  ok(
    "machine/down-secret.set-ask.json",
    "MachineDownFrame",
    { t: "secret.set", name: "CLOUDFLARE_API_TOKEN", sealed: "c2VhbGVkLWJveC1ieXRlcw==", members: [ID.bot], ask: true },
    "usable: keep a key that runs get only after a person approves",
  );
}
