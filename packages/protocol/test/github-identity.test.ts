// GitHub by computer (docs/plans/usable/design-github-identity.zh.md 3.2, 3.3, 3.6): the bounds of
// the new optional fields. The fixtures cover the shapes; these cover the lengths.
import { expect, test } from "bun:test";
import {
  GITHUB_UNAVAILABLE_REASONS,
  Machine,
  MACHINE_FEATURE_GITHUB_HOST,
  MachineGithub,
  MachineMetrics,
  MachineMonitor,
  RunEvent,
  RunGithub,
} from "../src/index.ts";

const committer = { name: "dev", email: "ai-B1@users.noreply.invalid" };
const github = { source: "machine", memberId: "01JBX0OWNER0000000000000AA", committer };

test("RunGithub.askedBy is optional and 1..100 characters", () => {
  expect(RunGithub.safeParse(github).success).toBe(true);
  expect(RunGithub.safeParse({ ...github, askedBy: "实习生" }).success).toBe(true);
  expect(RunGithub.safeParse({ ...github, askedBy: "x".repeat(100) }).success).toBe(true);
  expect(RunGithub.safeParse({ ...github, askedBy: "" }).success).toBe(false);
  expect(RunGithub.safeParse({ ...github, askedBy: "x".repeat(101) }).success).toBe(false);
});

test("MachineGithub: state 1..32 as an open string, login 1..100", () => {
  expect(MachineGithub.safeParse({ state: "signed_in", login: "example-bot" }).success).toBe(true);
  expect(MachineGithub.safeParse({ state: "a_future_state" }).success).toBe(true);
  expect(MachineGithub.safeParse({ state: "" }).success).toBe(false);
  expect(MachineGithub.safeParse({ state: "x".repeat(33) }).success).toBe(false);
  expect(MachineGithub.safeParse({ state: "signed_in", login: "" }).success).toBe(false);
  expect(MachineGithub.safeParse({ state: "signed_in", login: "x".repeat(101) }).success).toBe(false);
  expect(MachineGithub.safeParse({}).success).toBe(false);
});

test("metrics, Machine and MachineMonitor carry github; the monitor carries runsIn", () => {
  expect(MachineMetrics.parse({ at: 1, runs: [], github: { state: "no_gh" } }).github).toEqual({ state: "no_gh" });
  const machine = { id: "M1", name: "host", labels: [], createdBy: "U1", createdAt: 1, connected: true, healthy: true, runningRuns: [] };
  expect(Machine.parse({ ...machine, github: { state: "signed_out" } }).github).toEqual({ state: "signed_out" });
  expect(Machine.safeParse({ ...machine, github: { login: "x" } }).success).toBe(false);
  const monitor = {
    machine: { id: "M1", name: "host", connected: true },
    series: { range: "15m", from: 0, stepMs: 30_000, cpuPct: [], memPct: [], diskPct: [], netRxBps: [], netTxBps: [] },
    capacity: { slots: 1, running: 0, canTake: 1 },
    queue: { count: 0 },
    today: { finished: 0, succeeded: 0, failed: 0, finishedYesterday: 0, tokens: 0 },
    clis: [],
    runs: [],
    members: [],
  };
  expect(MachineMonitor.parse({ ...monitor, runsIn: "vm", github: { state: "signed_in", login: "b" } }).runsIn).toBe("vm");
  expect(MachineMonitor.safeParse({ ...monitor, runsIn: "" }).success).toBe(false);
  expect(MachineMonitor.safeParse({ ...monitor, runsIn: "x".repeat(17) }).success).toBe(false);
});

test("the github run event takes an optional reason of 1..32 characters", () => {
  expect(RunEvent.safeParse({ type: "github", state: "missing" }).success).toBe(true);
  expect(RunEvent.safeParse({ type: "github", state: "missing", reason: "no_gh" }).success).toBe(true);
  expect(RunEvent.safeParse({ type: "github", state: "missing", reason: "" }).success).toBe(false);
  expect(RunEvent.safeParse({ type: "github", state: "missing", reason: "x".repeat(33) }).success).toBe(false);
});

test("the feature name and the off reason", () => {
  expect(MACHINE_FEATURE_GITHUB_HOST).toBe("github-host");
  expect(GITHUB_UNAVAILABLE_REASONS).toContain("off");
});
